use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentSource {
    #[default]
    Manual,
    ModrinthUnknown,
    ModrinthProject {
        project_id: Arc<str>
    },
    CurseforgeProject {
        project_id: u32,
    }
}

impl ContentSource {
    pub fn can_be_replaced(&self) -> bool {
        match self {
            ContentSource::Manual => true,
            ContentSource::ModrinthUnknown => true,
            ContentSource::ModrinthProject { .. } => false,
            ContentSource::CurseforgeProject { .. } => false,
        }
    }

    pub fn should_replace_with(&self, other: &ContentSource) -> bool {
        match self {
            ContentSource::Manual => true,
            ContentSource::ModrinthUnknown => matches!(other, ContentSource::ModrinthProject { .. }),
            ContentSource::ModrinthProject { .. } => false,
            ContentSource::CurseforgeProject { .. } => false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentInstallReason {
    Standalone,
    Dependency,
    Modpack,
    Update,
}
