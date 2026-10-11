use gpui::*;

const RATE: f32 = 0.25;

pub struct AnimatedClippedElement {
    id: ElementId,
    open: bool,
    element: Option<Box<dyn FnOnce(f32) -> AnyElement>>,
}

impl AnimatedClippedElement {
    pub fn new(id: ElementId, open: bool, element: impl FnOnce(f32) -> AnyElement + 'static) -> Self {
        Self {
            id,
            open,
            element: Some(Box::new(element)),
        }
    }
}

impl IntoElement for AnimatedClippedElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[derive(Default, Clone)]
struct AnimatedClippedState {
    last_amount: f32,
    height: Pixels,
}

impl Element for AnimatedClippedElement {
    type RequestLayoutState = f32;
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<gpui::ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        let previous = window.with_element_state(global_id.unwrap(), |previous, _| {
            (previous.clone(), previous.unwrap_or(AnimatedClippedState {
                last_amount: if self.open { 1.0 } else { 0.0 },
                height: Pixels::ZERO,
            }))
        });

        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        let last_amount = if let Some(previous) = previous {
            style.size.height = previous.height.into();
            previous.last_amount
        } else {
            if self.open { 1.0 } else { 0.0 }
        };
        (window.request_layout(style, None, cx), last_amount)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        last_amount: &mut Self::RequestLayoutState,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> Self::PrepaintState {
        let available_space = Size::new(
            AvailableSpace::Definite(bounds.size.width),
            AvailableSpace::MinContent
        );
        let mut element = (self.element.take().unwrap())(*last_amount);
        let size = element.layout_as_root(available_space, window, cx);

        let should_render = window.with_element_state(global_id.unwrap(), |previous: Option<AnimatedClippedState>, window| {
            let mut previous = previous.unwrap();
            let mut force_animation_frame = false;

            let new_height = if cx.reduce_motion() {
                if self.open {
                    size.height
                } else {
                    Pixels::ZERO
                }
            } else {
                if self.open {
                    if previous.last_amount < 1.0 && size.height > previous.height + px(1.0) {
                        force_animation_frame = true;
                        let delta = size.height - previous.height;
                        previous.height + delta * RATE
                    } else {
                        size.height
                    }
                } else {
                    if previous.height > px(1.0) {
                        force_animation_frame = true;
                        previous.height * (1.0 - RATE)
                    } else {
                        Pixels::ZERO
                    }
                }
            };

            if force_animation_frame || previous.height != new_height {
                window.request_animation_frame();
            }
            previous.last_amount = (new_height / size.height).clamp(0.0, 1.0);
            previous.height = new_height;
            (new_height > Pixels::ZERO, previous)
        });

        if should_render {
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                element.prepaint_at(bounds.origin, window, cx);
            });
            Some(element)
        } else {
            None
        }
    }

    fn paint(
        &mut self,
        _global_id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        element: &mut Self::PrepaintState,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        if let Some(mut element) = element.take() {
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                element.paint(window, cx)
            });
        }
    }
}
