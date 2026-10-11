use std::sync::Arc;

use serde::Deserialize;
use ustr::Ustr;

use crate::fabric_loader_manifest::FabricLoaderVersion;

#[derive(Deserialize, Debug)]
#[cfg_attr(debug_assertions, serde(deny_unknown_fields))]
pub struct FabricLaunch {
    pub loader: Option<FabricLoaderVersion>,
    pub intermediary: Option<FabricIntermediaryVersion>,
    #[serde(rename = "launcherMeta")]
    pub launcher_meta: FabricLaunchLauncherMeta,
}

#[derive(Deserialize, Debug)]
#[cfg_attr(debug_assertions, serde(deny_unknown_fields))]
pub struct FabricIntermediaryVersion {
    pub maven: Ustr,
    pub version: Ustr,
    pub stable: bool,
}

#[derive(Deserialize, Debug)]
#[cfg_attr(debug_assertions, serde(deny_unknown_fields))]
pub struct FabricLaunchLauncherMeta {
    pub version: u32,
    pub min_java_version: u32,
    pub libraries: FabricLaunchLibraries,
    #[serde(rename = "mainClass")]
    pub main_class: FabricLaunchMainClasses,
}

#[derive(Deserialize, Debug)]
#[cfg_attr(debug_assertions, serde(deny_unknown_fields))]
pub struct FabricLaunchLibraries {
    pub client: Arc<[FabricLaunchLibrary]>,
    pub common: Arc<[FabricLaunchLibrary]>,
    pub server: Arc<[FabricLaunchLibrary]>,
    pub development: Arc<[FabricLaunchLibrary]>,
}

#[derive(Deserialize, Debug)]
pub struct FabricLaunchLibrary {
    pub name: Ustr,
    pub url: Ustr,
    pub sha1: Ustr,
    pub size: u32,
}

#[derive(Deserialize, Debug)]
#[cfg_attr(debug_assertions, serde(deny_unknown_fields))]
pub struct FabricLaunchMainClasses {
    pub client: Ustr,
    pub server: Ustr,
}
