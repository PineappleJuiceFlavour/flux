use std::sync::Arc;

use bridge::{instance::InstanceStatus, message::MessageToBackend};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Icon, Sizable, StyledExt, button::{Button, ButtonVariants}, h_flex, menu::{DropdownMenu, PopupMenuItem}, separator::Separator, table::{Column, ColumnSort, TableDelegate, TableState}, v_flex
};
use indexmap::IndexMap;

use crate::{
    component::{clipped_element::AnimatedClippedElement, reorderable::{Reorderable, ReorderableDragInfo, ReorderableState, SimpleDragPreview}, responsive_grid::ResponsiveGrid}, entity::{
        DataEntities, instance::{InstanceAddedEvent, InstanceEntry, InstanceModifiedEvent, InstanceRemovedEvent}
    }, icon::PandoraIcon, interface_config::InterfaceConfig, png_render_cache, root, ui
};

pub struct InstanceList {
    columns: Vec<Column>,
    items: Vec<InstanceEntry>,
    reorderable_state: Entity<ReorderableState>,
    data: DataEntities,
    _instance_added_subscription: Subscription,
    _instance_removed_subscription: Subscription,
    _instance_modified_subscription: Subscription,
}

impl InstanceList {
    pub fn create_table(data: &DataEntities, window: &mut Window, cx: &mut App) -> Entity<TableState<Self>> {
        let instances = data.instances.clone();
        let items = instances.read(cx).entries.values().filter_map(|i| {
            let entry = i.read(cx).clone();
            if entry.name == schema::quickplay::INSTANCE_NAME {
                None
            } else {
                Some(entry)
            }
        }).collect();
        cx.new(|cx| {
            let _instance_added_subscription = cx.subscribe::<_, InstanceAddedEvent>(&instances, |table: &mut TableState<InstanceList>, _, event, cx| {
                if event.instance.name == schema::quickplay::INSTANCE_NAME {
                    return;
                }
                table.delegate_mut().items.insert(0, event.instance.clone());
                cx.notify();
            });
            let _instance_removed_subscription = cx.subscribe::<_, InstanceRemovedEvent>(&instances, |table, _, event, cx| {
                table.delegate_mut().items.retain(|instance| {
                    instance.id != event.id
                });
                cx.notify();
            });
            let _instance_modified_subscription = cx.subscribe::<_, InstanceModifiedEvent>(&instances, |table, _, event, cx| {
                if let Some(entry) = table.delegate_mut().items.iter_mut().find(|entry| entry.id == event.instance.id) {
                    *entry = event.instance.clone();
                    cx.notify();
                }
            });
            let instance_list = Self {
                columns: vec![
                    Column::new("controls", "")
                        .width(150.)
                        .fixed_left()
                        .movable(false)
                        .resizable(false),
                    Column::new("name", t::instance::name())
                        .width(150.)
                        .fixed_left()
                        .sortable()
                        .resizable(true),
                    Column::new("version", t::instance::version())
                        .width(150.)
                        .fixed_left()
                        .sortable()
                        .resizable(true),
                    Column::new("loader", t::instance::modloader())
                        .width(150.)
                        .fixed_left()
                        .resizable(true),
                ],
                items,
                reorderable_state: ReorderableState::new(window, cx),
                data: data.clone(),
                _instance_added_subscription,
                _instance_removed_subscription,
                _instance_modified_subscription,
            };
            TableState::new(instance_list, window, cx)
        })
    }

    pub fn render_cards(&self, cx: &mut App) -> AnyElement {
        let mut by_group = IndexMap::<Option<Arc<str>>, Vec<Div>>::default();

        for item in &self.items {
            let group = item.configuration.group.clone();
            let rendered = self.render_card(item, cx);
            by_group.entry(group).or_default().push(rendered);
        }

        let size = Size::new(
            gpui::AvailableSpace::MinContent,
            gpui::AvailableSpace::MinContent
        );
        if by_group.len() == 1 {
            ResponsiveGrid::new(size).size_full().gap_4().children(by_group.into_iter().next().unwrap().1).into_any_element()
        } else {
            let size_without_no_group = if by_group.contains_key(&None) {
                by_group.len() - 1
            } else {
                by_group.len()
            };

            let ordering = &InterfaceConfig::get(cx).instance_group_order;

            by_group.sort_by_cached_key(|group_name, _| if let Some(group_name) = group_name {
                ordering.iter().position(|ordering_name| ordering_name == group_name).map(|v| v+1).unwrap_or(0)
            } else {
                std::usize::MAX
            });

            if size_without_no_group == 1 {
                v_flex()
                    .w_full()
                    .gap_2()
                    .children(by_group.into_iter().map(|(group_name, children)| {
                        let group_name = group_name.clone().map(SharedString::from).unwrap_or("No Group".into());
                        Self::render_card_group(group_name, children, cx, None)
                    }))
                    .into_any_element()
            } else {
                if !cx.has_active_drag() {
                    self.reorderable_state.update(cx, |state, cx| {
                        let Some((from, to)) = state.take_reorder() else {
                            return;
                        };

                        // Move element
                        let Some((key, value)) = by_group.shift_remove_index(from) else {
                            return
                        };
                        by_group.shift_insert(to, key, value);

                        // Update ordering
                        let order = &mut InterfaceConfig::get_mut(cx).instance_group_order;
                        order.clear();
                        for (key, _) in &by_group {
                            if let Some(key) = key {
                                order.push(key.clone());
                            }
                        }
                    });
                }

                let no_group_children = by_group.swap_remove(&None);

                let reorderable = Reorderable::new(&self.reorderable_state, by_group.len(), cx, move |render_index, info, _, cx| {
                    let Some((group_name, children)) = by_group.get_index_mut(render_index) else {
                        return div().into_any_element();
                    };
                    let Some(group_name) = group_name.clone().map(SharedString::from) else {
                        return div().into_any_element();
                    };
                    Self::render_card_group(group_name, std::mem::take(children), cx, Some(info))
                }).w_full().v_flex().gap_2().into_any_element();

                v_flex()
                    .w_full()
                    .gap_2()
                    .child(reorderable)
                    .when_some(no_group_children, |this, no_group_children| {
                        this.child(Self::render_card_group("No Group".into(), no_group_children, cx, None))
                    })
                    .into_any_element()
            }
        }
    }

    fn render_card_group(group_name: SharedString, children: Vec<Div>, cx: &mut App, drag_info: Option<ReorderableDragInfo>) -> AnyElement {
        let size = Size::new(
            gpui::AvailableSpace::MinContent,
            gpui::AvailableSpace::MinContent
        );

        let open = !InterfaceConfig::get(cx).instance_groups_closed.contains(&group_name);

        v_flex()
            .child(h_flex()
                .pb_1()
                .gap_2()
                .when_some(drag_info, |this, drag_info| {
                    this.child(drag_info.create_grip(cx, {
                        let group_name = group_name.clone();
                        move |_: &ReorderableDragInfo, _, _, cx| {
                            cx.new(|_| SimpleDragPreview {
                                name: group_name.clone()
                            })
                        }
                    }))
                })
                .child(group_name.clone())
                .child(h_flex()
                    .child(Button::new((group_name.clone(), 0x23B40E19))
                        .ghost()
                        .small()
                        .icon(PandoraIcon::Menu))
                    .child(Button::new((group_name.clone(), 0x23B40E18))
                        .ghost()
                        .small()
                        .icon(if open { PandoraIcon::ChevronDown } else { PandoraIcon::ChevronLeft })
                        .on_click({
                            let group_name = group_name.clone();
                            move |_, _, cx| {
                                if open {
                                    InterfaceConfig::get_mut(cx).instance_groups_closed.insert(group_name.clone());
                                } else {
                                    InterfaceConfig::get_mut(cx).instance_groups_closed.remove(&group_name);
                                }
                            }
                        })))
            )
            .child(Separator::horizontal().pb_2())
            .child(AnimatedClippedElement::new(
                (group_name, 0x23B40E17).into(),
                open,
                move |amount| {
                    ResponsiveGrid::new(size).size_full().gap_4().children(children).opacity(amount).into_any_element()
                }
            ))
            .into_any_element()
    }

    fn render_card(&self, item: &InstanceEntry, cx: &mut App) -> Div {
        let index = item.id.index;
        let loader_and_version = format!(
            "{} {}",
            item.configuration.loader.pretty_name(),
            item.configuration.minecraft_version.as_str(),
        );

        let icon = if let Some(icon) = item.icon.clone() {
            let transform = png_render_cache::ImageTransformation::Resize { width: 64, height: 64 };
            png_render_cache::render_with_transform(icon, transform, cx)
                .rounded(cx.theme().radius).size_16().min_w_16().min_h_16().into_any_element()
        } else {
            let icon_path = item.configuration.instance_fallback_icon
                .map(|s| s.as_str())
                .unwrap_or("icons/box.svg");
            Icon::default().path(icon_path).size_16().min_w_16().min_h_16().into_any_element()
        };

        let play_button = render_play_button(item, index, self.data.clone());

        let menu = Button::new(("menu", index))
            .ghost()
            .small()
            .icon(PandoraIcon::Menu)
            .dropdown_menu_with_anchor(Anchor::TopRight, {
                let instance = item.clone();
                let data = self.data.clone();
                move |this, _window, _cx| {
                    this
                        .item(PopupMenuItem::new("Move to Group").on_click({
                            let instance_id = instance.id;
                            let instance_name = instance.name.clone();
                            let instances = data.instances.clone();
                            let backend_handle = data.backend_handle.clone();
                            move |_, window, cx| {
                                crate::modals::move_instance_to_group::open_move_instance_to_group_modal(instance_id,
                                    instance_name.clone(), instances.clone(), backend_handle.clone(), window, cx);
                            }
                        }))
                }
            });

        let theme = cx.theme();
        v_flex()
            .flex_1()
            .p_2()
            .gap_2()
            .w_full()
            .min_w_64()
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius_lg)
            .child(div().absolute().right_2().top_2().child(menu))
            .child(h_flex()
                .w_full()
                .gap_2()
                .child(icon)
                .child(v_flex()
                    .truncate()
                    .w_full()
                    .child(item.name.clone())
                    .child(loader_and_version)
                    .pr_6()
                )
            ).child(h_flex()
                .gap_2()
                .child(play_button.flex_1().small())
                .child(Button::new(("view", index)).flex_1().small().info().label(t::instance::view()).on_click({
                    let name = item.name.clone();
                    move |_, window, cx| {
                        root::switch_page(ui::PageType::InstancePage { name: name.clone() },
                            &[ui::PageType::Instances], window, cx);
                    }
                })))

    }
}

impl TableDelegate for InstanceList {
    fn columns_count(&self, _cx: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _cx: &App) -> usize {
        self.items.len()
    }

    fn column(&self, col_ix: usize, _cx: &App) -> gpui_component::table::Column {
        self.columns[col_ix].clone()
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: gpui_component::table::ColumnSort,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) {
        if let Some(col) = self.columns.get_mut(col_ix) {
            match col.key.as_ref() {
                "name" => self.items.sort_by(|a, b| match sort {
                    ColumnSort::Descending => lexical_sort::natural_lexical_cmp(&a.name, &b.name).reverse(),
                    _ => lexical_sort::natural_lexical_cmp(&a.name, &b.name),
                }),
                "version" => self.items.sort_by(|a, b| match sort {
                    ColumnSort::Descending => lexical_sort::natural_lexical_cmp(&a.configuration.minecraft_version, &b.configuration.minecraft_version).reverse(),
                    _ => lexical_sort::natural_lexical_cmp(&a.configuration.minecraft_version, &b.configuration.minecraft_version),
                }),
                _ => {},
            }
        }
    }

    fn render_td(&mut self, row_ix: usize, col_ix: usize, _window: &mut Window, _cx: &mut Context<TableState<Self>>) -> impl IntoElement {
        let item = &self.items[row_ix];
        if let Some(col) = self.columns.get(col_ix) {
            match col.key.as_ref() {
                "name" => item.name.clone().into_any_element(),
                "version" => item.configuration.minecraft_version.as_str().into_any_element(),
                "controls" => {
                    let play_button = render_play_button(item, row_ix, self.data.clone());

                    h_flex()
                        .size_full()
                        .gap_2()
                        .border_r_4()
                        .child(play_button.w_1_2().small())
                        .child(Button::new("view").w_1_2().small().info().label(t::instance::view()).on_click({
                            let name = item.name.clone();
                            move |_, window, cx| {
                                root::switch_page(ui::PageType::InstancePage { name: name.clone() },
                                    &[ui::PageType::Instances], window, cx);
                            }
                        }))
                        .into_any_element()
                },
                "loader" => item.configuration.loader.pretty_name().into_any_element(),
                _ => t::common::unknown().into_any_element(),
            }
        } else {
            t::common::unknown().into_any_element()
        }
    }
}

fn render_play_button(item: &InstanceEntry, index: usize, data: DataEntities) -> Button {
    let name = item.name.clone();
    let id = item.id;
    match item.status {
        InstanceStatus::NotRunning => {
            Button::new(("start_instance", index))
                .success()
                .label(t::instance::start::label())
                .on_click(
                move |_, window, cx| {
                    root::start_instance(id, name.clone(), None, &data, window, cx);
                },
            )
        },
        InstanceStatus::Launching => {
            Button::new(("launching", index))
                .warning()
                .label("...")
        },
        InstanceStatus::Stopping => {
            Button::new(("launching", index))
                .danger()
                .label("...")
        },
        InstanceStatus::Running => {
            Button::new(("kill_instance", index))
                .danger()
                .label(t::instance::kill())
                .on_click({
                    let backend_handle = data.backend_handle.clone();
                    move |_, _, _| {
                        backend_handle.send(MessageToBackend::KillInstance { id });
                    }
                })
        },
    }
}
