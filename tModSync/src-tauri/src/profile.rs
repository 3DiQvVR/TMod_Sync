use crate::paths::{read_json, Settings, JSON_LIMIT};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const MAX_ITEMS: usize = 4096;
pub type Configs = BTreeMap<String, Value>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoomProfile {
    pub schema_version: u8,
    pub room_name: String,
    pub mods: Vec<RoomMod>,
    pub configs: Configs,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoomMod {
    pub name: String,
    pub version: String,
    pub sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workshop_id: Option<String>,
}

pub fn workshop_id(id: &str) -> bool {
    !id.starts_with('0') && id.bytes().all(|c| c.is_ascii_digit()) && id.parse::<u64>().is_ok()
}

pub(crate) fn config_name(name: &str) -> Result<()> {
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|p| {
        stem.strip_prefix(p).is_some_and(|n| {
            ["1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"].contains(&n)
        })
    });
    ensure!(
        !stem.is_empty()
            && !reserved
            && name.len() <= 255
            && name.to_ascii_lowercase().ends_with(".json")
            && !name
                .chars()
                .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c)),
        "无效或不可跨平台使用的配置文件名 {name:?}"
    );
    Ok(())
}

fn mod_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.chars().count() <= 200
        && !name
            .chars()
            .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
}

fn unique_names<'a>(items: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for name in items {
        ensure!(seen.insert(name.to_lowercase()), "清单中存在重名项 {name}");
        ensure!(seen.len() <= MAX_ITEMS, "清单项目超过 {MAX_ITEMS} 项");
    }
    Ok(())
}

impl RoomProfile {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema_version == 1, "仅支持 schemaVersion 1");
        ensure!(
            !self.room_name.trim().is_empty() && self.room_name.chars().count() <= 100,
            "房间名称须为 1 至 100 个字符"
        );
        unique_names(self.mods.iter().map(|m| m.name.as_str()))?;
        for m in &self.mods {
            ensure!(
                mod_name(&m.name) && version(&m.version).is_some(),
                "模组名称或版本无效：{}",
                m.name
            );
            ensure!(
                m.sha256.len() == 64
                    && m.sha256
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
                "{} 的 SHA256 无效",
                m.name
            );
            if let Some(id) = &m.workshop_id {
                ensure!(workshop_id(id), "{} 的工坊 ID 无效", m.name);
            }
        }
        unique_names(self.configs.keys().map(String::as_str))?;
        for name in self.configs.keys() {
            config_name(name)?;
        }
        // Includes punctuation and escaping so every exported profile can be imported again.
        ensure!(
            serde_json::to_vec_pretty(self)?.len() as u64 + 1 <= JSON_LIMIT,
            "清单超过大小上限"
        );
        Ok(())
    }
}

// Missing is a distinct state. Never follow a symlink/junction in a managed subpath.
pub(crate) fn entry(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                ensure!(
                    meta.file_attributes() & 0x400 == 0,
                    "目录包含重解析点：{}",
                    path.display()
                );
            }
            ensure!(
                !meta.file_type().is_symlink(),
                "拒绝读写符号链接：{}",
                path.display()
            );
            Ok(Some(meta))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("读取 {}", path.display())),
    }
}

pub(crate) fn data_root(s: &Settings) -> Result<&Path> {
    ensure!(
        s.data_dir.is_absolute()
            && s.data_dir.is_dir()
            && entry(&s.data_dir.join("Mods"))?.is_some_and(|m| m.is_dir()),
        "请在路径设置中选择含 Mods 的 tModLoader 数据目录"
    );
    Ok(&s.data_dir)
}

pub(crate) fn enabled(root: &Path) -> Result<BTreeSet<String>> {
    let path = root.join("Mods/enabled.json");
    if entry(&path)?.is_none() {
        return Ok(BTreeSet::new());
    }
    let names: Vec<String> = read_json(&path)?;
    unique_names(names.iter().map(String::as_str))?;
    ensure!(
        names.iter().all(|n| mod_name(n)),
        "enabled.json 存在无效的模组名称"
    );
    Ok(names.into_iter().collect())
}

pub(crate) fn config_files(root: &Path) -> Result<BTreeMap<String, PathBuf>> {
    let dir = root.join("ModConfigs");
    let mut files = BTreeMap::new();
    if entry(&dir)?.is_none() {
        return Ok(files);
    }
    let mut bytes = 0u64;
    for file in fs::read_dir(&dir).with_context(|| format!("读取 {}", dir.display()))? {
        let file = file?;
        let path = file.path();
        if !has_ext(&path, "json") {
            continue;
        }
        let meta = entry(&path)?.context("配置文件在扫描时发生变化")?;
        if meta.is_dir() {
            continue;
        }
        ensure!(meta.is_file(), "配置项不是常规文件 {}", path.display());
        let name = file
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("配置文件名不是 UTF-8"))?;
        config_name(&name)?;
        bytes = bytes.saturating_add(meta.len());
        ensure!(
            bytes <= JSON_LIMIT && files.len() < MAX_ITEMS,
            "ModConfigs 配置超过清单大小上限"
        );
        files.insert(name, path);
    }
    unique_names(files.keys().map(String::as_str))?;
    Ok(files)
}

fn configs(root: &Path) -> Result<Configs> {
    config_files(root)?
        .into_iter()
        .map(|(name, path)| Ok((name, read_json(&path)?)))
        .collect()
}

fn has_ext(path: &Path, ext: &str) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case(ext))
}

// -1 represents a missing component, following System.Version ordering.
type Version = [i32; 4];
fn version(s: &str) -> Option<Version> {
    if s.len() > 43 {
        return None;
    }
    let mut v = [-1; 4];
    for (i, part) in s.split('.').enumerate() {
        if i >= 4 || !part.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        v[i] = part.parse().ok()?;
    }
    (v[1] >= 0).then_some(v)
}

fn branch(s: &Settings) -> Result<Option<Version>> {
    if !s.branch.trim().is_empty() {
        return Ok(Some(
            version(s.branch.trim()).context("tModLoader 分支版本无效")?,
        ));
    }
    let path = s.data_dir.join("config.json");
    if !path.is_file() {
        return Ok(None);
    }
    let cfg: Value = read_json(&path)?;
    Ok(cfg
        .get("LastLaunchedTModLoaderVersion")
        .and_then(Value::as_str)
        .and_then(version))
}

fn stream(v: Version) -> u8 {
    if v < [0, 12, -1, -1] {
        0
    } else if v < [2022, 10, -1, -1] {
        1
    } else if v < [2023, 3, 85, -1] {
        2
    } else {
        3
    }
}

fn dotnet_string(reader: &mut impl Read) -> Result<String> {
    let mut length = 0usize;
    for shift in (0..=14).step_by(7) {
        let mut byte = [0];
        reader.read_exact(&mut byte)?;
        length |= usize::from(byte[0] & 127) << shift;
        if byte[0] < 128 {
            ensure!(length <= 1024, ".tmod 字符串过长");
            let mut text = vec![0; length];
            reader.read_exact(&mut text)?;
            return Ok(String::from_utf8(text)?);
        }
    }
    bail!(".tmod 字符串长度无效")
}

#[derive(Debug)]
pub(crate) struct Package {
    pub path: PathBuf,
    pub workshop_id: Option<String>,
    pub name: String,
    pub version: String,
    mod_version: Version,
    loader: Version,
    payload_hash: [u8; 20],
}

impl Package {
    pub(crate) fn read(path: &Path, workshop_id: Option<String>) -> Result<Self> {
        let mut file = BufReader::new(File::open(path)?);
        let mut magic = [0; 4];
        file.read_exact(&mut magic)?;
        ensure!(&magic == b"TMOD", "非法 .tmod 文件头");
        let loader = version(&dotnet_string(&mut file)?).context("无效的 tML 版本号")?;
        ensure!(loader >= [0, 11, -1, -1], "暂不支持旧压缩格式");
        let mut payload_hash = [0; 20];
        file.read_exact(&mut payload_hash)?;
        file.seek_relative(256)?;
        let mut size = [0; 4];
        file.read_exact(&mut size)?;
        let size = i32::from_le_bytes(size);
        let end = file
            .stream_position()?
            .checked_add(size.try_into()?)
            .context(".tmod 长度溢出")?;
        ensure!(end == file.get_ref().metadata()?.len(), ".tmod 数据不完整");
        let name = dotnet_string(&mut file)?;
        ensure!(mod_name(&name), ".tmod 内部名称无效");
        let version_text = dotnet_string(&mut file)?;
        let mod_version = version(&version_text).context(".tmod 版本号无效")?;
        Ok(Self {
            path: path.into(),
            workshop_id,
            name,
            version: version_text,
            mod_version,
            loader,
            payload_hash,
        })
    }

    pub(crate) fn summary(&self) -> Result<RoomMod> {
        let mut file = File::open(&self.path)?;
        let mut hash = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        Ok(RoomMod {
            name: self.name.clone(),
            version: self.version.clone(),
            workshop_id: self.workshop_id.clone(),
            sha256: hex::encode(hash.finalize()),
        })
    }
}

// Use the loadable TMOD header (including renamed files), never the folder label as a mod version.
pub(crate) fn catalog(
    s: &Settings,
    wanted: &BTreeSet<String>,
) -> Result<BTreeMap<String, Vec<Package>>> {
    let mut packages: BTreeMap<String, Vec<Package>> = BTreeMap::new();
    let mut add = |path: &Path, id: Option<String>| -> Result<()> {
        let Ok(package) = Package::read(path, id) else {
            // tML skips unreadable or obsolete packages when collecting available mods.
            return Ok(());
        };
        if wanted.contains(&package.name) {
            packages
                .entry(package.name.clone())
                .or_default()
                .push(package);
        }
        Ok(())
    };
    for file in fs::read_dir(s.data_dir.join("Mods"))? {
        let file = file?;
        if has_ext(&file.path(), "tmod") && file.path().is_file() {
            add(&file.path(), None)?;
        }
    }
    if !s.workshop_dir.as_os_str().is_empty() && s.workshop_dir.try_exists()? {
        let dirs = fs::read_dir(&s.workshop_dir).context("无法扫描已配置的 Steam 创意工坊目录")?;
        for item in dirs {
            let item = item?;
            let id = item.file_name().to_string_lossy().into_owned();
            if !workshop_id(&id) || !item.path().is_dir() {
                continue;
            }
            for file in WalkDir::new(item.path()).follow_links(false) {
                let file = file?;
                if file.file_type().is_file() && has_ext(file.path(), "tmod") {
                    add(file.path(), Some(id.clone()))?;
                }
            }
        }
    }
    Ok(packages)
}

fn choose<'a>(items: &'a [Package], current: Option<Version>) -> Result<&'a Package> {
    let mut pool: Vec<_> = items.iter().collect();
    let first = *pool.first().context("模组不存在")?;
    if let Some(loader) = current {
        pool.retain(|p| {
            stream(p.loader) == stream(loader)
                && stream(p.loader) != 2
                && p.loader[..2] <= loader[..2]
        });
        ensure!(
            !pool.is_empty(),
            "{} 没有适用于当前 tModLoader 分支的文件",
            first.name
        );
    } else {
        ensure!(
            pool.iter().all(|p| p.loader[..2] == first.loader[..2]),
            "{} 有多个 tModLoader 分支，请在路径设置中填写当前游戏的版本月份",
            first.name
        );
    }
    let best = pool
        .iter()
        .map(|p| (p.mod_version, p.loader))
        .max()
        .unwrap();
    pool.retain(|p| (p.mod_version, p.loader) == best);
    let ws_hashes: BTreeSet<_> = pool
        .iter()
        .filter(|p| p.workshop_id.is_some())
        .map(|p| p.payload_hash)
        .collect();
    pool.retain(|p| p.workshop_id.is_some() || !ws_hashes.contains(&p.payload_hash));
    let rank = |p: &Package| {
        (
            p.workshop_id.is_none(),
            p.path.file_stem().and_then(|s| s.to_str()) == Some(&p.name),
        )
    };
    let top = pool.iter().map(|p| rank(p)).max().unwrap();
    pool.retain(|p| rank(p) == top);
    if pool.len() > 1 {
        let first_hash = pool[0].summary()?.sha256;
        for item in &pool[1..] {
            ensure!(
                item.summary()?.sha256 == first_hash,
                "{} 同一版本有多份不同文件，请移走冲突副本",
                first.name
            );
        }
    }
    Ok(pool[0])
}

pub fn capture(s: &Settings, room_name: String) -> Result<RoomProfile> {
    let root = data_root(s)?;
    crate::sync::recover(s)?;
    let names = enabled(root)?;
    let packages = catalog(s, &names)?;
    let current = branch(s)?;
    let mut mods = Vec::new();
    for name in names {
        let items = packages
            .get(&name)
            .with_context(|| format!("找不到已启用的 {name}，请检查 Mods 和创意工坊路径"))?;
        mods.push(choose(items, current)?.summary()?);
    }
    let room = RoomProfile {
        schema_version: 1,
        room_name,
        mods,
        configs: configs(root)?,
    };
    room.validate()?;
    Ok(room)
}

#[derive(Default, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    pub missing: Vec<String>,
    pub mismatched: Vec<String>,
    pub extra: Vec<String>,
    pub disabled: Vec<String>,
    pub configs: Vec<String>,
    pub matched: usize,
    pub details: BTreeMap<String, String>,
}

impl Diff {
    pub fn aligned(&self) -> bool {
        self.missing.is_empty()
            && self.mismatched.is_empty()
            && self.extra.is_empty()
            && self.disabled.is_empty()
            && self.configs.is_empty()
    }
}

pub fn compare(s: &Settings, room: &RoomProfile, require_local: bool) -> Result<Diff> {
    room.validate()?;
    let root = data_root(s)?;
    crate::sync::recover(s)?;
    let local_enabled = enabled(root)?;
    let wanted = room.mods.iter().map(|m| m.name.clone()).collect();
    let installed = catalog(s, &wanted)?;
    let current = branch(s)?;
    let mut diff = Diff {
        extra: local_enabled.difference(&wanted).cloned().collect(),
        ..Default::default()
    };
    for m in &room.mods {
        let Some(items) = installed.get(&m.name) else {
            diff.missing.push(m.name.clone());
            continue;
        };
        if require_local {
            let mut present = false;
            let mut has_local = false;
            for p in items.iter().filter(|p| p.workshop_id.is_none()) {
                has_local = true;
                if p.version == m.version && p.summary()?.sha256 == m.sha256 {
                    present = true;
                    break;
                }
            }
            if !present {
                if has_local {
                    diff.mismatched.push(m.name.clone())
                } else {
                    diff.missing.push(m.name.clone())
                }
                let mut can_copy = false;
                for p in items
                    .iter()
                    .filter(|p| p.workshop_id.is_some() && p.version == m.version)
                {
                    if p.summary()?.sha256 == m.sha256 {
                        can_copy = true;
                        break;
                    }
                }
                diff.details.insert(
                    m.name.clone(),
                    if can_copy {
                        "工坊已有相同 SHA-256 的文件，可直接同步到 Mods。"
                    } else {
                        "未找到清单指定的文件，请订阅或向房主获取对应版本。"
                    }
                    .into(),
                );
                continue;
            }
        }
        let found = choose(items, current).and_then(Package::summary);
        if !local_enabled.contains(&m.name) {
            diff.disabled.push(m.name.clone());
        }
        let reason = match found {
            Ok(local) if local.version != m.version => {
                Some(format!("本地 {} · 房间 {}", local.version, m.version))
            }
            Ok(local) if local.sha256 != m.sha256 => Some(format!(
                "同版本文件 SHA256 不同 · 本地 {} / 房间 {}",
                local.sha256, m.sha256
            )),
            Ok(_) => None,
            Err(e) => Some(e.to_string()),
        };
        if let Some(text) = reason {
            diff.mismatched.push(m.name.clone());
            diff.details.insert(m.name.clone(), text);
        } else if local_enabled.contains(&m.name) {
            diff.matched += 1;
        }
    }
    let local = config_files(root)?;
    let keys: BTreeSet<_> = local.keys().chain(room.configs.keys()).collect();
    for key in keys {
        let same = match (local.get(key), room.configs.get(key)) {
            (Some(path), Some(expected)) => {
                read_json::<Value>(path).is_ok_and(|value| &value == expected)
            }
            _ => false,
        };
        if !same {
            diff.configs.push(key.clone());
        }
    }
    Ok(diff)
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
