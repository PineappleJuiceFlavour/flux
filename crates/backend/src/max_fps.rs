//! Flux's "Max FPS" preset: a Fabric instance with the best optimization mods that exist for the
//! chosen Minecraft version, tuned JVM flags and an FPS-first options.txt.
//!
//! Mods are resolved per version, so the preset works for any Fabric-supported version: a mod with no
//! build for that version is skipped instead of failing the whole install.

use std::sync::Arc;

use bridge::{
    install::{ContentDownload, ContentInstall, ContentInstallFile, ContentInstallPath, InstallTarget, NewInstancePreset},
    message::MessageToFrontend,
    modal_action::{ModalAction, ProgressTrackerFinishType},
};
use schema::{
    content::{ContentInstallReason, ContentSource},
    instance::{InstanceConfiguration, InstanceJvmFlagsConfiguration, InstanceMemoryConfiguration},
    loader::Loader,
    modrinth::{ModrinthDependencyType, ModrinthLoader, ModrinthProjectVersion, ModrinthProjectVersionsRequest, ModrinthVersionType},
};
use ustr::Ustr;

use crate::{BackendState, metadata::items::ModrinthProjectVersionsMetadataItem};

pub struct StackMod {
    pub project_id: &'static str,
    pub name: &'static str,
    /// Only installed when the user opts in (e.g. experimental renderers).
    pub opt_in: bool,
}

/// The optimization stack, in install order. Project ids are Modrinth's stable ids, not slugs.
pub const STACK: &[StackMod] = &[
    StackMod { project_id: "AANobbMI", name: "Sodium", opt_in: false },
    StackMod { project_id: "gvQqBUqZ", name: "Lithium", opt_in: false },
    StackMod { project_id: "uXXizFIs", name: "FerriteCore", opt_in: false },
    StackMod { project_id: "5ZwdcRci", name: "ImmediatelyFast", opt_in: false },
    StackMod { project_id: "NNAgCjsB", name: "Entity Culling", opt_in: false },
    StackMod { project_id: "51shyZVL", name: "More Culling", opt_in: false },
    StackMod { project_id: "LQ3K71Q1", name: "Dynamic FPS", opt_in: false },
    StackMod { project_id: "nmDcB62a", name: "ModernFix", opt_in: false },
    StackMod { project_id: "SfMw2IZN", name: "Nvidium", opt_in: true },
];

const SODIUM: &str = "AANobbMI";
const NVIDIUM: &str = "SfMw2IZN";

/// G1 flags that are valid on every Java version Minecraft has shipped with (8 through 25), so the
/// preset never stops an old instance from starting.
pub const JVM_FLAGS: &str = "-XX:+UseG1GC -XX:MaxGCPauseMillis=20 -XX:+UnlockExperimentalVMOptions \
-XX:G1NewSizePercent=20 -XX:G1ReservePercent=20 -XX:G1HeapRegionSize=16M -XX:+ParallelRefProcEnabled \
-XX:+DisableExplicitGC -XX:+AlwaysPreTouch";

/// Options that have kept the same key and format for many versions. No `version:` line: the game
/// then treats the file as legacy and runs its option fixers, which leave these keys alone.
pub const OPTIONS_TXT: &str = "enableVsync:false
maxFps:260
renderDistance:12
simulationDistance:8
entityShadows:false
renderClouds:\"false\"
particles:1
biomeBlendRadius:1
inactivityFpsLimit:\"afk\"
";

#[derive(Debug, Clone, Copy)]
pub struct MaxFpsOptions {
    pub nvidium: bool,
}

struct Resolved {
    mod_name: &'static str,
    version: ModrinthProjectVersion,
}

fn version_rank(version: &ModrinthProjectVersion) -> u8 {
    match version.version_type {
        Some(ModrinthVersionType::Release) => 0,
        Some(ModrinthVersionType::Beta) => 1,
        Some(ModrinthVersionType::Alpha) => 2,
        _ => 3,
    }
}

/// Newest release, else newest beta, else newest alpha. Modrinth lists versions newest first.
fn pick_version(versions: &[ModrinthProjectVersion]) -> Option<ModrinthProjectVersion> {
    versions.iter().min_by_key(|v| version_rank(v)).cloned()
}

impl BackendState {
    pub async fn create_max_fps_instance(self: &Arc<Self>, name: Ustr, minecraft_version: Ustr, options: MaxFpsOptions, modal_action: ModalAction) {
        let tracker = modal_action.push_tracker(Arc::from("Finding optimization mods"));
        let wanted: Vec<&StackMod> = STACK.iter().filter(|m| !m.opt_in || (m.project_id == NVIDIUM && options.nvidium)).collect();
        tracker.set_total(wanted.len());

        let lookups = wanted.iter().map(|stack_mod| {
            let this = self.clone();
            let tracker = tracker.clone();
            async move {
                let result = this.meta.fetch(ModrinthProjectVersionsMetadataItem(ModrinthProjectVersionsRequest {
                    project_id: Arc::from(stack_mod.project_id),
                    game_versions: Some(Arc::new([Arc::from(minecraft_version.as_str())])),
                    loaders: Some(Arc::new([ModrinthLoader::Fabric])),
                })).await;
                tracker.add_count(1);
                let version = result.ok().and_then(|versions| pick_version(&versions.0));
                version.map(|version| Resolved { mod_name: stack_mod.name, version })
            }
        });
        let mut resolved: Vec<Resolved> = futures::future::join_all(lookups).await.into_iter().flatten().collect();
        tracker.set_finished(ProgressTrackerFinishType::Normal);

        // Nvidium is built against one exact Sodium build; use that one so the pair loads together.
        if let Some(nvidium) = resolved.iter().find(|r| r.version.project_id.as_ref() == NVIDIUM) {
            let pinned_sodium = nvidium.version.dependencies.iter().flatten().find(|dep| {
                dep.dependency_type == ModrinthDependencyType::Required && dep.project_id.as_deref() == Some(SODIUM)
            }).and_then(|dep| dep.version_id.clone());
            if let Some(pinned_sodium) = pinned_sodium {
                if let Some(sodium) = resolved.iter_mut().find(|r| r.version.project_id.as_ref() == SODIUM) {
                    sodium.version.id = pinned_sodium;
                }
            }
        }

        if !resolved.iter().any(|r| r.version.project_id.as_ref() == SODIUM) {
            modal_action.set_finished_with_error(Arc::from(format!(
                "Sodium has no Fabric build for Minecraft {minecraft_version}, so the Max FPS preset can't be used for this version"
            ).as_str()));
            return;
        }

        let skipped: Vec<&str> = wanted.iter()
            .filter(|w| !resolved.iter().any(|r| r.version.project_id.as_ref() == w.project_id))
            .map(|w| w.name)
            .collect();
        if !skipped.is_empty() {
            self.send.send_info(format!("Max FPS: no {minecraft_version} build of {}, skipped", skipped.join(", ")));
        }
        log::info!("Max FPS preset for {minecraft_version}: {}", resolved.iter()
            .map(|r| format!("{} {}", r.mod_name, r.version.version_number.as_deref().unwrap_or("?")))
            .collect::<Vec<_>>().join(", "));

        let mut configuration = InstanceConfiguration::new(minecraft_version, Loader::Fabric);
        configuration.memory = Some(InstanceMemoryConfiguration { enabled: true, min: 2048, max: 4096 });
        configuration.jvm_flags = Some(InstanceJvmFlagsConfiguration { enabled: true, flags: Arc::from(JVM_FLAGS) });

        let mut files: Vec<(Arc<str>, Arc<[u8]>)> = vec![(Arc::from("options.txt"), Arc::from(OPTIONS_TXT.as_bytes()))];
        if let Some((filename, jar)) = crate::max_fps_mod::bundled_jar(&minecraft_version) {
            files.push((Arc::from(format!("mods/{filename}").as_str()), Arc::from(jar)));
        }

        let content = ContentInstall {
            target: InstallTarget::NewInstance {
                name: Some(Arc::from(name.as_str())),
                preset: Some(Arc::new(NewInstancePreset { configuration, files })),
            },
            loader: Loader::Fabric,
            minecraft_version,
            files: resolved.iter().map(|r| ContentInstallFile {
                replace_old: None,
                path: ContentInstallPath::Automatic,
                download: ContentDownload::Modrinth {
                    project_id: r.version.project_id.clone(),
                    version_id: Some(r.version.id.clone()),
                    install_dependencies: true,
                },
                content_source: ContentSource::ModrinthProject { project_id: r.version.project_id.clone() },
                reason: ContentInstallReason::Standalone,
            }).collect(),
        };

        self.install_content(content, modal_action.clone()).await;
        modal_action.set_finished();
        self.send.send(MessageToFrontend::Refresh);
    }
}
