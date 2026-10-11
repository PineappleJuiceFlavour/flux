use gpui::{CursorStyle, DragMoveEvent, Hitbox, LayoutId, Point, prelude::*};
use gpui::{AnyElement, App, AppContext, Bounds, Element, Entity, InteractiveElement, Interactivity, IntoElement, ParentElement, Pixels, Render, SharedString, StyleRefinement, Styled, Window, div, px};
use gpui_component::{ActiveTheme, h_flex};

use crate::icon::PandoraIcon;

pub struct ReorderableState {
    dragging_from_index: Option<usize>,
    dragging_to_index: Option<usize>,
    vertical_bounds: Vec<(Option<Pixels>, Pixels, Option<Pixels>)>,
    shift_offsets: Vec<Pixels>
}

impl ReorderableState {
    pub fn new(_window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|_| Self {
            dragging_from_index: None,
            dragging_to_index: None,
            vertical_bounds: Vec::new(),
            shift_offsets: Vec::new(),
        })
    }

    pub fn take_reorder(&mut self) -> Option<(usize, usize)> {
        let from = self.dragging_from_index.take()?;

        self.vertical_bounds.clear();
        self.shift_offsets.clear();

        let to = self.dragging_to_index.take()?;
        if from == to {
            return None;
        }
        Some((from, to))
    }
}

pub struct SimpleDragPreview {
    pub name: SharedString,
}

impl Render for SimpleDragPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .child(h_flex()
                .left(px(-9.0))
                .top(px(-9.0))
                .p_2()
                .gap_2()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .text_color(cx.theme().popover_foreground)
                .shadow_lg()
                .child(div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size_6()
                    .min_w_6()
                    .child(PandoraIcon::GripVertical))
                .child(self.name.clone()))
    }
}

#[derive(Clone)]
pub struct ReorderableDragInfo {
    index: usize,
    state: Entity<ReorderableState>,
}

impl ReorderableDragInfo {
    pub fn create_grip<W: 'static + Render>(
        self,
        cx: &mut App,
        constructor: impl Fn(&Self, Point<Pixels>, &mut Window, &mut App) -> Entity<W> + 'static
    ) -> impl IntoElement {
        div()
            .id(("drag", self.index))
            .flex()
            .items_center()
            .justify_center()
            .size_6()
            .min_w_6()
            .text_color(cx.theme().muted_foreground)
            .hover(|style| style.text_color(cx.theme().foreground))
            .child(PandoraIcon::GripVertical)
            .cursor_grab()
            .on_drag(self, constructor)
    }
}

type RenderItemFn = dyn FnMut(usize, ReorderableDragInfo, &mut Window, &mut App) -> AnyElement + 'static;

pub struct Reorderable {
    state: Entity<ReorderableState>,
    interactivity: Interactivity,
    count: usize,
    render_item: Box<RenderItemFn>,
}

impl Reorderable {
    pub fn new(state: &Entity<ReorderableState>, count: usize, _cx: &mut App, render_item: impl FnMut(usize, ReorderableDragInfo, &mut Window, &mut App) -> AnyElement + 'static) -> Self {
        let mut interactivity = Interactivity::default();

        interactivity.on_drag_move(move |event: &DragMoveEvent<ReorderableDragInfo>, window, cx| {
            let info = event.drag(cx).clone();

            if cx.active_drag_cursor_style() != Some(CursorStyle::ClosedHand) {
                cx.set_active_drag_cursor_style(CursorStyle::ClosedHand, window);
            }

            info.state.update(cx, |state, _| {
                state.dragging_from_index = Some(info.index);
                state.dragging_to_index = None;
                let mouse_y = event.event.position.y;

                for (index, (top, mid, bottom)) in state.vertical_bounds.iter().enumerate() {
                    if let Some(top) = top && mouse_y < *top {
                        continue;
                    }
                    if let Some(bottom) = bottom && mouse_y > *bottom {
                        continue;
                    }

                    if index == info.index {
                        state.dragging_to_index = None;
                    } else if mouse_y > *mid {
                        if index > info.index {
                            state.dragging_to_index = Some(index);
                        } else {
                            state.dragging_to_index = Some((index+1).min(state.vertical_bounds.len().saturating_sub(1)));
                        }
                    } else {
                        if index > info.index {
                            state.dragging_to_index = Some(index.saturating_sub(1));
                        } else {
                            state.dragging_to_index = Some(index);
                        }
                    }
                    break;
                }
            });
        });

        Self {
            state: state.clone(),
            interactivity,
            count,
            render_item: Box::new(render_item)
        }
    }
}

impl Styled for Reorderable {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl InteractiveElement for Reorderable {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

impl IntoElement for Reorderable {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Reorderable {
    type RequestLayoutState = Vec<(AnyElement, LayoutId)>;
    type PrepaintState = (Vec<AnyElement>, Hitbox);

    fn id(&self) -> Option<gpui::ElementId> {
        self.interactivity.element_id.clone()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.interactivity.source_location()
    }

    fn request_layout(
        &mut self,
        global_id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        let mut elements = Vec::with_capacity(self.count);

        let layout_id = self.interactivity.request_layout(
            global_id,
            inspector_id,
            window,
            cx,
            |style, window, cx| {
                let mut children_layout_ids = Vec::with_capacity(self.count);
                for item_index in 0 .. self.count {
                    let info = ReorderableDragInfo {
                        index: item_index,
                        state: self.state.clone()
                    };
                    let mut rendered = (self.render_item)(item_index, info, window, cx);
                    let layout_id = rendered.request_layout(window, cx);

                    children_layout_ids.push(layout_id);
                    elements.push((rendered, layout_id));
                }

                window.request_layout(style, children_layout_ids, cx)
            },
        );

        (layout_id, elements)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        elements: &mut Self::RequestLayoutState,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> Self::PrepaintState {
        let mut elements_to_paint = Vec::with_capacity(elements.len());
        self.interactivity.prepaint(
            global_id,
            inspector_id,
            bounds,
            bounds.size,
            window,
            cx,
            |style, _scroll_offset, _hitbox, window, cx| {
                if self.count == 0 {
                    return;
                }

                let rem_size = window.rem_size();
                let font_size = window.text_style().font_size;
                let gap_height = style.gap.height.to_pixels(font_size, rem_size);
                let padding = style.padding.to_pixels(bounds.size.into(), rem_size);

                let mut origin = bounds.origin.clone() + gpui::point(padding.left.clone(), padding.top.clone());

                self.state.update(cx, |state, cx| {
                    state.vertical_bounds.clear();
                    if let Some(dragging_from_index) = state.dragging_from_index {
                        let dragging_to_index = state.dragging_to_index.unwrap_or(dragging_from_index);
                        let from_height = window.layout_bounds(elements[dragging_from_index].1).size.height + gap_height;

                        for (index, (mut element, layout_id)) in elements.drain(..).enumerate() {
                            let layout_bounds = window.layout_bounds(layout_id);
                            let size = layout_bounds.size;
                            let old_origin = layout_bounds.origin - window.pixel_snap_point(window.element_offset());

                            if dragging_from_index != index {
                                let mut desired_shift = Pixels::ZERO;
                                if index > dragging_from_index {
                                    if index <= dragging_to_index {
                                        desired_shift -= from_height;
                                    }
                                } else {
                                    if index >= dragging_to_index {
                                        desired_shift += from_height;
                                    }
                                };

                                let previous_shift = state.shift_offsets.get(index).copied().unwrap_or(px(0.0));

                                let shift_amount_delta = previous_shift - desired_shift;
                                let new_shift = if shift_amount_delta.abs() < px(1.0) {
                                    desired_shift
                                } else {
                                    previous_shift*0.5 + desired_shift*0.5
                                };

                                if previous_shift != new_shift {
                                    window.request_animation_frame();
                                    if state.shift_offsets.len() < index+1 {
                                        state.shift_offsets.resize(index+1, px(0.0));
                                    }
                                    state.shift_offsets[index] = new_shift;
                                }

                                let mut render_origin = origin.clone();
                                render_origin.y += new_shift;

                                element.prepaint_at(render_origin - old_origin, window, cx);
                                elements_to_paint.push(element);
                            }

                            let drag_bounds_top = if index == 0 {
                                None
                            } else {
                                Some(origin.y - gap_height/2.0)
                            };
                            let drag_bounds_bottom = if index == self.count-1 {
                                None
                            } else {
                                Some(origin.y + size.height + gap_height/2.0)
                            };
                            let drag_bounds_mid = origin.y + size.height/2.0;
                            state.vertical_bounds.push((drag_bounds_top, drag_bounds_mid, drag_bounds_bottom));
                            origin.y += size.height + gap_height;
                        }
                    } else {
                        for (mut element, layout_id) in elements.drain(..) {
                            let layout_bounds = window.layout_bounds(layout_id);
                            let size = layout_bounds.size;
                            let old_origin = layout_bounds.origin - window.pixel_snap_point(window.element_offset());
                            element.prepaint_at(origin - old_origin, window, cx);
                            elements_to_paint.push(element);
                            origin.y += size.height + gap_height;
                        }
                    }
                });
            },
        );

        // Dummy hitbox so drag mouse listener gets added
        let hitbox = window.insert_hitbox(Bounds::default(), gpui::HitboxBehavior::Normal);
        (elements_to_paint, hitbox)
    }

    fn paint(
        &mut self,
        global_id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint_state: &mut Self::PrepaintState,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        self.interactivity.paint(
            global_id,
            inspector_id,
            bounds,
            Some(&prepaint_state.1),
            window,
            cx,
            |_style, window, cx| {
                for element in prepaint_state.0.iter_mut() {
                    element.paint(window, cx);
                }
            },
        )
    }
}
