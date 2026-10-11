use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use ustr::Ustr;

pub const JAVA_RUNTIMES_URL: &str =
    "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

#[derive(Deserialize, Debug)]
pub struct JavaRuntimes {
    #[serde(flatten)]
    pub platforms: HashMap<Ustr, JavaRuntimePlatform>,
}

#[derive(Deserialize, Debug)]
pub struct JavaRuntimePlatform {
    #[serde(flatten)]
    pub components: HashMap<Ustr, Vec<JavaRuntimeComponent>>,
}

#[derive(Deserialize, Debug)]
#[cfg_attr(debug_assertions, serde(deny_unknown_fields))]
pub struct JavaRuntimeComponent {
    pub availability: JavaRuntimeComponentAvailability,
    pub manifest: JavaRuntimeComponentManifestLink,
    pub version: JavaRuntimeComponentVersion,
}

#[derive(Deserialize, Debug)]
#[cfg_attr(debug_assertions, serde(deny_unknown_fields))]
pub struct JavaRuntimeComponentManifestLink {
    pub sha1: Ustr,
    pub size: u32,
    pub url: Ustr,
}

#[derive(Deserialize, Debug)]
#[cfg_attr(debug_assertions, serde(deny_unknown_fields))]
pub struct JavaRuntimeComponentVersion {
    pub name: Ustr,
    pub released: DateTime<Utc>,
}

#[derive(Deserialize, Debug)]
#[cfg_attr(debug_assertions, serde(deny_unknown_fields))]
pub struct JavaRuntimeComponentAvailability {
    pub group: u32,
    pub progress: u32,
}
