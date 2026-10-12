use std::{path::{Path, PathBuf}, sync::Arc};

use bridge::{import::ImportFromOtherLauncherJob, modal_action::ModalAction};
use schema::{curseforge::{CurseforgeModLoaderType}, instance::{InstanceConfiguration, InstanceMemoryConfiguration}, loader::Loader};
use serde::Deserialize;
use ustr::Ustr;

use crate::BackendState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseforgeInstanceBaseModLoader {
    r#type: u32,
    latest: bool,
    recommended: bool,
    #[serde(rename = "forgeVersion")]
    loader_version: Option<Arc<str>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseforgeInstance {
    base_mod_loader: Option<CurseforgeInstanceBaseModLoader>,
    game_version: Ustr,
    #[serde(default)]
    is_memory_override: bool,
    #[serde(default)]
    allocated_memory: u32,
}

fn try_load_from_curseforge(config_path: &Path) -> Option<InstanceConfiguration> {
    let instance_cfg_bytes = match std::fs::read(config_path) {
        Ok(instance_cfg_bytes) => instance_cfg_bytes,
        Err(err) => {
            log::error!("CurseForge import: Unable to read {:?}: {:?}", config_path, err);
            return None;
        },
    };
    let instance_cfg = match serde_json::from_slice::<CurseforgeInstance>(&instance_cfg_bytes) {
        Ok(instance_cfg) => instance_cfg,
        Err(err) => {
            log::error!("Unable to parse CurseforgeInstance {:?}: {:?}", config_path, err);
            return None;
        },
    };

    let (loader, preferred_loader_version) = if let Some(base_mod_loader) = instance_cfg.base_mod_loader {
        let loader = CurseforgeModLoaderType::from_u32(base_mod_loader.r#type);
        if loader == CurseforgeModLoaderType::Any {
            log::warn!("CurseForge import: unknown mod loader type id {}", base_mod_loader.r#type);
            (Loader::Vanilla, None)
        } else if !base_mod_loader.latest && !base_mod_loader.recommended && let Some(loader_version) = base_mod_loader.loader_version {
            let preferred_loader_version = if loader == CurseforgeModLoaderType::Forge {
                format!("{}-{}", instance_cfg.game_version, loader_version).into()
            } else {
                loader_version.into()
            };
            (loader.as_pandora().unwrap_or(Loader::Vanilla), Some(preferred_loader_version))
        } else {
            (loader.as_pandora().unwrap_or(Loader::Vanilla), None)
        }
    } else {
        (Loader::Vanilla, None)
    };
    let mut configuration = InstanceConfiguration::new(instance_cfg.game_version, loader);
    configuration.preferred_loader_version = preferred_loader_version;
    if instance_cfg.is_memory_override {
        configuration.memory = Some(InstanceMemoryConfiguration {
            enabled: true,
            min: 0,
            max: instance_cfg.allocated_memory,
        })
    }

    Some(configuration)
}

pub async fn import_from_curseforge(backend: &BackendState, import_job: ImportFromOtherLauncherJob, modal_action: ModalAction) {
    import_instances_from_curseforge(backend, &import_job, &modal_action).await;
    modal_action.set_finished();
}

#[derive(Debug)]
struct CurseforgeInstanceToImport {
    pandora_path: PathBuf,
    config_path: PathBuf,
    folder: Arc<Path>,
}

pub async fn import_instances_from_curseforge(backend: &BackendState, import_job: &ImportFromOtherLauncherJob, modal_action: &ModalAction) {
    if import_job.paths.is_empty() {
        return;
    }

    let all_tracker = modal_action.push_tracker("Importing instances".into());

    let mut to_import = Vec::new();

    for folder in import_job.paths.iter() {
        if !folder.is_dir() {
            continue;
        }

        let Some(filename) = folder.file_name() else {
            continue;
        };

        let pandora_path = backend.directories.instances_dir.join(filename);
        if pandora_path.exists() {
            continue;
        }

        let curseforge_config = folder.join("minecraftinstance.json");
        if !curseforge_config.exists() {
            continue;
        }

        to_import.push(CurseforgeInstanceToImport {
            pandora_path,
            config_path: curseforge_config,
            folder: folder.clone()
        });
    }

    all_tracker.set_total(to_import.len());

    for to_import in to_import {
        let title = format!("Importing {}", to_import.folder.file_name().unwrap().to_string_lossy());
        let tracker = modal_action.push_tracker(title.into());

        let Some(configuration) = try_load_from_curseforge(&to_import.config_path) else {
            tracker.set_finished(bridge::modal_action::ProgressTrackerFinishType::Error);
            log::error!("Failed to load config path from curseforge for {:?}", to_import.folder.file_name().unwrap());
            continue;
        };

        let Ok(configuration_bytes) = serde_json::to_vec(&configuration) else {
            tracker.set_finished(bridge::modal_action::ProgressTrackerFinishType::Error);
            continue;
        };

        _ = std::fs::create_dir_all(&to_import.pandora_path);
        let target_dot_minecraft = to_import.pandora_path.join(".minecraft");

        _ = std::fs::create_dir_all(&target_dot_minecraft);
        _ = crate::fs::copy_content_recursive(&to_import.folder, &target_dot_minecraft, false, &|copied, total| {
            tracker.set_total(total as usize);
            tracker.set_count(copied as usize);
        });

        // Keep the picture CurseForge shows: the user's own, else the modpack's. If this fails
        // we just fall back on the default icon, it's fine.
        if let Ok(Some(png)) = curseforge_icon_png(backend, &to_import.folder).await {
            _ = crate::fs::write_safe(&to_import.pandora_path.join("icon.png"), &png);
        }
        _ = std::fs::write(to_import.pandora_path.join(ICON_CHECKED), b"");
        _ = std::fs::remove_dir_all(&target_dot_minecraft.join("profileImage"));
        _ = std::fs::remove_file(&target_dot_minecraft.join("minecraftinstance.json"));

        let info_path = to_import.pandora_path.join("info_v1.json");
        _ = crate::fs::write_safe(&info_path, &configuration_bytes);

        all_tracker.add_count(1);

        tracker.set_finished(bridge::modal_action::ProgressTrackerFinishType::Fast);
    }

    all_tracker.set_finished(bridge::modal_action::ProgressTrackerFinishType::Normal);

}

/// Marks an instance whose CurseForge picture has been looked for, so it's done only once.
const ICON_CHECKED: &str = ".flux-curseforge-icon";

/// The picture CurseForge shows for one of its instance folders, as PNG: the user's own picture
/// (profileImage), else the installed modpack's thumbnail. Err means the download failed (try
/// again later); Ok(None) means there's no picture.
async fn curseforge_icon_png(backend: &BackendState, folder: &Path) -> Result<Option<Vec<u8>>, ()> {
    let Ok(bytes) = std::fs::read(folder.join("minecraftinstance.json")) else { return Ok(None) };
    let bytes = bytes.strip_prefix([0xEF, 0xBB, 0xBF].as_slice()).unwrap_or(&bytes);
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(bytes) else { return Ok(None) };

    let own = json.get("profileImagePath").and_then(|p| p.as_str()).map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| folder.join("profileImage").read_dir().ok()?.flatten().map(|e| e.path()).find(|p| p.is_file()));
    if let Some(path) = own && let Some(png) = std::fs::read(path).ok().and_then(|b| to_png(&b)) {
        return Ok(Some(png));
    }

    let Some(url) = json.pointer("/installedModpack/thumbnailUrl").and_then(|u| u.as_str()).filter(|u| u.starts_with("https://")) else {
        return Ok(None);
    };
    let response = backend.http_client_provider.client().get(url).timeout(std::time::Duration::from_secs(8)).send().await
        .and_then(|r| r.error_for_status()).map_err(|err| log::warn!("CurseForge modpack picture {url}: {err}"))?;
    let bytes = response.bytes().await.map_err(|_| ())?;
    Ok(to_png(&bytes))
}

fn to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let format = image::guess_format(bytes).ok()?;
    if format == image::ImageFormat::Png {
        return Some(bytes.to_vec());
    }
    let image = image::load_from_memory_with_format(bytes, format).ok()?;
    let mut png = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some(png)
}

/// Instances imported from CurseForge before Flux kept their pictures: give each one without an
/// icon the picture of the CurseForge instance with the same folder name, once.
pub async fn restore_curseforge_icons(backend: &BackendState) {
    let Some(home) = std::env::var_os("USERPROFILE") else { return };
    let curseforge = Path::new(&home).join("curseforge").join("minecraft").join("Instances");
    let Ok(entries) = std::fs::read_dir(&backend.directories.instances_dir) else { return };
    let mut jobs = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() || dir.join("icon.png").exists() || dir.join(ICON_CHECKED).exists() {
            continue;
        }
        let source = curseforge.join(entry.file_name());
        if !source.join("minecraftinstance.json").is_file() {
            continue;
        }
        jobs.push(async move {
            match curseforge_icon_png(backend, &source).await {
                Ok(png) => {
                    if let Some(png) = png {
                        _ = crate::fs::write_safe(&dir.join("icon.png"), &png);
                    }
                    _ = std::fs::write(dir.join(ICON_CHECKED), b"");
                },
                Err(()) => {}, // offline: try again next time
            }
        });
    }
    futures::future::join_all(jobs).await;
}
