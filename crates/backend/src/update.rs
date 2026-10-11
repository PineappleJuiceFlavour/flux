use std::{ffi::{OsStr, OsString}, io::Cursor, path::{Path, PathBuf}, sync::Arc};

use base64::Engine;
use bridge::{handle::FrontendHandle, message::MessageToFrontend, modal_action::ModalAction};
use reqwest::StatusCode;
use schema::pandora_update::{UpdateInstallType, UpdateManifest, UpdatePrompt};
use sha1::{Digest, Sha1};
use rand::RngCore;

use crate::directories::LauncherDirectories;

/// Flux's own release channel. A release build gets these from `release.py` (see the Publishing
/// folder): FLUX_VERSION, FLUX_UPDATE_URL (the folder holding `update_manifest_<os>.json`, e.g.
/// `https://github.com/<you>/flux/releases/latest/download`) and FLUX_UPDATE_PUBKEY (the minisign
/// public key every update must be signed with). Builds without them never look for updates.
pub async fn check_for_updates(http_client: reqwest::Client, send: FrontendHandle) {
    let (Some(_), Some(version), Some(update_url)) = (option_env!("FLUX_UPDATE_PUBKEY"), option_env!("FLUX_VERSION"), option_env!("FLUX_UPDATE_URL")) else {
        log::info!("Not a Flux release build: skipping the update check");
        return;
    };

    let current_version = schema::forge::VersionFragment::string_to_parts(version);

    let url = format!("{}/update_manifest_{}.json", update_url.trim_end_matches('/'), std::env::consts::OS);
    // Offline, or no release published yet: nothing to tell the player.
    let response = match http_client.get(url).send().await {
        Ok(response) => response,
        Err(err) => {
            log::warn!("Couldn't check for Flux updates: {}", err);
            return;
        },
    };

    if response.status() != StatusCode::OK {
        log::warn!("Couldn't check for Flux updates: status {}", response.status());
        return;
    }

    let manifest_bytes = match response.bytes().await {
        Ok(manifest_bytes) => manifest_bytes,
        Err(err) => {
            log::warn!("Couldn't download the Flux update manifest: {}", err);
            return;
        },
    };

    let manifest = match serde_json::from_slice::<UpdateManifest>(&manifest_bytes) {
        Ok(manifest) => manifest,
        Err(err) => {
            log::error!("Error while parsing update manifest: {}", err);
            send.send_error("Unable to parse update manifest, see logs for more details");
            return;
        },
    };

    let update_version = schema::forge::VersionFragment::string_to_parts(&manifest.version);

    if current_version >= update_version {
        log::info!("Flux is up to date ({version})");
        return;
    }

    let exes = if let Some(universal) = manifest.downloads.archs.get("universal") {
        universal
    } else if let Some(exes) = manifest.downloads.archs.get(std::env::consts::ARCH) {
        exes
    } else {
        log::warn!("Unable to update, can't find arch \"{}\" in {:?}", std::env::consts::ARCH, manifest.downloads.archs.keys());
        return;
    };

    let Some(install_type) = determine_update_install_type() else {
        log::warn!("Unable to update, can't determine installation type");
        return;
    };

    let install_type_key = install_type.key();
    let Some(executable) = exes.exes.get(install_type_key) else {
        log::warn!("Unable to update, installation type \"{}\" not in {:?}", install_type_key, exes.exes.keys());
        return;
    };

    send.send(MessageToFrontend::UpdateAvailable {
        update: UpdatePrompt {
            old_version: version.into(),
            new_version: manifest.version.clone(),
            install_type,
            exe: executable.clone(),
        }
    });
}

fn determine_update_install_type() -> Option<UpdateInstallType> {
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return Some(UpdateInstallType::AppImage(appimage.into()));
    }

    let current_exe = std::env::current_exe().ok()?;

    if cfg!(target_os = "macos") && let Some(app) = determine_macos_app_path(&current_exe) {
        return Some(UpdateInstallType::App(app.to_path_buf()));
    }

    return Some(UpdateInstallType::Executable);
}

fn determine_macos_app_path(current_exe: &Path) -> Option<&Path> {
    let parent = current_exe.parent()?;

    if parent.file_name()? != OsStr::new("MacOS") {
        return None;
    }

    let parent2 = parent.parent()?;

    if parent2.file_name()? != OsStr::new("Contents") {
        return None;
    }

    let parent3 = parent2.parent()?;

    if parent3.extension()? != OsStr::new("app") {
        return None;
    }

    Some(parent3)
}

pub async fn install_update(http_client: reqwest::Client, dirs: Arc<LauncherDirectories>, send: FrontendHandle, update: UpdatePrompt, modal_action: ModalAction) {
    if let Err(error) = install_update_inner(http_client, &dirs, send.clone(), update, modal_action.clone()).await {
        modal_action.set_finished_with_error(error);
    }

    modal_action.set_finished();
    send.send(MessageToFrontend::Refresh);
}

async fn install_update_inner(http_client: reqwest::Client, dirs: &LauncherDirectories, send: FrontendHandle, update: UpdatePrompt, modal_action: ModalAction) -> Result<(), Arc<str>> {
    let title = format!("Downloading Flux {}", update.new_version);
    let tracker = modal_action.push_tracker(title.into());

    let mut expected_hash = [0u8; 20];
    let Ok(_) = hex::decode_to_slice(&*update.exe.sha1, &mut expected_hash) else {
        return Err("Unable to decode sha1 hash".into());
    };

    let Ok(response) = http_client.get(&*update.exe.download).send().await else {
        return Err("Error making download request".into());
    };

    if response.status() != StatusCode::OK {
        return Err("Download URL returned non-200 status code".into());
    }

    tracker.set_total(update.exe.size);

    use futures::StreamExt;
    let mut stream = response.bytes_stream();

    let mut bytes = Vec::new();

    while let Some(item) = stream.next().await {
        let Ok(item) = item else {
            return Err("Error while downloading update".into());
        };

        bytes.extend_from_slice(&*item);
        tracker.add_count(item.len());
    }

    let mut hasher = Sha1::new();
    hasher.update(&bytes);
    let actual_hash = hasher.finalize();

    if expected_hash != *actual_hash {
        return Err("Hash of downloaded file does not match".into());
    }

    let Some(pubkey) = option_env!("FLUX_UPDATE_PUBKEY") else {
        return Err("Unable to update, missing FLUX_UPDATE_PUBKEY at compile time".into());
    };

    let pubkey = base64::engine::general_purpose::STANDARD.decode(pubkey).unwrap();
    let sig = base64::engine::general_purpose::STANDARD.decode(&*update.exe.sig).unwrap();

    let pk = minisign_verify::PublicKey::decode(std::str::from_utf8(&pubkey).unwrap()).unwrap();
    let signature = minisign_verify::Signature::decode(std::str::from_utf8(&sig).unwrap()).unwrap();

    match pk.verify(&bytes, &signature, false) {
        Err(minisign_verify::Error::InvalidSignature) => {
            return Err("Invalid signature, file was not properly signed".into());
        },
        Err(err) => {
            return Err(format!("Error while validating signature: {:?}", err).into());
        },
        Ok(_) => {}
    }

    // The manifest isn't signed, so its version is checked against the signed one: an old signed
    // build can't be passed off as a newer version.
    let current = option_env!("FLUX_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
    check_signed_version(signature.trusted_comment(), &update.new_version, current)?;

    match update.install_type {
        UpdateInstallType::AppImage(appimage) => {
            let Some(filename) = appimage.file_name() else {
                return Err("Appimage path has no filename".into());
            };

            // This is temporary to address pre-3.3.0 including the version inside the filename
            // This should be removed at some point in the near future
            let new_filename = replace_os_str(filename, &format!("-{}", &update.old_version), "");
            let new_appimage = appimage.with_file_name(new_filename);

            replace_exe(appimage, new_appimage, &bytes, dirs)?;
        },
        UpdateInstallType::Executable => {
            let Ok(current_exe) = std::env::current_exe() else {
                return Err("Unable to determine current exe path".into());
            };

            let Some(filename) = current_exe.file_name() else {
                return Err("Current exe path has no filename".into());
            };

            // This is temporary to address pre-3.3.0 including the version inside the filename
            // This should be removed at some point in the near future
            let new_filename = replace_os_str(filename, &format!("-{}", &update.old_version), "");
            let new_exe = current_exe.with_file_name(new_filename);

            replace_exe(current_exe, new_exe, &bytes, dirs)?;
        },
        UpdateInstallType::App(current_app_folder) => {
            let mut temp_extract = dirs.temp_dir.join(format!("app_unpack_{}", rand::thread_rng().next_u64()));
            while temp_extract.exists() {
                log::warn!("Randomly generated app_unpack folder exists... what are the chances? ({:?})", temp_extract);
                temp_extract = dirs.temp_dir.join(format!("app_unpack_{}", rand::thread_rng().next_u64()));
            }

            let mut temp_backup = dirs.temp_dir.join(format!("app_backup_{}", rand::thread_rng().next_u64()));
            while temp_backup.exists() {
                log::warn!("Randomly generated app_backup folder exists... what are the chances? ({:?})", temp_backup);
                temp_backup = dirs.temp_dir.join(format!("app_backup_{}", rand::thread_rng().next_u64()));
            }

            let result = install_app_update(current_app_folder, &bytes, &temp_extract, &temp_backup);

            _ = std::fs::remove_dir_all(temp_backup);
            _ = std::fs::remove_dir_all(temp_extract);

            if let Err(err) = result {
                return Err(err);
            }
        },
    }

    send.send_success("Flux updated. Restart Flux to use the new version");

    Ok(())
}

fn add_new_extension(path: &Path) -> PathBuf {
    let mut new_exe_data = path.with_added_extension(format!("{}.new", rand::thread_rng().next_u64()));
    while new_exe_data.exists() {
        log::warn!("Randomly generated new_exe_data file exists... what are the chances? ({:?})", new_exe_data);
        new_exe_data = path.with_added_extension(format!("{}.new", rand::thread_rng().next_u64()));
    }
    return new_exe_data;
}

fn write_new_exe_temp(new_exe: &Path, data: &[u8], dirs: &LauncherDirectories) -> Result<PathBuf, String> {
    let new_exe_data = add_new_extension(new_exe);

    let Err(err) = std::fs::write(&new_exe_data, data) else {
        return Ok(new_exe_data);
    };

    if err.kind() != std::io::ErrorKind::PermissionDenied {
        log::error!("Error while writing new executable: {}", err);
        return Err("Error while writing new executable, see logs for more details".into());
    }

    let new_exe_data = add_new_extension(&dirs.temp_dir.join("new_exe_data"));

    if let Err(err) = std::fs::write(&new_exe_data, data) {
        log::error!("Error while writing new executable: {}", err);
        return Err("Error while writing new executable, see logs for more details".into());
    }

    Ok(new_exe_data)
}

fn replace_exe(old_exe: PathBuf, new_exe: PathBuf, data: &[u8], dirs: &LauncherDirectories) -> Result<(), String> {
    let new_exe_temp = write_new_exe_temp(&new_exe, data, dirs)?;

    #[cfg(unix)]
    {
        let result = move_new_exe_into(old_exe, new_exe, &new_exe_temp);
        _ = std::fs::remove_file(new_exe_temp);
        return result;
    }
    #[cfg(windows)]
    {
        let result = swap_running_exe(&old_exe, &new_exe, &new_exe_temp);
        _ = std::fs::remove_file(new_exe_temp);
        return result;
    }
}

/// Windows can't overwrite a running program's file, but it can rename it: park the running
/// Flux.exe as Flux.exe.old, put the new one in its place (used from the next start), and
/// `remove_old_exe` deletes the parked copy once it isn't running any more.
#[cfg(windows)]
fn swap_running_exe(old_exe: &Path, new_exe: &Path, new_exe_data: &Path) -> Result<(), String> {
    let parked = old_exe.with_extension("exe.old");
    _ = std::fs::remove_file(&parked);
    std::fs::rename(old_exe, &parked).map_err(|e| format!("Couldn't move the running Flux aside: {e}"))?;
    // The download may be in the temp folder on another drive, where a rename can't go.
    let placed = std::fs::rename(new_exe_data, new_exe).or_else(|_| std::fs::copy(new_exe_data, new_exe).map(|_| ()));
    if let Err(e) = placed {
        _ = std::fs::rename(&parked, old_exe);
        return Err(format!("Couldn't put the new Flux in place: {e}"));
    }
    Ok(())
}

/// Delete the copy an update parked (see `swap_running_exe`); called at start-up.
pub fn remove_old_exe() {
    #[cfg(windows)]
    if let Ok(exe) = std::env::current_exe() {
        _ = std::fs::remove_file(exe.with_extension("exe.old"));
    }
}

fn try_canonicalize(path: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().ok()?;

    if cfg!(windows) {
        let path_bytes = path.as_os_str().as_encoded_bytes();
        let canonical_bytes = canonical.as_os_str().as_encoded_bytes();
        if canonical_bytes.len() == path_bytes.len()+4
            && &canonical_bytes[..4] == b"\\\\?\\"
            && &canonical_bytes[4..] == path_bytes
        {
            None
        } else {
            Some(canonical)
        }
    } else {
        Some(canonical)
    }
}

#[cfg(unix)]
fn move_new_exe_into(old_exe_path: PathBuf, new_exe_path: PathBuf, new_exe_data: &Path) -> Result<(), String> {
    let old_exe_path = try_canonicalize(&old_exe_path).unwrap_or(old_exe_path);
    let new_exe_path = try_canonicalize(&new_exe_path).unwrap_or(new_exe_path);

    if let Err(err) = std::fs::rename(&new_exe_data, &new_exe_path) {
        if err.kind() == std::io::ErrorKind::PermissionDenied {
            log::info!("Permission denied while trying to update executable, need to elevate");

            let mut command = OsString::new();
            command.push("mv -f '");
            command.push(new_exe_data.as_os_str());
            command.push("' '");
            command.push(new_exe_path.as_os_str());
            command.push("' && chmod +x '");
            command.push(new_exe_path.as_os_str());

            if old_exe_path == new_exe_path {
                command.push("'");
            } else {
                command.push("' && rm -f '");
                command.push(old_exe_path.as_os_str());
                command.push("'");
            }

            // todo: replace runas with workspace command crate
            let result = if cfg!(target_os = "linux") {
                log::info!("Running with pkexec: {}", command.to_string_lossy());
                std::process::Command::new("pkexec").arg("sh").arg("-c").arg(command).status()
            } else {
                log::info!("Running with runas: {}", command.to_string_lossy());
                runas::Command::new("sh").arg("-c").arg(command).gui(true).status()
            };

            match result {
                Ok(status) if status.success() => {
                    return Ok(())
                },
                Ok(status) => {
                    log::error!("Error completing elevated executable install: {}", status);
                    return Err("Error completing elevated executable installation, see logs for more details".into());
                },
                Err(err) => {
                    log::error!("Error completing elevated executable install: {}", err);
                    return Err("Error completing elevated executable installation, see logs for more details".into());
                },
            }
        }

        return Err(format!("Error while updating executable file: {:?}", err).into());
    }

    if old_exe_path != new_exe_path {
        _ = std::fs::remove_file(&old_exe_path);
    }

    use std::os::unix::fs::PermissionsExt;
    _ = std::fs::set_permissions(&new_exe_path, std::fs::Permissions::from_mode(0o755));

    Ok(())
}

fn install_app_update(current_app_folder: PathBuf, bytes: &[u8], temp_extract: &Path, temp_backup: &Path) -> Result<(), Arc<str>> {
    let gz_decoder = flate2::bufread::GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(gz_decoder);

    if let Err(err) = archive.unpack(&temp_extract) {
        log::error!("Unable to unpack .app.tar.gz: {}", err);
        return Err("Error while unpacking .app.tar.gz archive, see logs for more details".into());
    }

    let app_dir = match find_child_with_extension(&temp_extract, OsStr::new("app")) {
        Ok(None) => {
            return Err("Unable to find .app folder in extracted archive".into());
        },
        Err(err) => {
            log::error!("Unable to find .app folder: {}", err);
            return Err("I/O error while finding .app folder, see logs for more details".into());
        },
        Ok(Some(app_dir)) => app_dir,
    };

    // Backup current .app folder
    let needs_authorization = match std::fs::rename(&current_app_folder, &temp_backup) {
        Ok(_) => false,
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => true,
        Err(err) => {
            log::error!("Unable to backup current .app: {}", err);
            return Err("I/O error while backing up current .app, see logs for more details".into());
        }
    };

    if needs_authorization && temp_backup.exists() {
        _ = std::fs::rename(&temp_backup, &current_app_folder);
        return Err("Rename from current .app to temp backup errored, but then succeeded".into());
    }

    if needs_authorization {
        // Move current -> backup, then app -> current in single elevated command
        let mut command = OsString::new();
        command.push("mv -f '");
        command.push(current_app_folder.as_os_str());
        command.push("' '");
        command.push(temp_backup.as_os_str());
        command.push("' && mv -f '");
        command.push(app_dir.as_os_str());
        command.push("' '");
        command.push(current_app_folder.as_os_str());
        command.push("'");

        let result = runas::Command::new("sh").arg("-c").arg(command).gui(true).status();

        let success = match result {
            Ok(status) if status.success() => true,
            Ok(status) => {
                log::error!("Error completing elevated .app install: {}", status);
                false
            },
            Err(err) => {
                log::error!("Error completing elevated .app install: {}", err);
                false
            },
        };

        if !success {
            if temp_backup.exists() {
                let mut command = OsString::new();
                command.push("mv -f '");
                command.push(temp_backup.as_os_str());
                command.push("' '");
                command.push(current_app_folder.as_os_str());
                command.push("'");

                _ = runas::Command::new("sh").arg("-c").arg(command).gui(true).status();
            }

            return Err("Error completing elevated .app installation, see logs for more details".into());
        }
    } else {
        if let Err(err) = std::fs::rename(&app_dir, &current_app_folder) {
            _ = std::fs::rename(&temp_backup, &current_app_folder);
            log::error!("Error renaming new .app to old .app: {}", err);
            return Err("Error completing elevated .app installation, see logs for more details".into());
        }
    }

    Ok(())
}

fn replace_os_str(input: &OsStr, from: &str, to: &str) -> OsString {
    let encoded = input.as_encoded_bytes();

    let from_bytes = from.as_bytes();
    let to_bytes = to.as_bytes();

    let mut new_encoded = Vec::new();
    let mut index = 0;
    while index < encoded.len() {
        if encoded[index..].starts_with(from_bytes) {
            new_encoded.extend_from_slice(to_bytes);
            index += from_bytes.len();
        } else {
            new_encoded.push(encoded[index]);
            index += 1;
        }
    }

    // SAFETY: We construct new_encoded from a mixture of encoded and valid utf-8
    unsafe { OsString::from_encoded_bytes_unchecked(new_encoded) }
}

fn find_child_with_extension(folder: &Path, extension: &OsStr) -> std::io::Result<Option<PathBuf>> {
    let read_dir = std::fs::read_dir(folder)?;

    for entry in read_dir {
        let entry = entry?;
        let path = entry.path();
        if path.extension() == Some(extension) {
            return Ok(Some(path));
        }
    }

    Ok(None)
}

/// The version in a signature's trusted comment ("...\tversion:1.1.0\t...") must be the one the
/// manifest offers, and newer than the running Flux.
fn check_signed_version(trusted_comment: &str, offered: &str, current: &str) -> Result<(), Arc<str>> {
    let signed = trusted_comment.split('\t').find_map(|part| part.strip_prefix("version:"))
        .ok_or_else(|| Arc::<str>::from("This update's signature doesn't name its version, so Flux won't install it"))?;
    if signed != offered {
        return Err(format!("This update is signed as version {signed}, not {offered}").into());
    }
    let parts = schema::forge::VersionFragment::string_to_parts;
    if parts(signed) <= parts(current) {
        return Err(format!("Refusing to install {signed}: it isn't newer than {current}").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use base64::Engine;

    #[test]
    fn signed_version_must_match_and_be_newer() {
        let check = super::check_signed_version;
        let comment = "timestamp:1\tfile:Flux.exe\tversion:1.1.0\thashed";
        assert!(check(comment, "1.1.0", "1.0.9").is_ok());
        assert!(check(comment, "1.2.0", "1.0.9").is_err(), "manifest says a different version");
        assert!(check(comment, "1.1.0", "1.1.0").is_err(), "same version");
        assert!(check(comment, "1.1.0", "1.2.0").is_err(), "older version");
        assert!(check("timestamp:1\tfile:Flux.exe\thashed", "1.1.0", "1.0.9").is_err(), "no version");
    }

    /// FLUX_SIG_TEST=<file>|<base64 public key file>|<base64 signature file>: the same check
    /// install_update runs, on a file signed by Flux's release.py.
    #[test]
    fn release_py_signatures_verify() {
        let Ok(spec) = std::env::var("FLUX_SIG_TEST") else { return };
        let parts: Vec<&str> = spec.split('|').collect();
        let data = std::fs::read(parts[0]).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD;
        let pk = String::from_utf8(b64.decode(parts[1].trim()).unwrap()).unwrap();
        let sig = String::from_utf8(b64.decode(parts[2].trim()).unwrap()).unwrap();
        let pk = minisign_verify::PublicKey::decode(&pk).unwrap();
        let signature = minisign_verify::Signature::decode(&sig).unwrap();
        pk.verify(&data, &signature, false).expect("good signature verifies");
        let mut tampered = data.clone();
        tampered[0] ^= 1;
        assert!(pk.verify(&tampered, &signature, false).is_err(), "a changed file must fail");
        println!("signature from release.py verified");
    }
}
