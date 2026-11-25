#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod launch;
mod network;
mod paths;
mod profile;
mod server;
mod sync;
mod sync_mods;

use anyhow::{Context, Result};
use paths::Settings;
use profile::RoomProfile;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

type Api<T> = Result<T, String>;
fn message(e: anyhow::Error) -> String {
    format!("{e:#}")
}

// Carry the document as text across the JS bridge so 64-bit config values survive.
fn parse_room(text: &str) -> Result<RoomProfile> {
    anyhow::ensure!((text.len() as u64) <= paths::JSON_LIMIT, "清单超过 32 MiB");
    let room: RoomProfile = serde_json::from_str(text)?;
    room.validate()?;
    Ok(room)
}

fn room_text(room: &RoomProfile) -> Result<String> {
    room.validate()?;
    let text = serde_json::to_string_pretty(room)?;
    anyhow::ensure!((text.len() as u64) < paths::JSON_LIMIT, "清单超过 32 MiB");
    Ok(text)
}

async fn disk<T, F>(app: AppHandle, job: F) -> Api<T>
where
    T: Send + 'static,
    F: FnOnce(PathBuf) -> Result<T> + Send + 'static,
{
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::create_dir_all(&dir)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join("workspace.lock"))?;
        fs2::FileExt::lock_exclusive(&lock)?;
        job(dir)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(message)
}

#[tauri::command]
async fn settings(app: AppHandle) -> Api<Settings> {
    disk(app, |d| paths::load(&d)).await
}
#[tauri::command]
async fn detect_paths() -> Api<Settings> {
    tauri::async_runtime::spawn_blocking(paths::detect)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn path_status(app: AppHandle) -> Api<paths::PathStatus> {
    disk(app, |d| Ok(paths::status(&paths::load(&d)?))).await
}
#[tauri::command]
async fn save_settings(app: AppHandle, value: Settings) -> Api<()> {
    disk(app, move |d| {
        value.validate()?;
        paths::write_json(&d.join("config.json"), &value)
    })
    .await
}
#[tauri::command]
async fn pick_folder() -> Api<Option<PathBuf>> {
    Ok(rfd::AsyncFileDialog::new()
        .pick_folder()
        .await
        .map(|p| p.path().into()))
}
#[tauri::command]
async fn capture(app: AppHandle, room_name: String) -> Api<String> {
    disk(app, move |d| {
        room_text(&profile::capture(&paths::load(&d)?, room_name)?)
    })
    .await
}
#[tauri::command]
async fn import_room() -> Api<Option<String>> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .add_filter("Room Profile", &["json"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    tauri::async_runtime::spawn_blocking(move || -> Result<String> {
        let room: RoomProfile = paths::read_json(file.path())?;
        room_text(&room)
    })
    .await
    .map_err(|e| e.to_string())?
    .map(Some)
    .map_err(message)
}
#[tauri::command]
async fn export_room(room: String) -> Api<bool> {
    let room = parse_room(&room).map_err(message)?;
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_file_name("room-profile.json")
        .add_filter("Room Profile", &["json"])
        .save_file()
        .await
    else {
        return Ok(false);
    };
    paths::write_json(file.path(), &room).map_err(message)?;
    Ok(true)
}
#[tauri::command]
async fn compare(app: AppHandle, room: String, require_local: Option<bool>) -> Api<profile::Diff> {
    disk(app, move |d| {
        profile::compare(
            &paths::load(&d)?,
            &parse_room(&room)?,
            require_local.unwrap_or(false),
        )
    })
    .await
}
#[tauri::command]
async fn apply_room(app: AppHandle, room: String) -> Api<Option<PathBuf>> {
    disk(app, move |d| {
        sync::apply(&paths::load(&d)?, &parse_room(&room)?)
    })
    .await
}
#[derive(serde::Serialize)]
struct SyncResult {
    backup: Option<PathBuf>,
    diff: profile::Diff,
}

#[tauri::command]
async fn sync_mods(app: AppHandle, room: String) -> Api<SyncResult> {
    disk(app, move |d| {
        let s = paths::load(&d)?;
        let room = parse_room(&room)?;
        let backup = sync::apply(&s, &room)?;
        let diff = profile::compare(&s, &room, true)?;
        Ok(SyncResult { backup, diff })
    })
    .await
}
#[tauri::command]
async fn network_overview(
    provider: network::Provider,
    network_id: String,
) -> Api<network::Overview> {
    Ok(network::overview(provider, &network_id).await)
}
#[tauri::command]
async fn download_network(provider: network::Provider) -> Api<()> {
    open::that(provider.download_url()).map_err(|e| e.to_string())
}
#[tauri::command]
async fn network_status(provider: network::Provider, network_id: String) -> Api<network::Status> {
    Ok(network::status(provider, &network_id).await)
}
#[tauri::command]
async fn connect_network(provider: network::Provider, network_id: String) -> Api<String> {
    network::connect(provider, &network_id)
        .await
        .map_err(message)
}
#[tauri::command]
async fn copy_text(
    text: String,
    clipboard: tauri::State<'_, std::sync::Mutex<Option<arboard::Clipboard>>>,
) -> Api<()> {
    let mut slot = clipboard.lock().map_err(|_| "剪贴板正忙".to_owned())?;
    if slot.is_none() {
        *slot = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
    }
    slot.as_mut()
        .unwrap()
        .set_text(text)
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn open_workshop(id: Option<String>, name: Option<String>) -> Api<()> {
    let Some(id) = id else {
        let mut url = tauri::Url::parse("https://steamcommunity.com/workshop/browse/")
            .map_err(|e| e.to_string())?;
        url.query_pairs_mut()
            .append_pair("appid", "1281930")
            .append_pair("searchtext", name.as_deref().unwrap_or(""));
        return open::that(url.as_str()).map_err(|e| e.to_string());
    };
    if !profile::workshop_id(&id) {
        return Err("无效的创意工坊 ID".into());
    }
    open::that(format!(
        "https://steamcommunity.com/sharedfiles/filedetails/?id={id}"
    ))
    .map_err(|e| e.to_string())
}
#[tauri::command]
async fn list_worlds(app: AppHandle) -> Api<Vec<String>> {
    disk(app, |d| server::worlds(&paths::load(&d)?)).await
}
#[tauri::command]
async fn server_status(app: AppHandle) -> Api<server::Status> {
    tauri::async_runtime::spawn_blocking(move || app.state::<server::Manager>().status())
        .await
        .map_err(|e| e.to_string())?
        .map_err(message)
}
#[tauri::command]
fn stop_server(app: AppHandle) -> Api<()> {
    app.state::<server::Manager>().stop().map_err(message)
}

fn preflight(s: &Settings, room: &RoomProfile, guest: bool) -> Result<()> {
    anyhow::ensure!(
        profile::compare(s, room, guest)?.aligned(),
        "启动预检失败 · 模组或配置已有变化，请刷新清单"
    );
    let online = tauri::async_runtime::block_on(network::status(s.provider, &s.network_id));
    anyhow::ensure!(online.online, "启动预检失败 · {}", online.detail);
    Ok(())
}

#[tauri::command]
async fn start_server(app: AppHandle, room: Option<String>, options: server::Options) -> Api<()> {
    disk(app.clone(), move |dir| {
        let s = paths::load(&dir)?;
        let room = match room {
            Some(raw) => parse_room(&raw)?,
            None => profile::capture(&s, "本机房间".into())?,
        };
        preflight(&s, &room, false)?;
        app.state::<server::Manager>().start(&dir, &s, options)
    })
    .await
}

#[tauri::command]
async fn launch_game(
    app: AppHandle,
    room: String,
    target: launch::Target,
    host: Option<std::net::IpAddr>,
) -> Api<()> {
    disk(app, move |d| {
        let s = paths::load(&d)?;
        preflight(&s, &parse_room(&room)?, host.is_some())?;
        launch::start(&s, target, host).context("启动 tModLoader 失败")
    })
    .await
}
fn main() {
    tauri::Builder::default()
        .manage(server::Manager::default())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let manager = window.state::<server::Manager>();
                if manager.status().is_ok_and(|s| s.running) {
                    api.prevent_close();
                    if manager.stop().is_ok() {
                        let window = window.clone();
                        tauri::async_runtime::spawn_blocking(move || {
                            while window
                                .state::<server::Manager>()
                                .status()
                                .is_ok_and(|s| s.running)
                            {
                                std::thread::sleep(std::time::Duration::from_millis(500));
                            }
                            let _ = window.close();
                        });
                    }
                }
            }
        })
        .manage(std::sync::Mutex::new(None::<arboard::Clipboard>))
        .invoke_handler(tauri::generate_handler![
            settings,
            detect_paths,
            path_status,
            save_settings,
            pick_folder,
            capture,
            import_room,
            export_room,
            compare,
            apply_room,
            sync_mods,
            network_status,
            network_overview,
            download_network,
            connect_network,
            copy_text,
            open_workshop,
            list_worlds,
            server_status,
            stop_server,
            start_server,
            launch_game
        ])
        .run(tauri::generate_context!())
        .expect("tModSync desktop runtime");
}

#[cfg(test)]
mod bridge_tests {
    use super::*;

    #[test]
    fn profile_json_preserves_configuration_integers_above_js_safe_range() {
        let text = r#"{"schemaVersion":1,"roomName":"精度测试","mods":[],"configs":{"Demo_Settings.json":{"seed":9007199254740993}}}"#;
        let output = room_text(&parse_room(text).unwrap()).unwrap();
        assert!(output.contains("9007199254740993"));
    }
}
