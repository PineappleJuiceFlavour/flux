use serde::{Deserialize, Serialize};

pub const INSTANCE_NAME: &'static str = "__QUICKPLAY__";

#[derive(Debug, Default, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum QuickplayPreset {
    #[default]
    Vanilla,
    Performance,
    Expanded,
}
