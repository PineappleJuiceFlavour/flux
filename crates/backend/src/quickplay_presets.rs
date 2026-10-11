use std::sync::Arc;

use bridge::{install::{ContentDownload, ContentInstallFile, ContentInstallPath}, modal_action::{ModalAction, ProgressTracker}};
use indexmap::{IndexMap, IndexSet};
use rustc_hash::{FxHashMap, FxHashSet};
use schema::{content::{ContentInstallReason, ContentSource}, loader::Loader, modrinth::{ModrinthDependencyType, ModrinthProjectVersion, ModrinthProjectVersionsRequest, ModrinthProjectVersionsResult}, quickplay::QuickplayPreset};
use ustr::Ustr;

use crate::metadata::{items::ModrinthProjectVersionsMetadataItem, manager::MetadataManager};

pub fn loader(preset: QuickplayPreset) -> Loader {
    match preset {
        QuickplayPreset::Vanilla => Loader::Vanilla,
        QuickplayPreset::Performance => Loader::Fabric,
        QuickplayPreset::Expanded => Loader::Fabric,
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum WantState {
    Unwanted,
    Optional,
    Required,
}

#[derive(Clone, Debug)]
enum ModrinthProjectInstall {
    Incompatible {
        wanted: bool,
    },
    Resolved(Arc<str>),
    Unresolved {
        want: WantState,
        incompatible_versions: Arc<FxHashSet<Arc<str>>>,
        preferred_versions: Arc<FxHashSet<Arc<str>>>,
        processing_versions: Option<Vec<ModrinthProjectVersion>>
    }
}

impl ModrinthProjectInstall {
    pub fn is_wanted_unresolved(&self) -> bool {
        match self {
            ModrinthProjectInstall::Incompatible { .. } => false,
            ModrinthProjectInstall::Resolved(_) => false,
            ModrinthProjectInstall::Unresolved { want, .. } => *want != WantState::Unwanted,
        }
    }

    pub fn is_required_unresolved(&self) -> bool {
        match self {
            ModrinthProjectInstall::Incompatible { .. } => false,
            ModrinthProjectInstall::Resolved(_) => false,
            ModrinthProjectInstall::Unresolved { want, .. } => *want == WantState::Required,
        }
    }

    pub fn try_merge(&mut self, other: ModrinthProjectInstall) -> bool {
        match self {
            ModrinthProjectInstall::Incompatible { wanted: self_wanted } => match other {
                ModrinthProjectInstall::Incompatible { wanted: other_wanted } => {
                    *self_wanted |= other_wanted;
                    true
                },
                ModrinthProjectInstall::Resolved(_) => false,
                ModrinthProjectInstall::Unresolved { want: other_want, .. } => {
                    *self_wanted |= other_want != WantState::Unwanted;
                    true
                },
            },
            ModrinthProjectInstall::Resolved(self_version) => match other {
                ModrinthProjectInstall::Incompatible { .. } => false,
                ModrinthProjectInstall::Resolved(other_version) => self_version == &other_version,
                ModrinthProjectInstall::Unresolved { incompatible_versions, .. } => {
                    !incompatible_versions.contains(self_version)
                },
            },
            ModrinthProjectInstall::Unresolved {
                want: self_want,
                incompatible_versions: self_incompatible,
                preferred_versions: self_preferred,
                ..
            } => match other {
                ModrinthProjectInstall::Incompatible { wanted: other_wanted } => {
                    if *self_want == WantState::Required {
                        return false;
                    } else {
                        *self = ModrinthProjectInstall::Incompatible { wanted: other_wanted || *self_want != WantState::Unwanted };
                        true
                    }
                },
                ModrinthProjectInstall::Resolved(other_version) => {
                    if self_incompatible.contains(&other_version) {
                        return false;
                    } else {
                        *self = ModrinthProjectInstall::Resolved(other_version);
                        return true;
                    }
                },
                ModrinthProjectInstall::Unresolved {
                    want: other_want,
                    incompatible_versions: other_incompatible,
                    preferred_versions: other_preferred,
                    ..
                } => {
                    *self_want = (*self_want).max(other_want);
                    extend_arc(self_incompatible, other_incompatible);
                    extend_arc(self_preferred, other_preferred);
                    remove_all_arc(self_preferred, self_incompatible.clone());
                    true
                }
            },
        }
    }
}

fn extend_arc(one: &mut Arc<FxHashSet<Arc<str>>>, two: Arc<FxHashSet<Arc<str>>>) {
    if two.is_empty() {
        return;
    }
    if one.is_empty() {
        *one = two;
    } else {
        for value in two.iter() {
            if !one.contains(value) {
                Arc::make_mut(one).insert(value.clone());
            }
        }
    }
}

fn remove_all_arc(one: &mut Arc<FxHashSet<Arc<str>>>, two: Arc<FxHashSet<Arc<str>>>) {
    if two.is_empty() {
        return;
    }
    if !one.is_empty() {
        for value in two.iter() {
            if one.contains(value) {
                Arc::make_mut(one).remove(value);
            }
        }
    }
}

#[derive(Clone, Debug)]
struct ResolutionState {
    installs: IndexMap::<Arc<str>, ModrinthProjectInstall>,
    initial: bool,
}

impl ResolutionState {
    fn try_apply_version(&mut self, candidate: &ModrinthProjectVersion) -> bool {
        self.installs.insert(candidate.project_id.clone(), ModrinthProjectInstall::Resolved(candidate.id.clone()));

        if let Some(dependencies) = &candidate.dependencies {
            for dependent in dependencies {
                let Some(dependent_project_id) = &dependent.project_id else {
                    continue;
                };
                let desired = match dependent.dependency_type {
                    ModrinthDependencyType::Required => {
                        if let Some(version) = &dependent.version_id {
                            ModrinthProjectInstall::Resolved(version.clone())
                        } else {
                            ModrinthProjectInstall::Unresolved {
                                want: WantState::Required,
                                incompatible_versions: Default::default(),
                                preferred_versions: Default::default(),
                                processing_versions: None,
                            }
                        }
                    },
                    ModrinthDependencyType::Optional => {
                        // We don't include optional dependencies by default, but we will check
                        // if a version is recommended and prefer that
                        if let Some(version) = dependent.version_id.clone() {
                            let mut preferred_versions = FxHashSet::default();
                            preferred_versions.insert(version);
                            ModrinthProjectInstall::Unresolved {
                                want: WantState::Unwanted,
                                incompatible_versions: Default::default(),
                                preferred_versions: Arc::new(preferred_versions),
                                processing_versions: None,
                            }
                        } else {
                            ModrinthProjectInstall::Unresolved {
                                want: WantState::Unwanted,
                                incompatible_versions: Default::default(),
                                preferred_versions: Default::default(),
                                processing_versions: None,
                            }
                        }
                    },
                    ModrinthDependencyType::Incompatible => {
                        if let Some(version) = dependent.version_id.clone() {
                            let mut incompatible_versions = FxHashSet::default();
                            incompatible_versions.insert(version);
                            ModrinthProjectInstall::Unresolved {
                                want: WantState::Unwanted,
                                incompatible_versions: Arc::new(incompatible_versions),
                                preferred_versions: Default::default(),
                                processing_versions: None,
                            }
                        } else {
                            ModrinthProjectInstall::Incompatible { wanted: false }
                        }
                    },
                    ModrinthDependencyType::Embedded => continue,
                };

                if let Some(existing) = self.installs.get_mut(dependent_project_id) {
                    if !existing.try_merge(desired) {
                        return false;
                    }
                } else {
                    self.installs.insert(dependent_project_id.clone(), desired);
                }
            }
        }

        true
    }
}

struct ModSetResolver {
    tracker: ProgressTracker,
    remaining_extra_on_tracker: usize,
    original_projects: IndexSet<Arc<str>>,
    project_versions: FxHashMap::<Arc<str>, Option<Arc<ModrinthProjectVersionsResult>>>,
    depended_by_counts: FxHashMap::<(Arc<str>, Arc<str>), usize>,
    stack: Vec<ResolutionState>,
    minecraft_version: Ustr,
    loader: Loader,
    meta: Arc<MetadataManager>
}

impl ModSetResolver {
    async fn try_resolve(&mut self) {
        'out: loop {
            let Some(current) = self.stack.last_mut() else {
                return;
            };

            if current.initial {
                current.initial = false;

                // Sort so required dependencies come first
                current.installs.sort_by_key(|_, v| !v.is_required_unresolved());

                // Download any missing versions
                let missing_versions = current.installs.iter()
                    .filter_map(|(k, v)| {
                        if !self.project_versions.contains_key(k) && v.is_wanted_unresolved() {
                            Some(k.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<FxHashSet<_>>();

                if !missing_versions.is_empty() {
                    let mut add_total = missing_versions.len();
                    if self.remaining_extra_on_tracker >= add_total {
                        self.remaining_extra_on_tracker -= add_total;
                        add_total = 0;
                    } else {
                        add_total -= self.remaining_extra_on_tracker;
                        self.remaining_extra_on_tracker = 0;
                    }
                    self.tracker.add_total(add_total);

                    let new_project_versions = futures::future::join_all(missing_versions.iter().map(|project_id| {
                        async {
                            let versions = self.meta.fetch(ModrinthProjectVersionsMetadataItem(ModrinthProjectVersionsRequest {
                                project_id: (*project_id).clone(),
                                game_versions: Some([self.minecraft_version.into()].into()),
                                loaders: Some(Arc::new([self.loader.as_modrinth_loader()])),
                            })).await;
                            self.tracker.add_count(1);
                            versions
                        }
                    })).await;

                    for (version, result) in missing_versions.into_iter().zip(new_project_versions.into_iter()) {
                        self.project_versions.insert(version, result.ok());
                    }
                }
            }

            let mut fork = current.clone();
            for (project, install) in &mut current.installs {
                let ModrinthProjectInstall::Unresolved {
                    want,
                    incompatible_versions,
                    preferred_versions,
                    processing_versions,
                } = install else {
                    continue;
                };

                if *want == WantState::Unwanted {
                    continue;
                }

                let processing_versions = processing_versions.get_or_insert_with(|| {
                    let Some(Some(version)) = self.project_versions.get(project) else {
                        return Vec::new();
                    };

                    let mut versions = version.0.to_vec();

                    // Remove incompatible versions
                    versions.retain(|v| !incompatible_versions.contains(&v.id));

                    // We sort and pop off the end, so reverse the vec so newest versions are last
                    versions.reverse();

                    // Sort so preferred versions come last (we pop off the end)
                    if !preferred_versions.is_empty() {
                        versions.sort_by_key(|v| preferred_versions.contains(&v.id));
                    }

                    // Sort by the number of other projects which depend on each version in order to maximize compatibility
                    // e.g. if Sodium 0.9.2 is depended on by [BBE, Iris] and
                    // Sodium 0.9.1 is depended on by [BBE, Iris, Voxy] then
                    // we should prefer 0.9.1 since otherwise Voxy will be incompatible
                    versions.sort_by_cached_key(|v| {
                        // Find number of unresolved projects that depend on this version
                        // More dependencies = later in the vec (we pop off the end)
                        match self.depended_by_counts.entry((v.project_id.clone(), v.id.clone())) {
                            std::collections::hash_map::Entry::Occupied(occupied_entry) => {
                                *occupied_entry.get()
                            },
                            std::collections::hash_map::Entry::Vacant(vacant_entry) => {
                                let mut count = 0;
                                'other_projects: for other_project in &self.original_projects {
                                    if other_project == &v.project_id {
                                        continue;
                                    }

                                    let Some(Some(versions)) = self.project_versions.get(other_project) else {
                                        continue;
                                    };

                                    for version in versions.0.iter() {
                                        if incompatible_versions.contains(&version.id) {
                                            continue;
                                        }
                                        if let Some(dependencies) = &version.dependencies {
                                            for dependency in dependencies {
                                                if dependency.dependency_type != ModrinthDependencyType::Required {
                                                    continue;
                                                }
                                                let Some(project_id) = &dependency.project_id else {
                                                    continue;
                                                };
                                                if project_id != &v.project_id {
                                                    continue;
                                                }
                                                if let Some(version_id) = &dependency.version_id && version_id != &v.id {
                                                    continue;
                                                }
                                                count += 1;
                                                continue 'other_projects; // Break so we don't increase the count more than once for this dependent project
                                            }
                                        }
                                    }
                                }
                                vacant_entry.insert(count);
                                count
                            },
                        }
                    });

                    versions
                });

                while let Some(candidate_version) = processing_versions.pop() {
                    if incompatible_versions.contains(&candidate_version.id) {
                        continue;
                    }
                    if project != &candidate_version.project_id {
                        // Modrinth must have really messed up here
                        continue;
                    }

                    let mut fork = fork.clone();
                    if !fork.try_apply_version(&candidate_version) {
                        continue;
                    }
                    fork.initial = true;
                    self.stack.push(fork);
                    continue 'out;
                }

                if *want == WantState::Required {
                    // Required a project that we couldn't find
                    self.stack.pop();
                    continue 'out;
                } else {
                    // Mark as incompatible
                    fork.installs.insert(project.clone(), ModrinthProjectInstall::Incompatible { wanted: *want != WantState::Unwanted });
                    *install = ModrinthProjectInstall::Incompatible { wanted: *want != WantState::Unwanted };
                    continue;
                }
            }

            // Success, everything is resolved
            return;
        }
    }
}

pub async fn resolve_installs(preset: QuickplayPreset, minecraft_version: Ustr, meta: Arc<MetadataManager>, modal_action: &ModalAction) -> Arc<[ContentInstallFile]> {
    let loader = loader(preset);
    if loader == Loader::Vanilla {
        return Arc::new([]);
    }

    let mut installs = IndexMap::<Arc<str>, ModrinthProjectInstall>::default();

    let original_projects = modrinth_projects(preset, minecraft_version);
    let original_projects: IndexSet<Arc<str>> = original_projects.iter().map(|s| Arc::from(*s)).collect();
    for project in &original_projects {
        installs.insert(project.clone(), ModrinthProjectInstall::Unresolved {
            want: WantState::Optional,
            incompatible_versions: Default::default(),
            preferred_versions: Default::default(),
            processing_versions: None,
        });
    }

    let tracker = modal_action.push_tracker("Resolving mods...".into());
    let extra = original_projects.len() * 3 / 2;
    tracker.add_total(extra);

    let mut resolver = ModSetResolver {
        tracker,
        remaining_extra_on_tracker: extra,
        original_projects: original_projects.clone(),
        project_versions: Default::default(),
        depended_by_counts: Default::default(),
        stack: vec![ResolutionState { installs, initial: true }],
        minecraft_version,
        loader,
        meta,
    };

    resolver.try_resolve().await;

    log::info!("Finished resolving quickplay mods");

    resolver.tracker.add_count(resolver.remaining_extra_on_tracker);
    resolver.tracker.set_finished(bridge::modal_action::ProgressTrackerFinishType::Normal);

    let Some(last) = resolver.stack.last() else {
        return Arc::new([]);
    };

    last.installs.iter().filter_map(|(project_id, install)| {
        match install {
            ModrinthProjectInstall::Incompatible { wanted } => {
                if *wanted {
                    log::warn!("Skipping project {}, unable to find compatible version", project_id);
                }
                None
            },
            ModrinthProjectInstall::Resolved(version_id) => Some(ContentInstallFile {
                replace_old: None,
                path: ContentInstallPath::Automatic,
                download: ContentDownload::Modrinth {
                    project_id: project_id.clone(),
                    version_id: Some(version_id.clone()),
                    install_dependencies: false
                },
                content_source: ContentSource::ModrinthProject { project_id: project_id.clone() },
                reason: if original_projects.contains(project_id) {
                    ContentInstallReason::Modpack
                } else {
                    ContentInstallReason::Dependency
                },
            }),
            ModrinthProjectInstall::Unresolved { want, .. } => {
                if *want != WantState::Unwanted {
                    log::error!("Project {} was left unresolved, this shouldn't happen", project_id);
                }
                None
            },
        }
    }).collect()
}

pub fn modrinth_projects(preset: QuickplayPreset, _minecraft_version: Ustr) -> &'static [&'static str] {
    match preset {
        QuickplayPreset::Vanilla => &[],
        QuickplayPreset::Performance => PERFORMANCE,
        QuickplayPreset::Expanded => EXPANDED,
    }
}

// Ordering is important! Projects that are depended on should come first
static PERFORMANCE: &'static [&'static str] = &[
    "P7dR8mSH", // Fabric API
    "mOgUt4GM", // Mod Menu
    "AANobbMI", // Sodium
    "gvQqBUqZ", // Lithium
    "VSNURh3q", // c2me
    "p8RJPJIC", // Ixeris
    // "fQEb0iXm", // No Krypton (incompatible with e4mc)
    "ONZm0H7Y", // Better Block Entities
    "OnlVIpq5", // Fast Noise
    "x1hIzbuY", // Fast Quit
    "ZP7xHXtw", // Remove Reloading Screen
    "alhWWxax", // Cull Fewer Leaves
];

// Ordering is important! Projects that are depended on should come first
static EXPANDED: &'static [&'static str] = &[
    // Same as PERFORMANCE
    "P7dR8mSH", // Fabric API
    "mOgUt4GM", // Mod Menu
    "AANobbMI", // Sodium
    "gvQqBUqZ", // Lithium
    "VSNURh3q", // c2me
    "p8RJPJIC", // Ixeris
    // "fQEb0iXm", // No Krypton (incompatible with e4mc)
    "ONZm0H7Y", // Better Block Entities
    "OnlVIpq5", // Fast Noise
    "x1hIzbuY", // Fast Quit
    "ZP7xHXtw", // Remove Reloading Screen
    "alhWWxax", // Cull Fewer Leaves

    "YL57xq9U", // Iris
    "4das1Fjq", // Flashback
    "9eGKb6K1", // Simple Voice Chat
    "w7ThoJFB", // Zoomify
    "qANg5Jrr", // e4mc
    "QwxR6Gcd", // Debugify
    "M08ruV16", // Bobby
    "fxxUqruK", // Voxy
    "Kw7Sm3Xf", // Noxesium
    "bEpr0Arc", // Litematica
    "TnOXNf5e", // Peek
    "EsAfCjCV", // Apple Skin
];
