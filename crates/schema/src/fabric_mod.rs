use std::{collections::HashMap, sync::Arc};

use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FabricModJson {
    pub id: Arc<str>,
    pub version: Arc<str>,
    pub name: Option<Arc<str>>,
    // pub description: Option<Arc<str>>,
    pub authors: Option<Vec<Person>>,
    pub icon: Option<Icon>,
    // #[serde(alias = "requires")]
    // pub depends: Option<HashMap<Arc<str>, Dependency>>,
    // pub breaks: Option<HashMap<Arc<str>, Dependency>>,
}

// #[derive(Deserialize, Debug)]
// #[serde(untagged)]
// enum Dependency {
//     Single(Arc<str>),
//     Multiple(Vec<Arc<str>>)
// }

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum Icon {
    Single(Arc<str>),
    Sizes(HashMap<usize, Arc<str>>),
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum Person {
    Name(Arc<str>),
    NameAndContact { name: Arc<str> },
}

impl Person {
    pub fn name(&self) -> &str {
        match self {
            Person::Name(name) => name,
            Person::NameAndContact { name, .. } => name,
        }
    }
}
