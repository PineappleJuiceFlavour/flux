use indexmap::IndexMap;
use serde::Deserialize;
use ustr::Ustr;

#[derive(Deserialize, Clone, Debug)]
#[cfg_attr(debug_assertions, serde(deny_unknown_fields))]
pub struct AssetsIndex {
    pub objects: IndexMap<Ustr, AssetObject>,
    // Used for 1.7 and below, indicates that the objects should be stored
    // in assets/virtual/{assets_id}/ instead
    pub r#virtual: Option<bool>,
    // Used for 1.5 and below, indicates that the objects should be stored
    // in .minecraft/resources instead
    pub map_to_resources: Option<bool>,
}

#[derive(Deserialize, Clone, Debug)]
#[cfg_attr(debug_assertions, serde(deny_unknown_fields))]
pub struct AssetObject {
    pub hash: Ustr,
    pub size: u32,
}
