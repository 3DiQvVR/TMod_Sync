use crate::paths::{read_json, write_json, Settings, JSON_LIMIT};
use crate::profile::{config_files, config_name, data_root, entry, RoomProfile};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tempfile::{Builder, NamedTempFile};

const BACKUPS: &str = ".tmodsync-backups";
const JOURNAL: &str = ".tmodsync-pending.json";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Snapshot {
    enabled_existed: bool,
    original_configs: Vec<String>,
    incoming_configs: Vec<String>,
    #[serde(default)]
    original_mods: Vec<String>,
    #[serde(default)]
    incoming_mods: Vec<String>,
}

fn directory(path: &Path, create: bool) -> Result<()> {
    match entry(path)? {
        Some(meta) => ensure!(meta.is_dir(), "需要目录 {}", path.display()),
        None if create => fs::create_dir(path)?,
        None => anyhow::bail!("缺少目录 {}", path.display()),
    }
    Ok(())
}

fn regular_file(path: &Path) -> Result<bool> {
    let Some(meta) = entry(path)? else {
        return Ok(false);
    };
    ensure!(meta.is_file(), "需要常规文件 {}", path.display());
    Ok(true)
}

fn remove_file(path: &Path) -> Result<()> {
    if regular_file(path)? {
        fs::remove_file(path)?;
    }
    Ok(())
}

// Used for byte-exact backups, including the game's original formatting and BOM.
fn copy_file(source: &Path, destination: &Path) -> Result<()> {
    ensure!(regular_file(source)?, "备份来源不存在 {}", source.display());
    regular_file(destination)?;
    let mut input = File::open(source)?;
    ensure!(
        source
            .extension()
            .is_some_and(|v| v.eq_ignore_ascii_case("tmod"))
            || input.metadata()?.len() <= JSON_LIMIT,
        "文件超过大小上限"
    );
    let mut temp = NamedTempFile::new_in(destination.parent().context("缺少目标目录")?)?;
    io::copy(&mut input, &mut temp)?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    temp.persist(destination)?;
    Ok(())
}

fn validate_snapshot(snapshot: &Snapshot) -> Result<()> {
    for name in snapshot.original_mods.iter().chain(&snapshot.incoming_mods) {
        crate::sync_mods::filename(name)?;
    }
    for names in [&snapshot.original_configs, &snapshot.incoming_configs] {
        let mut seen = BTreeSet::new();
        ensure!(names.len() <= 4096, "备份项目过多");
        for name in names {
            config_name(name)?;
            ensure!(seen.insert(name.to_lowercase()), "备份清单中存在重名文件");
        }
    }
    Ok(())
}

fn snapshot(
    root: &Path,
    files: &BTreeMap<String, PathBuf>,
    room: &RoomProfile,
    repairs: &crate::sync_mods::Repairs,
) -> Result<PathBuf> {
    let backups = root.join(BACKUPS);
    directory(&backups, true)?;
    let temp = Builder::new().prefix("snapshot-").tempdir_in(&backups)?;
    let configs = temp.path().join("ModConfigs");
    fs::create_dir(&configs)?;
    for (name, path) in files {
        copy_file(path, &configs.join(name))?;
    }
    let enabled = root.join("Mods/enabled.json");
    let enabled_existed = regular_file(&enabled)?;
    if enabled_existed {
        copy_file(&enabled, &temp.path().join("enabled.json"))?;
    }
    if !repairs.originals.is_empty() {
        let dir = temp.path().join("Mods");
        fs::create_dir(&dir)?;
        for name in &repairs.originals {
            copy_file(&root.join("Mods").join(name), &dir.join(name))?;
        }
    }
    let manifest = Snapshot {
        original_mods: repairs.originals.clone(),
        incoming_mods: repairs
            .installs
            .iter()
            .map(|(name, _)| name.clone())
            .collect(),
        enabled_existed,
        original_configs: files.keys().cloned().collect(),
        incoming_configs: room.configs.keys().cloned().collect(),
    };
    write_json(&temp.path().join("manifest.json"), &manifest)?;
    Ok(temp.keep())
}

pub fn recover(s: &Settings) -> Result<()> {
    let root = data_root(s)?;
    let journal = root.join(JOURNAL);
    if !regular_file(&journal)? {
        return Ok(());
    }
    let id: String = read_json(&journal)?;
    ensure!(
        id.len() <= 80
            && id
                .strip_prefix("snapshot-")
                .is_some_and(|tail| !tail.is_empty()
                    && tail
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')),
        "无效的同步恢复记录"
    );
    let backups = root.join(BACKUPS);
    directory(&backups, false)?;
    let backup = backups.join(id);
    directory(&backup, false)?;
    let manifest = backup.join("manifest.json");
    ensure!(regular_file(&manifest)?, "同步恢复记录缺少 manifest.json");
    let saved: Snapshot = read_json(&manifest)?;
    validate_snapshot(&saved)?;
    let originals = backup.join("ModConfigs");
    directory(&originals, false)?;
    // Validate every backup before touching current files.
    for name in &saved.original_configs {
        ensure!(regular_file(&originals.join(name))?, "备份缺少 {name}");
    }
    if saved.enabled_existed {
        ensure!(
            regular_file(&backup.join("enabled.json"))?,
            "备份缺少 enabled.json"
        );
    }

    let mods = root.join("Mods");
    let saved_mods = backup.join("Mods");
    if !saved.original_mods.is_empty() {
        directory(&saved_mods, false)?;
    }
    for name in &saved.original_mods {
        ensure!(regular_file(&saved_mods.join(name))?, "备份缺少 {name}");
    }
    for name in &saved.incoming_mods {
        if !saved.original_mods.contains(name) {
            remove_file(&mods.join(name))?;
        }
    }
    for name in &saved.original_mods {
        copy_file(&saved_mods.join(name), &mods.join(name))?;
    }
    let live = root.join("ModConfigs");
    directory(&live, true)?;
    for name in &saved.incoming_configs {
        if !saved.original_configs.contains(name) {
            remove_file(&live.join(name))?;
        }
    }
    for name in &saved.original_configs {
        copy_file(&originals.join(name), &live.join(name))?;
    }
    let enabled = root.join("Mods/enabled.json");
    if saved.enabled_existed {
        copy_file(&backup.join("enabled.json"), &enabled)?;
    } else {
        remove_file(&enabled)?;
    }
    fs::remove_file(journal)?;
    Ok(())
}

pub fn apply(s: &Settings, room: &RoomProfile) -> Result<Option<PathBuf>> {
    room.validate()?;
    let root = data_root(s)?;
    recover(s)?;
    let files = config_files(root)?;
    let repairs = crate::sync_mods::prepare(s, room)?;
    let desired: Vec<_> = room.mods.iter().map(|m| m.name.clone()).collect();
    let enabled_path = root.join("Mods/enabled.json");
    regular_file(&enabled_path)?;
    let enabled_same = regular_file(&enabled_path)?
        && read_json::<Vec<String>>(&enabled_path).is_ok_and(|names| names == desired);
    let configs = root.join("ModConfigs");
    let mut changed = Vec::new();
    for (name, value) in &room.configs {
        let path = configs.join(name);
        regular_file(&path)?;
        if !files.contains_key(name)
            || read_json::<serde_json::Value>(&path).ok().as_ref() != Some(value)
        {
            changed.push((name, value));
        }
    }
    let redundant: Vec<_> = files
        .iter()
        .filter(|(name, _)| !room.configs.contains_key(*name))
        .collect();
    if enabled_same
        && changed.is_empty()
        && redundant.is_empty()
        && repairs.installs.is_empty()
        && repairs.originals.is_empty()
    {
        return Ok(None);
    }

    let backup = snapshot(root, &files, room, &repairs)?;
    let journal = root.join(JOURNAL);
    write_json(
        &journal,
        &backup
            .file_name()
            .context("备份目录缺少名称")?
            .to_str()
            .context("备份名称编码错误")?,
    )?;
    let commit = || -> Result<()> {
        let mods = root.join("Mods");
        for name in &repairs.originals {
            remove_file(&mods.join(name))?;
        }
        for (name, file) in repairs.installs {
            file.persist_noclobber(mods.join(name))?;
        }
        // Remove prior spellings first so Windows and Linux converge on the same canonical names.
        if !changed.is_empty() {
            directory(&configs, true)?;
        }
        for (_, path) in redundant {
            remove_file(path)?;
        }
        for (name, value) in changed {
            write_json(&configs.join(name), value)?;
        }
        if !enabled_same {
            write_json(&enabled_path, &desired)?;
        }
        fs::remove_file(&journal)?;
        Ok(())
    };
    if let Err(error) = commit() {
        recover(s).with_context(|| {
            format!(
                "同步失败（{error:#}）；自动恢复中断。备份位于 {}",
                backup.display()
            )
        })?;
        return Err(error).context("同步未完成，已恢复原配置");
    }
    Ok(Some(backup))
}
