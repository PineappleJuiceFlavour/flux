use std::sync::Arc;

use gpui::{prelude::*, *};
use gpui_component::{
    IndexPath,
    select::{SelectDelegate, SelectItem, SelectState},
};

use crate::{component::search_helper::SearchHelper, entity::instance::InstanceEntry};

pub struct InstanceDropdown {
    instances: Arc<[InstanceEntry]>,
    search: SearchHelper<InstanceEntry>,
}

impl InstanceDropdown {
    pub fn create(instances: Arc<[InstanceEntry]>, window: &mut Window, cx: &mut App) -> Entity<SelectState<Self>> {
        cx.new(|cx| {
            let instance_list = Self {
                instances: instances.clone(),
                search: SearchHelper::new(instances, |item| item.name.clone()),
            };
            SelectState::new(instance_list, None, window, cx).searchable(true)
        })
    }

    pub fn get(&self, index: usize) -> Option<&InstanceEntry> {
        self.search.get(index)
    }
}

impl SelectDelegate for InstanceDropdown {
    type Item = InstanceEntry;

    fn items_count(&self, _section: usize) -> usize {
        self.search.len()
    }

    fn item(&self, ix: gpui_component::IndexPath) -> Option<&Self::Item> {
        self.search.get(ix.row)
    }

    fn position<V>(&self, value: &V) -> Option<gpui_component::IndexPath>
    where
        Self::Item: gpui_component::select::SelectItem<Value = V>,
        V: PartialEq,
    {
        if let Some(searched_iter) = self.search.iter() {
            for (ix, item) in searched_iter.enumerate() {
                if item.value() == value {
                    return Some(IndexPath::default().row(ix));
                }
            }
        } else {
            for (ix, item) in self.instances.iter().enumerate() {
                if item.value() == value {
                    return Some(IndexPath::default().row(ix));
                }
            }
        }

        None
    }

    fn perform_search(
        &mut self,
        query: &str,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<()> {
        self.search.search(query);
        Task::ready(())
    }
}
