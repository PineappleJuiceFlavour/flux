use std::sync::Arc;

use gpui::SharedString;

pub struct SearchHelper<T> {
    last_search: SharedString,
    items: Arc<[T]>,
    lower_keys: Vec<SharedString>,
    searched_starts_with: Vec<usize>,
    searched_contains: Vec<usize>,
    searched: bool,
}

impl<T> SearchHelper<T> {
    pub fn new(items: Arc<[T]>, key: impl Fn(&T) -> SharedString) -> Self {
        let lower_keys = items
            .iter()
            .map(key)
            .map(|key| {
                if key.chars().any(char::is_uppercase) {
                    key.to_lowercase().into()
                } else {
                    key.clone()
                }
            })
            .collect();
        Self {
            last_search: SharedString::default(),
            items,
            lower_keys,
            searched_starts_with: Vec::new(),
            searched_contains: Vec::new(),
            searched: false,
        }
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        if self.searched {
            let item_idx = if index >= self.searched_starts_with.len() {
                self.searched_contains.get(index - self.searched_starts_with.len())?
            } else {
                self.searched_starts_with.get(index)?
            };
            self.items.get(*item_idx)
        } else {
            self.items.get(index)
        }
    }

    pub fn iter(&self) -> Option<impl Iterator<Item = &T>> {
        if self.searched {
            Some(
                self.searched_starts_with
                    .iter()
                    .chain(self.searched_contains.iter())
                    .map(|i| self.items.get(*i).unwrap()),
            )
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        if self.searched {
            self.searched_starts_with.len() + self.searched_contains.len()
        } else {
            self.items.len()
        }
    }

    pub fn search(&mut self, query: &str) {
        if query.is_empty() {
            self.searched = false;
            self.last_search = SharedString::default();
            return;
        }

        if self.searched && query == self.last_search.as_str() {
            return;
        }

        if self.searched && query.starts_with(self.last_search.as_str()) {
            self.searched_contains.retain(|i| self.lower_keys[*i].contains(query));
            self.searched_starts_with.retain(|i| {
                let key = &self.lower_keys[*i];
                if !key.starts_with(query) {
                    if key.contains(query)
                        && let Err(insert_at) = self.searched_contains.binary_search(i)
                    {
                        self.searched_contains.insert(insert_at, *i);
                    }
                    false
                } else {
                    true
                }
            });
        } else {
            self.searched_contains.clear();
            self.searched_starts_with.clear();
            for (index, key) in self.lower_keys.iter().enumerate() {
                if key.starts_with(query) {
                    self.searched_starts_with.push(index);
                } else if key.contains(query) {
                    self.searched_contains.push(index);
                }
            }
        }

        self.searched = true;
        self.last_search = SharedString::new(query);
    }
}
