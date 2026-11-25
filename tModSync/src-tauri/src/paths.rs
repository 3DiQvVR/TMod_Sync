use crate::network::Provider;
use anyhow::{ensure, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const JSON_LIMIT: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub data_dir: PathBuf,
    pub install_dir: PathBuf,
    pub workshop_dir: PathBuf,
    #[serde(default)]
    pub branch: String,
    pub provider: Provider,
    #[serde(default)]
    pub network_id: String,
    pub port: u16,
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        for dir in [&self.data_dir, &self.install_dir, &self.workshop_dir] {
            ensure!(
                dir.as_os_str().is_empty() || dir.is_absolute(),
                "请输入绝对路径：{}",
                dir.display()
            );
        }
        ensure!(self.port > 0, "端口必须在 1–65535 之间");
        ensure!(
            self.workshop_dir.as_os_str().is_empty()
                || self.workshop_dir.ends_with("content/1281930"),
            "请选择 Steam 创意工坊的 content/1281930 目录"
        );
        ensure!(
            self.network_id.is_empty()
                || (self.network_id.len() == 16
                    && self.network_id.bytes().all(|b| b.is_ascii_hexdigit())),
            "ZeroTier 网络 ID 需要 16 位十六进制字符"
        );
        ensure!(
            self.branch.is_empty()
                || regex::Regex::new(r"^20[0-9]{2}\.(0?[1-9]|1[0-2])$")?.is_match(&self.branch),
            "分支月份格式应为 YYYY.MM"
        );
        Ok(())
    }
}

#[derive(Serialize)]
pub struct PathStatus {
    pub data: bool,
    pub install: bool,
    pub workshop: bool,
}

pub fn status(s: &Settings) -> PathStatus {
    let client = if cfg!(windows) {
        "start-tModLoader.bat"
    } else {
        "start-tModLoader.sh"
    };
    let server = if cfg!(windows) {
        "start-tModLoaderServer.bat"
    } else {
        "start-tModLoaderServer.sh"
    };
    PathStatus {
        data: !s.data_dir.as_os_str().is_empty() && s.data_dir.join("Mods").is_dir(),
        install: !s.install_dir.as_os_str().is_empty()
            && s.install_dir.join(client).is_file()
            && s.install_dir.join(server).is_file(),
        workshop: !s.workshop_dir.as_os_str().is_empty() && s.workshop_dir.is_dir(),
    }
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !path.as_os_str().is_empty() && !paths.contains(&path) {
        paths.push(path);
    }
}

fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    #[cfg(windows)]
    {
        use winreg::{
            enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY},
            RegKey,
        };
        if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam") {
            if let Ok(s) = key.get_value::<String, _>("SteamPath") {
                push_unique(&mut roots, s.into());
            }
        }
        if let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey_with_flags(r"SOFTWARE\Valve\Steam", KEY_READ | KEY_WOW64_32KEY)
        {
            if let Ok(s) = key.get_value::<String, _>("InstallPath") {
                push_unique(&mut roots, s.into());
            }
        }
        for name in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(p) = env::var_os(name) {
                push_unique(&mut roots, PathBuf::from(p).join("Steam"));
            }
        }
    }
    if let Some(home) = dirs::home_dir() {
        for relative in [
            "Library/Application Support/Steam",
            ".steam/steam",
            ".local/share/Steam",
            ".var/app/com.valvesoftware.Steam/.local/share/Steam",
        ] {
            let p = home.join(relative);
            if p.is_dir() {
                push_unique(&mut roots, p);
            }
        }
    }
    roots
}

// Only top-level library entries and "path" properties have absolute directory values.
fn vdf_libraries(vdf: &str) -> Vec<PathBuf> {
    let entries =
        regex::Regex::new(r#"(?mi)^\s*"(?:path|[0-9]+)"[ \t]+"((?:\\.|[^"\\])*)""#).unwrap();
    entries
        .captures_iter(vdf)
        .map(|c| PathBuf::from(c[1].replace(r"\\", r"\").replace(r#"\""#, "\"")))
        .filter(|p| p.is_absolute())
        .collect()
}

fn locate_steam(mut roots: Vec<PathBuf>) -> (PathBuf, PathBuf) {
    for root in roots.clone() {
        if let Ok(vdf) = fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
            for lib in vdf_libraries(&vdf) {
                push_unique(&mut roots, lib);
            }
        }
    }
    let install_sub = "steamapps/common/tModLoader";
    let workshop_sub = "steamapps/workshop/content/1281930";
    let game_root = roots.iter().find(|r| r.join(install_sub).is_dir());
    let install = game_root.map(|r| r.join(install_sub)).unwrap_or_default();
    let workshop = game_root
        .map(|r| r.join(workshop_sub))
        .filter(|r| r.is_dir())
        .or_else(|| {
            roots
                .iter()
                .map(|r| r.join(workshop_sub))
                .find(|r| r.is_dir())
        })
        .unwrap_or_default();
    (install, workshop)
}

fn locate_data(install: &Path) -> PathBuf {
    let mut roots = Vec::new();
    if !install.as_os_str().is_empty() && install.join("savehere.txt").is_file() {
        push_unique(&mut roots, install.join("tModLoader"));
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(p) = dirs::data_dir() {
            push_unique(&mut roots, p.join("Terraria/tModLoader"));
        }
        if let Some(p) = dirs::home_dir() {
            push_unique(
                &mut roots,
                p.join(".var/app/com.valvesoftware.Steam/.local/share/Terraria/tModLoader"),
            );
        }
    }
    #[cfg(target_os = "macos")]
    if let Some(p) = dirs::data_dir() {
        push_unique(&mut roots, p.join("Terraria/tModLoader"));
    }
    if let Some(p) = dirs::document_dir() {
        push_unique(&mut roots, p.join("My Games/Terraria/tModLoader"));
    }
    if let Some(p) = dirs::home_dir() {
        push_unique(&mut roots, p.join("Documents/My Games/Terraria/tModLoader"));
    }
    if let Some(p) = env::var_os("OneDrive") {
        push_unique(
            &mut roots,
            PathBuf::from(p).join("Documents/My Games/Terraria/tModLoader"),
        );
    }
    roots
        .iter()
        .find(|p| p.join("Mods").is_dir())
        .cloned()
        .or_else(|| roots.iter().find(|p| p.is_dir()).cloned())
        .unwrap_or_default()
}

pub fn detect() -> Settings {
    let (install_dir, workshop_dir) = locate_steam(steam_roots());
    Settings {
        data_dir: locate_data(&install_dir),
        install_dir,
        workshop_dir,
        branch: String::new(),
        provider: crate::network::inventory().preferred(),
        network_id: String::new(),
        port: 7777,
    }
}

pub fn load(config_dir: &Path) -> Result<Settings> {
    let file = config_dir.join("config.json");
    if !file.try_exists()? {
        return Ok(detect());
    }
    let settings: Settings = read_json(&file)?;
    settings.validate()?;
    Ok(settings)
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .with_context(|| format!("无法打开 {}", path.display()))?
        .take(JSON_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= JSON_LIMIT,
        "JSON 超过 32 MiB：{}",
        path.display()
    );
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    serde_json::from_slice(bytes).with_context(|| format!("JSON 格式错误：{}", path.display()))
}

pub fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().context("找不到目标目录")?;
    let bytes = serde_json::to_vec_pretty(value)?;
    ensure!((bytes.len() as u64) < JSON_LIMIT, "JSON 超过 32 MiB");
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&bytes)?;
    temp.write_all(b"\n")?;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .with_context(|| format!("写入失败：{}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secondary_library_and_workshop() {
        let tmp = tempfile::tempdir().unwrap();
        let first = tmp.path().join("Steam");
        let second = tmp.path().join("Library with spaces");
        fs::create_dir_all(first.join("steamapps")).unwrap();
        let install = second.join("steamapps/common/tModLoader");
        let workshop = second.join("steamapps/workshop/content/1281930");
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(&workshop).unwrap();
        fs::write(
            first.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\"1\"\n{{\n\"path\" \"{}\"\n}}\n}}",
                second.display().to_string().replace('\\', "\\\\")
            ),
        )
        .unwrap();
        assert_eq!(locate_steam(vec![first]), (install, workshop));
    }

    #[test]
    fn persisted_settings_win_over_detection() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = detect();
        s.port = 8888;
        write_json(&dir.path().join("config.json"), &s).unwrap();
        assert_eq!(load(dir.path()).unwrap(), s);
        s.port = 0;
        assert!(s.validate().is_err());
    }
}
