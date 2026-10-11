use gpui::{prelude::*, *};
use gpui_component::{
    IndexPath,
    select::{SelectDelegate, SelectItem, SelectState},
};

#[derive(Clone)]
pub enum DropdownName {
    Literal(SharedString),
    Translated(fn() -> &'static str),
}

impl DropdownName {
    pub fn new(name: impl Into<SharedString>) -> Self {
        Self::Literal(name.into())
    }

    pub fn translated(f: fn() -> &'static str) -> Self {
        Self::Translated(f)
    }

    pub fn as_str(&self) -> &str {
        match self {
            DropdownName::Literal(shared_string) => &**shared_string,
            DropdownName::Translated(f) => (f)(),
        }
    }
}

impl From<SharedString> for DropdownName {
    fn from(value: SharedString) -> Self {
        Self::Literal(value)
    }
}

#[derive(Clone)]
pub struct NamedDropdownItem<T: Clone + PartialEq> {
    pub name: DropdownName,
    pub item: T
}

impl <T: Clone + PartialEq> PartialEq for NamedDropdownItem<T> {
    fn eq(&self, other: &Self) -> bool {
        self.item == other.item
    }
}

impl<T: Clone + PartialEq> SelectItem for NamedDropdownItem<T> {
    type Value = T;

    fn title(&self) -> SharedString {
        match &self.name {
            DropdownName::Literal(name) => name.clone(),
            DropdownName::Translated(name) => name().into(),
        }
    }

    fn value(&self) -> &Self::Value {
        &self.item
    }
}

pub struct NamedDropdown<T: Clone + PartialEq> {
    items: Vec<NamedDropdownItem<T>>,
}

impl<T: Clone + PartialEq> NamedDropdown<T> {
    pub fn new(items: Vec<NamedDropdownItem<T>>) -> Self {
        Self {
            items,
        }
    }

    pub fn create(items: Vec<NamedDropdownItem<T>>, window: &mut Window, cx: &mut App) -> Entity<SelectState<Self>> {
        cx.new(|cx| {
            let delegate = Self::new(items);
            SelectState::new(delegate, None, window, cx)
        })
    }

    pub fn create_and_select(items: Vec<NamedDropdownItem<T>>, selected: T, window: &mut Window, cx: &mut App) -> Entity<SelectState<Self>> {
        cx.new(|cx| {
            let delegate = Self::new(items);
            let mut select_state = SelectState::new(delegate, None, window, cx);
            select_state.set_selected_value(&selected, window, cx);
            select_state
        })
    }
}

impl<T: Clone + PartialEq + 'static> SelectDelegate for NamedDropdown<T> {
    type Item = NamedDropdownItem<T>;

    fn items_count(&self, _section: usize) -> usize {
        self.items.len()
    }

    fn item(&self, ix: gpui_component::IndexPath) -> Option<&Self::Item> {
        self.items.get(ix.row)
    }

    fn position<V>(&self, value: &V) -> Option<gpui_component::IndexPath>
    where
        Self::Item: gpui_component::select::SelectItem<Value = V>,
        V: PartialEq,
    {
        for (ix, item) in self.items.iter().enumerate() {
            if item.value() == value {
                return Some(IndexPath::default().row(ix));
            }
        }

        None
    }

    fn perform_search(
        &mut self,
        _query: &str,
        _window: &mut Window,
        _: &mut App,
    ) -> Task<()> {
        Task::ready(())
    }
}

pub struct SearchableNamedDropdown<T: Clone + PartialEq> {
    items: Vec<NamedDropdownItem<T>>,
    casefolded: Option<(u8, Vec<String>)>,
    last_query: Option<String>,
    searched: Option<Vec<usize>>,
}

impl<T: Clone + PartialEq> SearchableNamedDropdown<T> {
    pub fn new(items: Vec<NamedDropdownItem<T>>) -> Self {
        Self {
            items,
            casefolded: None,
            last_query: None,
            searched: None,
        }
    }

    pub fn create(items: Vec<NamedDropdownItem<T>>, window: &mut Window, cx: &mut App) -> Entity<SelectState<Self>> {
        cx.new(|cx| {
            let delegate = Self::new(items);
            SelectState::new(delegate, None, window, cx).searchable(true)
        })
    }

    pub fn create_and_select(items: Vec<NamedDropdownItem<T>>, selected: T, window: &mut Window, cx: &mut App) -> Entity<SelectState<Self>> {
        cx.new(|cx| {
            let delegate = Self::new(items);
            let mut select_state = SelectState::new(delegate, None, window, cx).searchable(true);
            select_state.set_selected_value(&selected, window, cx);
            select_state
        })
    }
}

impl<T: Clone + PartialEq + 'static> SelectDelegate for SearchableNamedDropdown<T> {
    type Item = NamedDropdownItem<T>;

    fn items_count(&self, _section: usize) -> usize {
        self.searched.as_ref().map(|s| s.len()).unwrap_or(self.items.len())
    }

    fn item(&self, ix: gpui_component::IndexPath) -> Option<&Self::Item> {
        if let Some(searched) = &self.searched {
            self.items.get(*searched.get(ix.row)?)
        } else {
            self.items.get(ix.row)
        }
    }

    fn position<V>(&self, value: &V) -> Option<gpui_component::IndexPath>
    where
        Self::Item: gpui_component::select::SelectItem<Value = V>,
        V: PartialEq,
    {
        if let Some(searched) = &self.searched {
            for (ix, index) in searched.iter().enumerate() {
                let item = &self.items[*index];
                if item.value() == value {
                    return Some(IndexPath::default().row(ix));
                }
            }
        } else {
            for (ix, item) in self.items.iter().enumerate() {
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
        _: &mut App,
    ) -> Task<()> {
        if query.is_empty() {
            self.last_query = None;
            self.searched = None;
            return Task::ready(());
        }

        let lang_id = t::get_current_lang_id();
        if self.casefolded.as_ref().map(|(l, _)| *l != lang_id).unwrap_or(true) {
            let (_, mut casefolded) = self.casefolded.take().unwrap_or_else(|| (0, Vec::with_capacity(self.items.len())));
            casefolded.clear();

            for item in &self.items {
                casefolded.push(casefold::simple_fold(item.name.as_str().to_string()));
            }

            self.casefolded = Some((lang_id, casefolded));
        }
        let (_, casefolded) = self.casefolded.as_ref().unwrap();

        let query = casefold::simple_fold(query.to_string());

        if let Some(searched) = &mut self.searched && let Some(last_query) = &self.last_query && query.contains(last_query) {
            searched.retain(|index| casefolded[*index].contains(&query));
            self.last_query = Some(query);
        } else {
            let searched = self.searched.get_or_insert_default();
            searched.clear();

            for (index, item) in casefolded.iter().enumerate() {
                if item.contains(&query) {
                    searched.push(index);
                }
            }

            self.last_query = Some(query);
        }
        Task::ready(())
    }
}
