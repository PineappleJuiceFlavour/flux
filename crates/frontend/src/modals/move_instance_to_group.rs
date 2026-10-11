use bridge::{handle::BackendHandle, instance::InstanceID, message::MessageToBackend};
use gpui::{prelude::*, *};
use gpui_component::{WindowExt, button::{Button, ButtonVariants}, dialog::Dialog, h_flex, input::{Input, InputState}, v_flex};
use rustc_hash::FxHashSet;

use crate::{entity::instance::InstanceEntries, interface_config::InterfaceConfig};

struct MoveInstanceToGroupModalState {
    instance_id: InstanceID,
    title: SharedString,
    backend_handle: BackendHandle,
    existing_groups: Vec<SharedString>,
    new_group_name_input_state: Entity<InputState>,
}

impl MoveInstanceToGroupModalState {
    pub fn new(
        instance_id: InstanceID,
        instance_name: SharedString,
        instances: Entity<InstanceEntries>,
        backend_handle: BackendHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let new_group_name_input_state = cx.new(|cx| {
            InputState::new(window, cx).placeholder("New Group Name")
        });

        let title = format!("Move \"{}\" to Group", instance_name);

        let existing_groups = instances.read(cx).entries.values()
            .filter_map(|entry| entry.read(cx).configuration.group.clone())
            .collect::<FxHashSet<_>>();

        let mut existing_groups = existing_groups.into_iter().map(SharedString::from).collect::<Vec<_>>();
        let ordering = &InterfaceConfig::get(cx).instance_group_order;
        existing_groups.sort_by_cached_key(|group_name| {
            ordering.iter().position(|ordering_name| &**ordering_name == group_name.as_str()).map(|v| v+1).unwrap_or(0)
        });

        Self {
            instance_id,
            title: title.into(),
            backend_handle,
            existing_groups,
            new_group_name_input_state,
        }
    }

    pub fn render(&mut self, dialog: Dialog, _window: &mut Window, cx: &mut Context<Self>) -> Dialog {
        let content = v_flex()
            .gap_2()
            .children(self.existing_groups.iter().map(|group| {
                let group = group.clone();
                Button::new(group.clone()).label(group.clone()).success().on_click(cx.listener(move |this, _, window, cx| {
                    this.backend_handle.send(MessageToBackend::MoveInstanceToGroup {
                        instance_id: this.instance_id,
                        group: group.clone().into(),
                    });
                    window.close_dialog(cx);
                }))
            }))
            .child(Button::new("no-group").label("No Group").success().on_click(cx.listener(|this, _, window, cx| {
                this.backend_handle.send(MessageToBackend::MoveInstanceToGroup {
                    instance_id: this.instance_id,
                    group: "".into(),
                });
                window.close_dialog(cx);
            })))
            .child(h_flex()
                .gap_2()
                .child(Input::new(&self.new_group_name_input_state))
                .child(Button::new("create-new").label("Add to New Group").success().on_click(cx.listener(|this, _, window, cx| {
                    let name = this.new_group_name_input_state.read(cx).value();
                    this.backend_handle.send(MessageToBackend::MoveInstanceToGroup {
                        instance_id: this.instance_id,
                        group: name.into(),
                    });
                    window.close_dialog(cx);
                }))));

        dialog
            .overlay_closable(true)
            .title(self.title.clone())
            .child(content)
    }
}

pub fn open_move_instance_to_group_modal(
    instance_id: InstanceID,
    instance_name: SharedString,
    instances: Entity<InstanceEntries>,
    backend_handle: BackendHandle,
    window: &mut Window,
    cx: &mut App,
) {
    let state = cx.new(|cx| {
        MoveInstanceToGroupModalState::new(instance_id, instance_name, instances, backend_handle, window, cx)
    });

    window.open_dialog(cx, move |modal, window, cx| {
        cx.update_entity(&state, |state, cx| {
            state.render(modal, window, cx)
        })
    });
}
