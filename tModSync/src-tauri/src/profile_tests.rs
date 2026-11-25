use super::*;
use crate::paths::write_json;
use crate::sync;
use serde_json::json;

fn fixture() -> (tempfile::TempDir, Settings) {
    let root = tempfile::tempdir().unwrap();
    let s = Settings {
        data_dir: root.path().join("game"),
        workshop_dir: root.path().join("1281930"),
        install_dir: PathBuf::new(),
        branch: String::new(),
        provider: crate::network::Provider::Tailscale,
        network_id: String::new(),
        port: 7777,
    };
    fs::create_dir_all(s.data_dir.join("Mods")).unwrap();
    fs::create_dir(&s.workshop_dir).unwrap();
    (root, s)
}

fn put_string(target: &mut Vec<u8>, value: &str) {
    let mut len = value.len();
    while len >= 128 {
        target.push((len as u8) | 0x80);
        len >>= 7;
    }
    target.push(len as u8);
    target.extend(value.as_bytes());
}

fn package(path: &Path, build: &str, name: &str, version: &str, marker: u8) {
    let mut payload = Vec::new();
    put_string(&mut payload, name);
    put_string(&mut payload, version);
    payload.extend_from_slice(&1i32.to_le_bytes());
    put_string(&mut payload, "fixture.txt");
    payload.extend_from_slice(&1i32.to_le_bytes());
    payload.extend_from_slice(&1i32.to_le_bytes());
    payload.push(marker);
    let mut file = b"TMOD".to_vec();
    put_string(&mut file, build);
    file.extend([marker; 20]);
    file.extend([0; 256]);
    file.extend((payload.len() as i32).to_le_bytes());
    file.extend(payload);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, file).unwrap();
}

#[test]
fn header_branch_and_full_package_digest() {
    let (_tmp, s) = fixture();
    let id = "9482761372";
    let past = s.workshop_dir.join(id).join("2024.6/alias.tmod");
    let future = s.workshop_dir.join(id).join("2025.4/alias.tmod");
    let name = "A".repeat(135);
    package(&past, "2024.6.1.0", &name, "1.2.3", 1);
    package(&future, "2025.4.2.0", &name, "2.0", 2);
    write_json(&s.data_dir.join("Mods/enabled.json"), &[&name]).unwrap();
    assert!(capture(&s, "unknown branch".into()).is_err());
    write_json(
        &s.data_dir.join("config.json"),
        &json!({"LastLaunchedTModLoaderVersion":"2024.12.3.0"}),
    )
    .unwrap();
    let room = capture(&s, "雪原".into()).unwrap();
    assert_eq!(room.mods[0].version, "1.2.3");
    assert_eq!(
        room.mods[0].sha256,
        hex::encode(Sha256::digest(fs::read(&past).unwrap()))
    );
    assert_eq!(room.mods[0].workshop_id.as_deref(), Some(id));
    assert!(compare(&s, &room, false).unwrap().aligned());
    // Same displayed version, different signed file contents.
    package(&past, "2024.6.1.0", &name, "1.2.3", 3);
    assert_eq!(compare(&s, &room, false).unwrap().mismatched, [name]);
}

#[test]
fn room_round_trip_reports_and_repairs_each_difference() {
    let (_host_tmp, host) = fixture();
    let (_guest_tmp, guest) = fixture();
    let mods = host.data_dir.join("Mods");
    package(
        &mods.join("renamed.tmod"),
        "2024.12",
        "Adventures",
        "1.4.2",
        10,
    );
    package(&mods.join("other.tmod"), "2024.12", "Map", "2.1", 20);
    write_json(&mods.join("enabled.json"), &["Adventures", "Map"]).unwrap();
    let host_cfg = host.data_dir.join("ModConfigs");
    fs::create_dir(&host_cfg).unwrap();
    let settings = json!({"seed":9007199254740993u64});
    write_json(&host_cfg.join("Map_Server.json"), &settings).unwrap();
    let exported = capture(&host, "篝火".into()).unwrap();
    let room: RoomProfile =
        serde_json::from_slice(&serde_json::to_vec(&exported).unwrap()).unwrap();

    let guest_mods = guest.data_dir.join("Mods");
    write_json(&guest_mods.join("enabled.json"), &["Extra"]).unwrap();
    let guest_cfg = guest.data_dir.join("ModConfigs");
    fs::create_dir(&guest_cfg).unwrap();
    fs::write(guest_cfg.join("notes.txt"), "keep").unwrap();
    fs::write(
        guest_cfg.join("Old.json"),
        b"\xef\xbb\xbf { \"foo\" : 2 }\n",
    )
    .unwrap();
    fs::copy(mods.join("renamed.tmod"), guest_mods.join("any.tmod")).unwrap();
    let diff = compare(&guest, &room, false).unwrap();
    assert_eq!(diff.missing, ["Map"]);
    assert_eq!(diff.disabled, ["Adventures"]);
    assert_eq!(diff.extra, ["Extra"]);
    assert_eq!(diff.configs, ["Map_Server.json", "Old.json"]);
    let backup = sync::apply(&guest, &room).unwrap().unwrap();
    assert_eq!(
        fs::read(backup.join("ModConfigs/Old.json")).unwrap(),
        b"\xef\xbb\xbf { \"foo\" : 2 }\n"
    );
    assert!(!guest_cfg.join("Old.json").exists());
    assert_eq!(
        fs::read_to_string(guest_cfg.join("notes.txt")).unwrap(),
        "keep"
    );
    assert_eq!(
        read_json::<Value>(&guest_cfg.join("Map_Server.json")).unwrap(),
        settings
    );
    assert!(sync::apply(&guest, &room).unwrap().is_none());
    assert!(!compare(&guest, &room, false).unwrap().aligned());
    fs::copy(mods.join("other.tmod"), guest_mods.join("Map.tmod")).unwrap();
    assert!(compare(&guest, &room, false).unwrap().aligned());

    // Simulate process exit after replacing live files, before journal removal.
    write_json(
        &guest.data_dir.join(".tmodsync-pending.json"),
        backup.file_name().unwrap().to_str().unwrap(),
    )
    .unwrap();
    sync::recover(&guest).unwrap();
    assert_eq!(
        enabled(&guest.data_dir).unwrap(),
        BTreeSet::from(["Extra".into()])
    );
    assert!(guest_cfg.join("Old.json").exists());
    assert!(!guest_cfg.join("Map_Server.json").exists());
    assert!(backup.is_dir());
}

#[test]
fn malformed_imports_are_rejected_before_disk_writes() {
    let (_tmp, s) = fixture();
    let mut room = capture(&s, "空房间".into()).unwrap();
    for name in [
        "../a.json",
        "C:\\a.json",
        "con.json",
        "NUL .json",
        "a.json:extra",
        "a.\u{0000}json",
    ] {
        room.configs = BTreeMap::from([(name.into(), json!(2))]);
        assert!(sync::apply(&s, &room).is_err(), "{name}");
    }
    room.configs = BTreeMap::from([("M.json".into(), json!(1)), ("m.json".into(), json!(2))]);
    assert!(room.validate().is_err());
    assert!(!s.data_dir.join("Mods/enabled.json").exists());
    assert!(!s.data_dir.join(".tmodsync-backups").exists());
}

#[test]
fn malformed_package_or_enabled_file_cannot_greenlight_capture() {
    let (_tmp, s) = fixture();
    let mods = s.data_dir.join("Mods");
    fs::write(mods.join("enabled.json"), "not json").unwrap();
    assert!(capture(&s, "room".into()).is_err());
    write_json(&mods.join("enabled.json"), &["Broken"]).unwrap();
    fs::write(mods.join("Broken.tmod"), b"TMOD\xff\xff\xff").unwrap();
    assert!(capture(&s, "room".into()).is_err());
}

#[test]
fn sync_mods_restores_exact_workshop_files_and_commits_profile_order() {
    let (_tmp, s) = fixture();
    let mods = s.data_dir.join("Mods");
    let ws = s.workshop_dir.join("1234");
    let source = ws.join("2025.2/renamed.tmod");
    package(&source, "2025.2", "World", "2.0", 11);
    package(&mods.join("Kit.tmod"), "2025.2", "Kit", "1.0", 2);
    write_json(&mods.join("enabled.json"), &["World", "Kit"]).unwrap();
    let mut room = capture(&s, "room".into()).unwrap();
    room.mods.sort_by(|a, b| b.name.cmp(&a.name));
    package(&ws.join("2024.1/older.tmod"), "2024.1", "World", "2.0", 44);
    assert_eq!(compare(&s, &room, true).unwrap().missing, ["World"]);
    package(&mods.join("old-name.tmod"), "2025.2", "World", "2.0", 19);
    let old = fs::read(mods.join("old-name.tmod")).unwrap();
    write_json(&mods.join("enabled.json"), &["Extra", "Kit"]).unwrap();
    let backup = sync::apply(&s, &room).unwrap().unwrap();
    assert_eq!(
        fs::read(mods.join("old-name.tmod")).unwrap(),
        fs::read(&source).unwrap()
    );
    assert_eq!(fs::read(backup.join("Mods/old-name.tmod")).unwrap(), old);
    assert_eq!(
        read_json::<Vec<String>>(&mods.join("enabled.json")).unwrap(),
        ["World", "Kit"]
    );
    assert!(sync::apply(&s, &room).unwrap().is_none());
    write_json(
        &s.data_dir.join(".tmodsync-pending.json"),
        backup.file_name().unwrap().to_str().unwrap(),
    )
    .unwrap();
    sync::recover(&s).unwrap();
    assert_eq!(fs::read(mods.join("old-name.tmod")).unwrap(), old);
    assert_eq!(
        read_json::<Vec<String>>(&mods.join("enabled.json")).unwrap(),
        ["Extra", "Kit"]
    );
}

#[test]
fn missing_mod_copy_preserves_unrelated_file_and_rejects_wrong_hash() {
    let (_tmp, s) = fixture();
    let mods = s.data_dir.join("Mods");
    let source = s.workshop_dir.join("3456/2025.2/archive.tmod");
    package(&source, "2025.2", "A", "1.0", 11);
    write_json(&mods.join("enabled.json"), &["A"]).unwrap();
    let room = capture(&s, "room".into()).unwrap();
    package(&mods.join("A.tmod"), "2025.2", "Unrelated", "1.0", 7);
    package(&source, "2025.2", "A", "1.0", 3);
    assert!(sync::apply(&s, &room).unwrap().is_none());
    assert_eq!(compare(&s, &room, true).unwrap().missing, ["A"]);
    package(&source, "2025.2", "A", "1.0", 11);
    assert!(sync::apply(&s, &room).unwrap().is_some());
    assert_eq!(
        Package::read(&mods.join("A.tmod"), None).unwrap().name,
        "Unrelated"
    );
    assert!(compare(&s, &room, true).unwrap().aligned());
    fs::remove_file(&source).unwrap();
    assert!(compare(&s, &room, true).unwrap().aligned());
    assert!(sync::apply(&s, &room).unwrap().is_none());
}
