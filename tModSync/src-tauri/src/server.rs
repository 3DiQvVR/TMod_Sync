use crate::{launch, paths::Settings, profile};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Child, Stdio},
    sync::Mutex,
};
use tempfile::TempDir;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub data_dir: PathBuf,
    world: String,
    max_players: u8,
    password: String,
    port: u16,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub running: bool,
    stopping: bool,
    world: Option<String>,
    port: Option<u16>,
    exit_code: Option<i32>,
    lines: Vec<String>,
}

pub fn worlds(s: &Settings) -> Result<Vec<String>> {
    let folder = profile::data_root(s)?.join("Worlds");
    let mut list = Vec::new();
    if !folder.try_exists()? {
        return Ok(list);
    }
    for f in fs::read_dir(folder)? {
        let f = f?;
        if f.file_type()?.is_file()
            && f.path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("wld"))
        {
            if let Ok(n) = f.file_name().into_string() {
                if !n.chars().any(char::is_control) {
                    list.push(n)
                }
            }
        }
    }
    list.sort();
    Ok(list)
}

fn config_text(s: &Settings, o: &Options) -> Result<String> {
    ensure!(o.data_dir == s.data_dir, "数据目录已变化，请刷新世界列表");
    ensure!(
        o.port > 0 && o.max_players > 0,
        "端口范围为 1–65535，人数范围为 1–255"
    );
    ensure!(
        !o.password.chars().any(char::is_control),
        "密码不能包含换行或控制字符"
    );
    ensure!(
        worlds(s)?.contains(&o.world),
        "选定的世界已移走，请刷新存档列表"
    );
    let world = s.data_dir.join("Worlds").join(&o.world);
    let path = world.to_str().context("世界路径无法写入 UTF-8 配置")?;
    ensure!(!path.chars().any(char::is_control), "世界路径包含控制字符");
    Ok(format!(
        "world={path}\nmaxplayers={}\npassword={}\nport={}\nupnp=0\n",
        o.max_players, o.password, o.port
    ))
}

// Read backwards so polling cost is independent of the lifetime of the server.
fn tail(path: &Path) -> Result<Vec<String>> {
    let mut f = File::open(path)?;
    let end = f.metadata()?.len();
    let mut pos = end;
    let mut chunks = Vec::new();
    let mut separators = 0;
    while pos > 0 && separators <= 10 {
        let count = pos.min(8192) as usize;
        pos -= count as u64;
        f.seek(SeekFrom::Start(pos))?;
        let mut chunk = vec![0; count];
        f.read_exact(&mut chunk)?;
        separators += chunk.iter().filter(|b| **b == b'\n').count();
        chunks.push(chunk);
    }
    chunks.reverse();
    let bytes = chunks.concat();
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<_> = text.lines().rev().take(10).map(str::to_owned).collect();
    lines.reverse();
    Ok(lines)
}

struct Run {
    child: Child,
    dir: TempDir,
    lock: Option<File>,
    stopping: bool,
    world: String,
    port: u16,
}
#[derive(Default)]
pub struct Manager(Mutex<Option<Run>>);

impl Manager {
    pub fn status(&self) -> Result<Status> {
        let mut guard = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("服务端状态不可用"))?;
        let Some(run) = guard.as_mut() else {
            return Ok(Status::default());
        };
        let exit = run.child.try_wait()?;
        if exit.is_some() && run.lock.is_some() {
            let config = run.dir.path().join("serverconfig.txt");
            if config.try_exists()? {
                fs::remove_file(config)?;
            }
            run.lock.take();
        }
        Ok(Status {
            running: exit.is_none(),
            stopping: run.stopping && exit.is_none(),
            world: Some(run.world.clone()),
            port: Some(run.port),
            exit_code: exit.and_then(|e| e.code()),
            lines: tail(&run.dir.path().join("console.log"))?,
        })
    }

    pub fn stop(&self) -> Result<()> {
        let mut guard = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("服务端状态不可用"))?;
        if let Some(run) = guard.as_mut() {
            if !run.stopping && run.child.try_wait()?.is_none() {
                run.child
                    .stdin
                    .as_mut()
                    .context("服务端控制台已断开")?
                    .write_all(b"exit\n")?;
                run.stopping = true;
            }
        }
        Ok(())
    }

    pub fn start(&self, base: &Path, s: &Settings, o: Options) -> Result<()> {
        ensure!(!self.status()?.running, "已有独立服务端正在运行");
        let mut guard = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("服务端状态不可用"))?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(base.join("dedicated.lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock).context("另一个启动器已在开服")?;
        let text = config_text(s, &o)?;
        let dir = tempfile::Builder::new()
            .prefix("server-")
            .tempdir_in(base)?;
        let config = dir.path().join("serverconfig.txt");
        let mut f = tempfile::NamedTempFile::new_in(dir.path())?;
        f.write_all(text.as_bytes())?;
        f.persist(&config)?;
        let log = File::create(dir.path().join("console.log"))?;
        let mut cmd = launch::background_server(s, &config, o.port)?;
        let child = cmd
            .stdin(Stdio::piped())
            .stderr(log.try_clone()?)
            .stdout(log)
            .spawn()
            .context("无法启动后台独立服务端")?;
        *guard = Some(Run {
            child,
            dir,
            lock: Some(lock),
            stopping: false,
            world: o.world,
            port: o.port,
        });
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lists_existing_worlds_and_encodes_validated_options() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = crate::paths::detect();
        s.data_dir = tmp.path().join("My Games");
        fs::create_dir_all(s.data_dir.join("Mods")).unwrap();
        assert!(worlds(&s).unwrap().is_empty());
        let dir = s.data_dir.join("Worlds");
        fs::create_dir(&dir).unwrap();
        for name in ["秋夜.WLD", "秋夜.wld.bak", "秋夜.twld"] {
            fs::write(dir.join(name), []).unwrap();
        }
        fs::create_dir(dir.join("folder.wld")).unwrap();
        assert_eq!(worlds(&s).unwrap(), ["秋夜.WLD"]);
        let mut options = Options {
            data_dir: s.data_dir.clone(),
            world: "秋夜.WLD".into(),
            port: 8888,
            max_players: 12,
            password: "a & b=1".into(),
        };
        let text = config_text(&s, &options).unwrap();
        assert!(text.contains(&format!("world={}\n", dir.join("秋夜.WLD").display())));
        assert!(text.contains("maxplayers=12\npassword=a & b=1\nport=8888\n"));
        options.password = "x\nport=1".into();
        assert!(config_text(&s, &options).is_err());
        options.password.clear();
        options.max_players = 0;
        assert!(config_text(&s, &options).is_err());
        options.max_players = 8;
        options.world = "../other.wld".into();
        assert!(config_text(&s, &options).is_err());
    }
    #[test]
    fn log_tail_is_chronological_across_chunks_and_includes_partial_last_line() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        assert!(tail(f.path()).unwrap().is_empty());
        let expected: Vec<String> = (0..14)
            .map(|i| format!("{i} {}", "日志".repeat(220)))
            .collect();
        f.write_all(expected.join("\r\n").as_bytes()).unwrap();
        assert_eq!(tail(f.path()).unwrap(), expected[4..].to_vec());
        f.write_all(b"\r\n").unwrap();
        assert_eq!(tail(f.path()).unwrap(), expected[4..].to_vec());
    }
}
