use crate::paths::Settings;
use crate::profile::{catalog, config_name, entry, Package, RoomMod, RoomProfile};
use anyhow::{ensure, Context, Result};
use std::{collections::BTreeSet, fs::File, io, path::Path};
use tempfile::NamedTempFile;

#[derive(Default)]
pub struct Repairs {
    pub originals: Vec<String>,
    pub installs: Vec<(String, NamedTempFile)>,
}

pub fn filename(name: &str) -> Result<()> {
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .context("无效的模组文件名")?;
    ensure!(
        name.len() <= 255
            && name.to_ascii_lowercase().ends_with(".tmod")
            && !name.contains(['/', '\\', ':']),
        "无效的模组文件名 {name}"
    );
    config_name(&format!("{stem}.json"))
}

fn matches(package: &Package, expected: &RoomMod) -> Result<bool> {
    Ok(package.version == expected.version && package.summary()?.sha256 == expected.sha256)
}

// Stage verified bytes before the existing journal commits binaries, configs and enabled.json.
pub fn prepare(s: &Settings, room: &RoomProfile) -> Result<Repairs> {
    let mods = s.data_dir.join("Mods");
    let names = room.mods.iter().map(|m| m.name.clone()).collect();
    let indexed = catalog(s, &names)?;
    let mut plan = Repairs::default();
    let mut reserved = BTreeSet::new();
    for expected in &room.mods {
        let Some(packages) = indexed.get(&expected.name) else {
            continue;
        };
        let mut has_exact = false;
        let mut stale = Vec::new();
        for package in packages.iter().filter(|p| p.workshop_id.is_none()) {
            if matches(package, expected)? {
                has_exact = true;
            } else {
                let name = package
                    .path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .context("无法备份非 UTF-8 文件名")?;
                filename(name)?;
                stale.push(name.to_owned());
            }
        }
        if !has_exact {
            let mut source = None;
            for package in packages.iter().filter(|p| p.workshop_id.is_some()) {
                if matches(package, expected)? {
                    source = Some(&package.path);
                    break;
                }
            }
            let Some(source) = source else { continue };
            let mut name = format!("{}.tmod", expected.name);
            if let Some(old) = stale.first() {
                name = old.clone();
            } else if filename(&name).is_err()
                || entry(&mods.join(&name))?.is_some()
                || reserved.contains(&name.to_lowercase())
            {
                name = format!("tmodsync-{}.tmod", expected.sha256);
                ensure!(
                    entry(&mods.join(&name))?.is_none(),
                    "目标文件名已被占用：{name}"
                );
            }
            ensure!(
                reserved.insert(name.to_lowercase()),
                "目标文件名冲突：{name}"
            );
            let mut temp = NamedTempFile::new_in(&mods)?;
            io::copy(&mut File::open(source)?, temp.as_file_mut())?;
            temp.as_file().sync_all()?;
            let staged = Package::read(temp.path(), None)?;
            ensure!(
                staged.name == expected.name && matches(&staged, expected)?,
                "复制期间工坊文件已发生变化：{}",
                expected.name
            );
            plan.installs.push((name, temp));
        }
        plan.originals.extend(stale);
    }
    Ok(plan)
}
