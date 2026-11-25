use crate::paths::Settings;
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::{
    ffi::OsString,
    net::IpAddr,
    path::{Path, PathBuf},
    process::{Child, Command},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Client,
    Server,
}

fn script_name(target: Target) -> &'static str {
    match (target, cfg!(windows)) {
        (Target::Client, true) => "start-tModLoader.bat",
        (Target::Server, true) => "start-tModLoaderServer.bat",
        (Target::Client, false) => "start-tModLoader.sh",
        (Target::Server, false) => "start-tModLoaderServer.sh",
    }
}

// tML accepts the parent of content, while profile discovery operates on the app-ID directory.
fn workshop_root(content: &Path) -> Result<&Path> {
    let parent = content.parent().context("创意工坊路径为空")?;
    ensure!(
        content.ends_with("content/1281930"),
        "创意工坊目录应指向 workshop/content/1281930"
    );
    parent.parent().context("找不到创意工坊根目录")
}

fn launch_args(
    data: &Path,
    workshop: &Path,
    port: u16,
    target: Target,
    host: Option<IpAddr>,
) -> Result<Vec<OsString>> {
    ensure!(port > 0, "端口必须为 1 至 65535");
    let mut args = vec![OsString::from("-tmlsavedirectory"), data.as_os_str().into()];
    if !workshop.as_os_str().is_empty() {
        args.push("-steamworkshopfolder".into());
        args.push(workshop_root(workshop)?.as_os_str().into());
    }
    match (target, host) {
        (Target::Server, _) => args.push("-nosteam".into()),
        (Target::Client, Some(ip)) => {
            ensure!(
                !ip.is_unspecified() && !ip.is_multicast(),
                "请填写房主的虚拟局域网 IP"
            );
            args.extend(["-join".into(), ip.to_string().into()]);
        }
        (Target::Client, None) => (),
    }
    args.extend(["-port".into(), port.to_string().into()]);
    Ok(args)
}

fn needs_intact_path(path: &Path) -> bool {
    // The upstream server wrapper flattens argv into a string then expands it unquoted.
    // Terraria rejoins ordinary space-separated words, but these characters change the path.
    let text = path.as_os_str().to_string_lossy();
    text.contains(['*', '?', '[', '\t', '\n', '\r'])
        || text.contains("  ")
        || text.contains(" -")
        || text.contains(" +")
        || text.ends_with(' ')
}

#[cfg(windows)]
fn spawn_game(script: &Path, args: &[OsString], target: Target) -> Result<Child> {
    use std::os::windows::process::CommandExt;
    // Let Rust handle batch-file quoting. No arguments are interpolated into a shell command.
    let mut cmd = Command::new(script);
    cmd.current_dir(script.parent().unwrap()).args(args);
    let flags = if target == Target::Server {
        0x10
    } else {
        0x08000000
    };
    Ok(cmd.creation_flags(flags).spawn()?)
}

#[cfg(target_os = "macos")]
fn spawn_game(script: &Path, args: &[OsString], target: Target) -> Result<Child> {
    if target == Target::Client {
        return Ok(Command::new("/bin/bash").arg(script).args(args).spawn()?);
    }
    // Terminal supplies the interactive TTY for world selection. AppleScript's quoted form
    // quotes every separate argv item on Terminal's side, including user-chosen paths.
    const OPEN_TERMINAL: &str = r#"on run argv
        set launchLine to "exec /bin/bash"
        repeat with itemValue in argv
            set launchLine to launchLine & " " & quoted form of (contents of itemValue)
        end repeat
        tell application "Terminal"
            activate
            do script launchLine
        end tell
    end run"#;
    Ok(Command::new("/usr/bin/osascript")
        .args(["-e", OPEN_TERMINAL])
        .arg(script)
        .args(args)
        .spawn()?)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn spawn_game(script: &Path, args: &[OsString], target: Target) -> Result<Child> {
    if target == Target::Client {
        return Ok(Command::new("/bin/bash").arg(script).args(args).spawn()?);
    }
    for (terminal, option) in [
        ("x-terminal-emulator", "-e"),
        ("gnome-terminal", "--"),
        ("konsole", "-e"),
        ("xfce4-terminal", "-x"),
        ("xterm", "-e"),
    ] {
        match Command::new(terminal)
            .args([option, "/bin/bash"])
            .arg(script)
            .args(args)
            .spawn()
        {
            Ok(child) => return Ok(child),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e).context("无法打开服务端终端"),
        }
    }
    anyhow::bail!("请选择已安装终端模拟器的桌面环境，以便服务端接受世界选择和控制台输入")
}

pub fn start(settings: &Settings, target: Target, host: Option<IpAddr>) -> Result<()> {
    ensure!(
        settings.install_dir.is_absolute() && settings.install_dir.is_dir(),
        "请先设置 tModLoader 安装目录"
    );
    ensure!(
        settings.data_dir.is_absolute() && settings.data_dir.is_dir(),
        "游戏数据目录不存在"
    );
    if !settings.workshop_dir.as_os_str().is_empty() {
        ensure!(
            settings.workshop_dir.is_absolute() && settings.workshop_dir.is_dir(),
            "请检查创意工坊目录"
        );
    }
    let mut script: PathBuf = settings.install_dir.join(script_name(target));
    ensure!(script.is_file(), "缺少官方启动脚本 {}", script.display());
    let mut args = launch_args(
        &settings.data_dir,
        &settings.workshop_dir,
        settings.port,
        target,
        host,
    )?;
    if target == Target::Server
        && (needs_intact_path(&settings.data_dir) || needs_intact_path(&settings.workshop_dir))
    {
        // Use the same official bootstrap that start-tModLoaderServer ultimately invokes.
        // The Windows client batch file directly forwards argv to this bootstrap, too.
        let bootstrap = if cfg!(windows) {
            "start-tModLoader.bat"
        } else {
            "LaunchUtils/ScriptCaller.sh"
        };
        script = settings.install_dir.join(bootstrap);
        ensure!(script.is_file(), "缺少官方启动脚本 {}", script.display());
        args.extend([
            "-server".into(),
            "-config".into(),
            "serverconfig.txt".into(),
        ]);
    }
    let mut child = spawn_game(&script, &args, target)
        .with_context(|| format!("无法运行 {}", script.display()))?;
    // Reap the launcher when it exits without tying the game's lifetime to this window.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub fn background_server(s: &Settings, config: &Path, port: u16) -> Result<Command> {
    let script = s.install_dir.join(script_name(Target::Server));
    ensure!(
        s.install_dir.is_absolute() && script.is_file(),
        "找不到独立服务端脚本 {}",
        script.display()
    );
    let intact = [&s.data_dir, &s.workshop_dir, config]
        .iter()
        .any(|p| needs_intact_path(p));
    let mut args = launch_args(&s.data_dir, &s.workshop_dir, port, Target::Server, None)?;
    args.extend(["-config".into(), config.as_os_str().into()]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let helper = s.install_dir.join("LaunchUtils/busybox-sh.bat");
        let mut cmd = if helper.is_file() {
            // The stock helper uses START, which detaches busybox from our pipes. Run the
            // official batch entry in a private directory with an attached busybox helper.
            let dir = config.parent().context("配置目录不存在")?;
            let stage = dir.join("start-tModLoaderServer.bat");
            std::fs::copy(&script, &stage)?;
            std::fs::create_dir(dir.join("LaunchUtils"))?;
            let busybox = s.install_dir.join("LaunchUtils/busybox64.exe");
            ensure!(busybox.is_file(), "缺少官方 BusyBox 启动程序");
            let entry = if intact {
                "./LaunchUtils/ScriptCaller.sh -server"
            } else {
                "./start-tModLoaderServer.sh"
            };
            let workshop = if s.workshop_dir.as_os_str().is_empty() {
                ""
            } else {
                " -steamworkshopfolder \"%TMS_WORKSHOP%\""
            };
            let bridge=format!("@echo off\r\nsetlocal DisableDelayedExpansion\r\ncd /d \"%TMS_INSTALL%\" || exit /b 1\r\n\"%TMS_BUSYBOX%\" bash {entry} -nosteam -config \"%TMS_CONFIG%\" -tmlsavedirectory \"%TMS_DATA%\"{workshop} -port {port}\r\nexit /b %errorlevel%\r\n");
            std::fs::write(dir.join("LaunchUtils/busybox-sh.bat"), bridge)?;
            let mut cmd = Command::new(stage);
            cmd.env("TMS_INSTALL", &s.install_dir)
                .env("TMS_BUSYBOX", busybox)
                .env("TMS_CONFIG", config)
                .env("TMS_DATA", &s.data_dir);
            if !s.workshop_dir.as_os_str().is_empty() {
                cmd.env("TMS_WORKSHOP", workshop_root(&s.workshop_dir)?);
            }
            cmd
        } else {
            let mut cmd = Command::new(script);
            cmd.args(args);
            cmd
        };
        cmd.current_dir(&s.install_dir).creation_flags(0x08000000);
        Ok(cmd)
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("/bin/bash");
        if intact {
            cmd.arg(s.install_dir.join("LaunchUtils/ScriptCaller.sh"))
                .arg("-server");
        } else {
            cmd.arg(script);
        }
        cmd.current_dir(&s.install_dir).args(args);
        Ok(cmd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_wrapper_handles_normal_spaces_but_needs_intact_argv_for_onedrive() {
        assert!(!needs_intact_path(Path::new("My Games/Terraria")));
        assert!(needs_intact_path(Path::new("OneDrive - Studio/My Games")));
        assert!(needs_intact_path(Path::new("games/[archive]/workshop")));
        assert!(needs_intact_path(Path::new("games/two  spaces")));
    }

    #[test]
    fn uses_checked_save_and_workshop_paths_and_joins_by_ip() {
        let args = launch_args(
            Path::new("My Games/tModLoader"),
            Path::new("Steam Library/workshop/content/1281930"),
            8123,
            Target::Client,
            Some("100.99.8.7".parse().unwrap()),
        )
        .unwrap();
        let expected: Vec<OsString> = [
            "-tmlsavedirectory",
            "My Games/tModLoader",
            "-steamworkshopfolder",
            "Steam Library/workshop",
            "-join",
            "100.99.8.7",
            "-port",
            "8123",
        ]
        .iter()
        .map(OsString::from)
        .collect();
        assert_eq!(args, expected);
    }

    #[test]
    fn standalone_server_disables_steam_and_respects_nondefault_port() {
        let args =
            launch_args(Path::new("Game"), Path::new(""), 8123, Target::Server, None).unwrap();
        assert!(args.contains(&"-nosteam".into()));
        assert!(!args.contains(&"-join".into()));
        assert_eq!(args.last().unwrap(), "8123");
        assert!(launch_args(
            Path::new("Game"),
            Path::new("Mods"),
            7777,
            Target::Client,
            None
        )
        .is_err());
        assert!(launch_args(Path::new("Game"), Path::new(""), 0, Target::Client, None).is_err());
    }
}
