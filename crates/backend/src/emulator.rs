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

/// Sony's own list of the current PS3 system software (the one a real PS3 checks for updates).
const PS3_UPDATE_LIST: &str = "http://fus01.ps3.update.playstation.net/update/ps3/list/us/ps3-updatelist.txt";

/// The full PS3 system software in Sony's update list: (version, download address). Only
/// addresses on PlayStation's update servers are accepted.
pub fn parse_ps3_update_list(list: &str) -> Option<(String, String)> {
    list.lines().find_map(|line| {
        let field = |key: &str| line.split(';').find_map(|f| f.strip_prefix(key)).map(str::to_string);
        let url = field("CDN=")?;
        if !url.ends_with("/PS3UPDAT.PUP") || !url.starts_with("http://dus01.ps3.update.playstation.net/") {
            return None;
        }
        // "4.9300" -> "4.93"
        let version = field("SystemSoftwareVersion=").map(|v| v.trim_end_matches('0').trim_end_matches('.').to_string());
        Some((version.unwrap_or_else(|| "latest".into()), url))
    })
}

impl BackendState {
    /// Download the PS3 system software from PlayStation's servers and hand it to RPCS3's own
    /// installer (`rpcs3 --installfw`), which checks the file and installs it.
    pub async fn install_ps3_firmware(&self, rpcs3_exe: &Path, modal_action: ModalAction) {
        match self.install_ps3_firmware_inner(rpcs3_exe, &modal_action).await {
            Ok(version) => {
                self.send.send_success(format!("PS3 system software {version} downloaded: finish the install in RPCS3"));
                modal_action.set_finished();
            },
            Err(error) => {
                log::error!("PS3 system software install failed: {error}");
                modal_action.set_finished_with_error(error);
            },
        }
    }

    async fn install_ps3_firmware_inner(&self, rpcs3_exe: &Path, modal_action: &ModalAction) -> Result<String, Arc<str>> {
        use std::io::Write;

        if !rpcs3_exe.is_file() {
            return Err("Install RPCS3 first".into());
        }
        let tracker = modal_action.push_tracker(Arc::from("Asking PlayStation for the latest PS3 system software"));
        tracker.set_total(1);
        let client = self.http_client_provider.redirecting();
        let list = client.get(PS3_UPDATE_LIST).send().await
            .and_then(|r| r.error_for_status()).map_err(|_| Arc::<str>::from("Couldn't reach PlayStation's update server"))?
            .text().await.map_err(|_| Arc::<str>::from("Couldn't read PlayStation's update list"))?;
        let (version, url) = parse_ps3_update_list(&list).ok_or_else(|| Arc::<str>::from("PlayStation's update list has no PS3 system software"))?;
        tracker.add_count(1);
        tracker.set_finished(ProgressTrackerFinishType::Normal);

        let folder = self.directories.root_launcher_dir.join("emulators").join("ps3-system-software");
        std::fs::create_dir_all(&folder).map_err(|_| Arc::<str>::from("Couldn't create the download folder"))?;
        let pup = folder.join("PS3UPDAT.PUP");
        let part = folder.join("PS3UPDAT.PUP.part");

        let tracker = modal_action.push_tracker(Arc::from(format!("Downloading PS3 system software {version}").as_str()));
        let mut response = client.get(&url).send().await
            .and_then(|r| r.error_for_status()).map_err(|_| Arc::<str>::from("Couldn't download the PS3 system software"))?;
        let expected = response.content_length();
        tracker.set_total(expected.unwrap_or(0) as usize);
        let mut file = std::io::BufWriter::new(std::fs::File::create(&part).map_err(|_| Arc::<str>::from("Couldn't save the download"))?);
        let mut written = 0u64;
        let mut header = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Arc::<str>::from("The download was interrupted"))? {
            if modal_action.has_requested_cancel() {
                drop(file);
                _ = std::fs::remove_file(&part);
                return Err("Cancelled".into());
            }
            if header.len() < 5 {
                header.extend_from_slice(&chunk[..chunk.len().min(5 - header.len())]);
            }
            file.write_all(&chunk).map_err(|_| Arc::<str>::from("Couldn't save the download (is the disk full?)"))?;
            written += chunk.len() as u64;
            tracker.add_count(chunk.len());
        }
        file.flush().map_err(|_| Arc::<str>::from("Couldn't save the download"))?;
        drop(file);
        // A PUP file starts with "SCEUF"; anything else (an error page, a cut-off file) is thrown away.
        if header != b"SCEUF" || expected.is_some_and(|n| n != written) {
            _ = std::fs::remove_file(&part);
            return Err("The download from PlayStation wasn't a complete PS3 system software file. Try again.".into());
        }
        std::fs::rename(&part, &pup).map_err(|_| Arc::<str>::from("Couldn't save the download"))?;
        tracker.set_finished(ProgressTrackerFinishType::Normal);

        // RPCS3 verifies the file itself and shows its own install progress.
        let mut command = std::process::Command::new(rpcs3_exe);
        command.arg("--installfw").arg(&pup);
        if let Some(dir) = rpcs3_exe.parent() {
            command.current_dir(dir);
        }
        command.spawn().map_err(|e| Arc::<str>::from(format!("Couldn't start RPCS3: {e}").as_str()))?;
        Ok(version)
    }
}

#[cfg(test)]
mod ps3_firmware_tests {
    use super::parse_ps3_update_list;

    #[test]
    fn finds_the_full_system_software() {
        let list = "# US\nDest=84;CompatibleSystemSoftwareVersion=4.9300-;\n\
            Dest=84;IncrementalUpdateVersion=00010b72-00010b72;ImageVersion=00010b94;SystemSoftwareVersion=4.9300;CDN=http://dus01.ps3.update.playstation.net/update/ps3/image/us/2026_0318_x/PS3PATCH.PUP;CDN_Timeout=30;\n\
            Dest=84;ImageVersion=00010b94;SystemSoftwareVersion=4.9300;CDN=http://dus01.ps3.update.playstation.net/update/ps3/image/us/2026_0318_x/PS3UPDAT.PUP;CDN_Timeout=30;\n";
        let (version, url) = parse_ps3_update_list(list).unwrap();
        assert_eq!(version, "4.93");
        assert!(url.ends_with("/2026_0318_x/PS3UPDAT.PUP"));
    }

    #[test]
    fn ignores_other_hosts() {
        assert!(parse_ps3_update_list("Dest=84;SystemSoftwareVersion=4.93;CDN=http://example.com/PS3UPDAT.PUP;").is_none());
    }

    /// FLUX_PS3FW_TEST=1: read Sony's real list and check the first bytes of the real file.
    #[test]
    fn live_update_list() {
        if std::env::var_os("FLUX_PS3FW_TEST").is_none() {
            return;
        }
        let list = std::process::Command::new("curl").args(["-s", "-m", "30", super::PS3_UPDATE_LIST]).output().unwrap();
        let (version, url) = parse_ps3_update_list(&String::from_utf8_lossy(&list.stdout)).unwrap();
        let head = std::process::Command::new("curl").args(["-s", "-m", "30", "-r", "0-4", &url]).output().unwrap();
        println!("{version} {url}");
        assert_eq!(head.stdout, b"SCEUF");
    }
}

/// One game update in Sony's title-update list (`<package version size sha1sum url>`).
#[derive(Debug, Clone, PartialEq)]
pub struct Ps3TitleUpdate {
    pub version: String,
    pub size: u64,
    pub sha1: String,
    pub url: String,
}

/// The updates in a `<serial>-ver.xml` from Sony, oldest first. Only packages on PlayStation's
/// own download servers are kept.
pub fn parse_ps3_title_updates(xml: &str) -> Vec<Ps3TitleUpdate> {
    fn attr(tag: &str, name: &str) -> Option<String> {
        let start = tag.find(&format!(" {name}=\""))? + name.len() + 3;
        let end = tag[start..].find('"')? + start;
        Some(tag[start..end].to_string())
    }
    xml.split("<package").skip(1).filter_map(|rest| {
        let tag = &rest[..rest.find('>')?];
        let url = attr(tag, "url")?;
        let host = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://"))?.split('/').next()?;
        if !host.ends_with(".np.dl.playstation.net") || !url.ends_with(".pkg") {
            return None;
        }
        Some(Ps3TitleUpdate {
            version: attr(tag, "version")?,
            size: attr(tag, "size")?.parse().ok()?,
            sha1: attr(tag, "sha1sum")?.to_lowercase(),
            url,
        })
    }).collect()
}

/// A PS3 serial: four letters and five digits (BLUS30464).
pub fn is_ps3_serial(serial: &str) -> bool {
    serial.len() == 9 && serial[..4].chars().all(|c| c.is_ascii_uppercase()) && serial[4..].chars().all(|c| c.is_ascii_digit())
}

impl BackendState {
    /// Download a PS3 game's latest official update from PlayStation's servers, check it against
    /// Sony's checksum, and hand it to RPCS3's installer (`rpcs3 --installpkg`).
    pub async fn install_ps3_game_update(&self, serial: &str, installed: Option<&str>, rpcs3_exe: &Path, modal_action: ModalAction) {
        match self.install_ps3_game_update_inner(serial, installed, rpcs3_exe, &modal_action).await {
            Ok(text) => {
                self.send.send_success(text);
                modal_action.set_finished();
            },
            Err(error) => {
                log::error!("PS3 game update for {serial} failed: {error}");
                modal_action.set_finished_with_error(error);
            },
        }
    }

    async fn install_ps3_game_update_inner(&self, serial: &str, installed: Option<&str>, rpcs3_exe: &Path, modal_action: &ModalAction) -> Result<String, Arc<str>> {
        use sha1::Digest;
        use std::io::{Read, Write};

        if !is_ps3_serial(serial) {
            return Err("Flux couldn't read this game's serial".into());
        }
        if !rpcs3_exe.is_file() {
            return Err("Install RPCS3 first".into());
        }
        let tracker = modal_action.push_tracker(Arc::from("Asking PlayStation for the latest game update"));
        tracker.set_total(1);
        // Sony serves this list only over HTTPS signed by its own SHA-1 "SCEI DNAS Root 05", which
        // no current TLS library accepts, so the certificate isn't checked for this one address.
        // What it says is still checked: the package must come from PlayStation's servers, and it
        // is only installed if it matches the list's size and SHA-1.
        let sony = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_secs(30))
            .build().map_err(|_| Arc::<str>::from("Couldn't start the download"))?;
        let list_url = format!("https://a0.ww.np.dl.playstation.net/tpl/np/{serial}/{serial}-ver.xml");
        let response = sony.get(&list_url).send().await.map_err(|_| Arc::<str>::from("Couldn't reach PlayStation's update server"))?;
        let xml = if response.status().is_success() { response.text().await.unwrap_or_default() } else { String::new() };
        let Some(update) = parse_ps3_title_updates(&xml).pop() else {
            return Err("PlayStation has no updates for this game".into());
        };
        tracker.add_count(1);
        tracker.set_finished(ProgressTrackerFinishType::Normal);
        if installed.is_some_and(|v| v >= update.version.as_str()) {
            return Ok(format!("{serial} is already up to date ({})", update.version));
        }

        let folder = self.directories.root_launcher_dir.join("emulators").join("ps3-updates");
        std::fs::create_dir_all(&folder).map_err(|_| Arc::<str>::from("Couldn't create the download folder"))?;
        let name = update.url.rsplit('/').next().unwrap_or("update.pkg").to_string();
        let pkg = folder.join(&name);
        let part = folder.join(format!("{name}.part"));

        let tracker = modal_action.push_tracker(Arc::from(format!("Downloading update {}", update.version).as_str()));
        tracker.set_total(update.size as usize);
        let mut response = self.http_client_provider.redirecting().get(&update.url).send().await
            .and_then(|r| r.error_for_status()).map_err(|_| Arc::<str>::from("Couldn't download the update"))?;
        let mut file = std::io::BufWriter::new(std::fs::File::create(&part).map_err(|_| Arc::<str>::from("Couldn't save the download"))?);
        while let Some(chunk) = response.chunk().await.map_err(|_| Arc::<str>::from("The download was interrupted"))? {
            if modal_action.has_requested_cancel() {
                drop(file);
                _ = std::fs::remove_file(&part);
                return Err("Cancelled".into());
            }
            file.write_all(&chunk).map_err(|_| Arc::<str>::from("Couldn't save the download (is the disk full?)"))?;
            tracker.add_count(chunk.len());
        }
        file.flush().map_err(|_| Arc::<str>::from("Couldn't save the download"))?;
        drop(file);
        tracker.set_finished(ProgressTrackerFinishType::Normal);

        // Sony's SHA-1 covers the package without its last 32 bytes (the package's own digest).
        let tracker = modal_action.push_tracker(Arc::from("Checking the update"));
        tracker.set_total(1);
        let check = (|| -> Option<bool> {
            let len = std::fs::metadata(&part).ok()?.len();
            if len != update.size || len < 32 {
                return Some(false);
            }
            let mut reader = std::io::BufReader::new(std::fs::File::open(&part).ok()?).take(len - 32);
            let mut hasher = sha1::Sha1::new();
            let mut buf = vec![0u8; 1 << 16];
            loop {
                let n = reader.read(&mut buf).ok()?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            Some(hex::encode(hasher.finalize()) == update.sha1)
        })();
        if check != Some(true) {
            _ = std::fs::remove_file(&part);
            return Err("The update didn't match PlayStation's checksum, so it wasn't installed. Try again.".into());
        }
        _ = std::fs::remove_file(&pkg);
        std::fs::rename(&part, &pkg).map_err(|_| Arc::<str>::from("Couldn't save the download"))?;
        tracker.add_count(1);
        tracker.set_finished(ProgressTrackerFinishType::Normal);

        let mut command = std::process::Command::new(rpcs3_exe);
        command.arg("--installpkg").arg(&pkg);
        if let Some(dir) = rpcs3_exe.parent() {
            command.current_dir(dir);
        }
        command.spawn().map_err(|e| Arc::<str>::from(format!("Couldn't start RPCS3: {e}").as_str()))?;
        Ok(format!("Update {} downloaded: RPCS3 is installing it", update.version))
    }
}

#[cfg(test)]
mod ps3_update_tests {
    use super::*;

    #[test]
    fn reads_sonys_update_list() {
        let xml = "<?xml version='1.0' encoding='UTF-8'?>\n<titlepatch status=\"alive\" titleid=\"BLUS30464\"><tag name=\"BLUS30464_T4\" popup=\"true\" signoff=\"true\"><package version=\"01.05\" size=\"34437328\" sha1sum=\"1858889bd6a336a096df1cb20bcf7431094d433c\" url=\"http://b0.ww.np.dl.playstation.net/tppkg/np/BLUS30464/BLUS30464_T4/e401a6be34feeab8/UP0006-BLUS30464_00-SKATE030UPDATE05-A0105-V0100-PE.pkg\" ps3_system_ver=\"03.0000\"><paramsfo><TITLE>Skate 3 DLC</TITLE></paramsfo></package></tag></titlepatch>";
        let updates = parse_ps3_title_updates(xml);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].version, "01.05");
        assert_eq!(updates[0].size, 34437328);
        assert_eq!(updates[0].sha1, "1858889bd6a336a096df1cb20bcf7431094d433c");
    }

    #[test]
    fn ignores_packages_elsewhere() {
        let xml = "<package version=\"01.05\" size=\"1\" sha1sum=\"00\" url=\"http://example.com/x.pkg\">";
        assert!(parse_ps3_title_updates(xml).is_empty());
    }

    #[test]
    fn serials() {
        assert!(is_ps3_serial("BLUS30464"));
        assert!(!is_ps3_serial("BLUS3046"));
        assert!(!is_ps3_serial("../../etc"));
    }

    /// FLUX_PS3UPD_TEST=1: Sony's real list for Skate 3, read through the same client settings.
    #[tokio::test]
    async fn live_skate3_list() {
        if std::env::var_os("FLUX_PS3UPD_TEST").is_none() {
            return;
        }
        let sony = reqwest::Client::builder().danger_accept_invalid_certs(true).build().unwrap();
        let xml = sony.get("https://a0.ww.np.dl.playstation.net/tpl/np/BLUS30464/BLUS30464-ver.xml").send().await.unwrap().text().await.unwrap();
        let updates = parse_ps3_title_updates(&xml);
        println!("{updates:?}");
        assert_eq!(updates.last().unwrap().version, "01.05");
    }
}
