//! Flux's managed emulators: each is downloaded from its project's official releases into
//! `<launcher dir>/emulators/<id>` the first time it's needed, and set to portable mode where the
//! emulator supports it, so its settings, BIOS folder and saves stay in that folder.

use std::{io::Read, path::{Path, PathBuf}, sync::Arc};

use bridge::modal_action::{ModalAction, ProgressTrackerFinishType};

use crate::BackendState;

pub struct EmulatorSource {
    pub id: &'static str,
    pub name: &'static str,
    /// GitHub `owner/repo` and which release: `None` = latest stable, `Some(tag)` = that tag.
    pub repo: &'static str,
    pub tag: Option<&'static str>,
    /// The Windows download: every needle must be in the asset name, none of the excluded ones.
    pub asset_has: &'static [&'static str],
    pub asset_not: &'static [&'static str],
    /// Files created next to the exe to keep the emulator portable.
    pub portable_files: &'static [&'static str],
    /// The emulator's program, matched case-insensitively by prefix and `.exe` suffix.
    pub exe_prefix: &'static str,
}

pub const SOURCES: &[EmulatorSource] = &[
    EmulatorSource {
        id: "xenia", name: "Xenia Canary", repo: "xenia-canary/xenia-canary-releases", tag: None,
        asset_has: &["windows", ".zip"], asset_not: &[], portable_files: &["portable.txt"], exe_prefix: "xenia_canary",
    },
    EmulatorSource {
        id: "duckstation", name: "DuckStation", repo: "stenzek/duckstation", tag: Some("latest"),
        asset_has: &["windows-x64-release.zip"], asset_not: &["symbols", "sse2"], portable_files: &["portable.txt"], exe_prefix: "duckstation-qt",
    },
    EmulatorSource {
        id: "pcsx2", name: "PCSX2", repo: "PCSX2/pcsx2", tag: None,
        asset_has: &["windows-x64-Qt.7z"], asset_not: &["symbols"], portable_files: &["portable.ini"], exe_prefix: "pcsx2-qt",
    },
    EmulatorSource {
        id: "rpcs3", name: "RPCS3", repo: "RPCS3/rpcs3-binaries-win", tag: None,
        asset_has: &["_win64", ".7z"], asset_not: &[".sha256"], portable_files: &[], exe_prefix: "rpcs3",
    },
    EmulatorSource {
        id: "shadps4", name: "shadPS4", repo: "shadps4-emu/shadPS4", tag: None,
        asset_has: &["win64", ".zip"], asset_not: &[], portable_files: &[], exe_prefix: "shadps4",
    },
    EmulatorSource {
        id: "ppsspp", name: "PPSSPP", repo: "hrydgard/ppsspp", tag: None,
        asset_has: &["Windows-x64.zip"], asset_not: &[], portable_files: &["installed.txt"], exe_prefix: "ppssppwindows64",
    },
    EmulatorSource {
        id: "xemu", name: "xemu", repo: "xemu-project/xemu", tag: None,
        asset_has: &["windows-x86_64.zip"], asset_not: &["dbg", "pdb"], portable_files: &[], exe_prefix: "xemu",
    },
];

pub fn source(id: &str) -> Option<&'static EmulatorSource> {
    SOURCES.iter().find(|s| s.id == id)
}

pub fn emulator_dir(launcher_dir: &Path, id: &str) -> PathBuf {
    // Xenia predates the generic layout and kept its folder name.
    let folder = if id == "xenia" { "xenia-canary" } else { id };
    launcher_dir.join("emulators").join(folder)
}

/// The emulator's program in `dir` (searched one level down too).
pub fn find_exe(dir: &Path, prefix: &str) -> Option<PathBuf> {
    let matches = |p: &Path| {
        let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        name.starts_with(prefix) && name.ends_with(".exe")
    };
    let entries: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).collect();
    entries.iter().find(|p| p.is_file() && matches(p)).cloned()
        .or_else(|| entries.iter().filter(|p| p.is_dir()).find_map(|d| find_exe(d, prefix)))
}

fn asset_matches(name: &str, source: &EmulatorSource) -> bool {
    let lower = name.to_lowercase();
    source.asset_has.iter().all(|n| lower.contains(&n.to_lowercase())) && !source.asset_not.iter().any(|n| lower.contains(&n.to_lowercase()))
}

impl BackendState {
    pub async fn install_emulator(&self, id: &str, modal_action: ModalAction) {
        let Some(source) = source(id) else {
            modal_action.set_finished_with_error(Arc::from(format!("Flux can't install the emulator '{id}'").as_str()));
            return;
        };
        if let Err(error) = self.install_emulator_inner(source, &modal_action).await {
            log::error!("{} install failed: {error}", source.name);
            modal_action.set_finished_with_error(error);
            return;
        }
        self.send.send_success(format!("{} installed", source.name));
        modal_action.set_finished();
    }

    async fn install_emulator_inner(&self, source: &EmulatorSource, modal_action: &ModalAction) -> Result<(), Arc<str>> {
        let tracker = modal_action.push_tracker(Arc::from(format!("Finding the latest {}", source.name).as_str()));
        tracker.set_total(1);
        let client = self.http_client_provider.redirecting();
        let api = match source.tag {
            Some(tag) => format!("https://api.github.com/repos/{}/releases/tags/{tag}", source.repo),
            None => format!("https://api.github.com/repos/{}/releases/latest", source.repo),
        };
        let release: serde_json::Value = client.get(&api)
            .header("Accept", "application/vnd.github+json")
            .send().await.map_err(|_| Arc::<str>::from("Couldn't reach GitHub"))?
            .json().await.map_err(|_| Arc::<str>::from("Unexpected GitHub response"))?;
        let asset = release["assets"].as_array().into_iter().flatten()
            .find(|a| a["name"].as_str().is_some_and(|n| asset_matches(n, source)))
            .ok_or_else(|| Arc::<str>::from(format!("The latest {} release has no Windows download", source.name).as_str()))?;
        let url = asset["browser_download_url"].as_str().ok_or_else(|| Arc::<str>::from("Unexpected GitHub response"))?;
        let file_name = asset["name"].as_str().unwrap_or("download.zip").to_string();
        let tag = release["tag_name"].as_str().unwrap_or("latest").to_string();
        tracker.add_count(1);
        tracker.set_finished(ProgressTrackerFinishType::Normal);

        let tracker = modal_action.push_tracker(Arc::from(format!("Downloading {} {tag}", source.name).as_str()));
        let mut response = client.get(url).send().await.map_err(|_| Arc::<str>::from(format!("Couldn't download {}", source.name).as_str()))?;
        if !response.status().is_success() {
            return Err(format!("The {} download returned status {}", source.name, response.status()).into());
        }
        tracker.set_total(response.content_length().unwrap_or(0) as usize);
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Arc::<str>::from("The download was interrupted"))? {
            tracker.add_count(chunk.len());
            bytes.extend_from_slice(&chunk);
            if modal_action.has_requested_cancel() {
                return Err("Cancelled".into());
            }
        }
        tracker.set_finished(ProgressTrackerFinishType::Normal);

        let tracker = modal_action.push_tracker(Arc::from("Unpacking"));
        tracker.set_total(1);
        let dest = emulator_dir(&self.directories.root_launcher_dir, source.id);
        let staging = dest.with_extension("partial");
        _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|e| Arc::<str>::from(format!("Couldn't create {}: {e}", staging.display())))?;
        if file_name.to_lowercase().ends_with(".7z") {
            extract_7z(&bytes, &staging, &file_name)?;
        } else {
            extract_zip(bytes, &staging)?;
        }
        let exe = find_exe(&staging, source.exe_prefix)
            .ok_or_else(|| Arc::<str>::from(format!("{} wasn't in the download", source.name).as_str()))?;
        let exe_dir = exe.parent().unwrap().to_path_buf();
        for file in source.portable_files {
            _ = std::fs::write(exe_dir.join(file), "");
        }
        // Keep the player's settings, BIOS and saves when updating: move them across.
        if dest.is_dir() {
            for keep in ["bios", "memcards", "sstates", "inis", "config", "dev_hdd0", "dev_flash", "user", "content", "cache", "settings.ini", "xenia-canary.config.toml", "recent.toml", "xemu.toml", "memstick"] {
                let old = dest.join(keep);
                if old.exists() {
                    let new = exe_dir.join(keep);
                    _ = std::fs::remove_dir_all(&new);
                    _ = std::fs::remove_file(&new);
                    _ = std::fs::rename(&old, &new);
                }
            }
            _ = std::fs::remove_dir_all(&dest);
        }
        std::fs::rename(&exe_dir, &dest).map_err(|e| Arc::<str>::from(format!("Couldn't move {} into place: {e}", source.name).as_str()))?;
        _ = std::fs::remove_dir_all(&staging);
        tracker.add_count(1);
        tracker.set_finished(ProgressTrackerFinishType::Normal);
        log::info!("Installed {} {tag} into {}", source.name, dest.display());
        Ok(())
    }
}

fn extract_zip(bytes: Vec<u8>, into: &Path) -> Result<(), Arc<str>> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| Arc::<str>::from("The download isn't a valid zip"))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|_| Arc::<str>::from("Corrupt zip entry"))?;
        // enclosed_name rejects absolute paths and `..`, so nothing can land outside the folder.
        let Some(relative) = file.enclosed_name() else { continue };
        let out = into.join(relative);
        if file.is_dir() {
            _ = std::fs::create_dir_all(&out);
            continue;
        }
        if let Some(parent) = out.parent() {
            _ = std::fs::create_dir_all(parent);
        }
        let mut data = Vec::new();
        file.read_to_end(&mut data).map_err(|_| Arc::<str>::from("Corrupt zip entry"))?;
        std::fs::write(&out, data).map_err(|e| Arc::<str>::from(format!("Couldn't write {}: {e}", out.display())))?;
    }
    Ok(())
}

/// 7-Zip archives go through Windows' own tar (libarchive, built in since Windows 10 1803).
fn extract_7z(bytes: &[u8], into: &Path, file_name: &str) -> Result<(), Arc<str>> {
    let archive = into.join(file_name);
    std::fs::write(&archive, bytes).map_err(|e| Arc::<str>::from(format!("Couldn't save the download: {e}")))?;
    let tar = std::env::var_os("SystemRoot").map(|r| PathBuf::from(r).join("System32").join("tar.exe")).unwrap_or_else(|| "tar.exe".into());
    let mut command = std::process::Command::new(tar);
    command.arg("-xf").arg(&archive).arg("-C").arg(into);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let status = command.status().map_err(|e| Arc::<str>::from(format!("Couldn't run tar to unpack: {e}")))?;
    _ = std::fs::remove_file(&archive);
    // libarchive refuses absolute and `..` paths by default, so extraction stays inside `into`.
    if !status.success() {
        return Err("Couldn't unpack the download".into());
    }
    Ok(())
}
