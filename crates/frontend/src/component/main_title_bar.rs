use bridge::handle::BackendHandle;
use gpui::*;
use gpui_component::{Sizable, button::{Button, ButtonVariants}};
use schema::pandora_update::UpdatePrompt;

use crate::{component::{generic_title_bar::TitleBar, page_path::PagePath}, icon::PandoraIcon};

#[derive(IntoElement)]
pub struct MainTitleBar {
    pub page_path: PagePath,
    pub controls: AnyElement,
    pub update: Option<UpdatePrompt>,
    pub send: BackendHandle,
}

impl RenderOnce for MainTitleBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        TitleBar {
            left_content: vec![
                div().overflow_hidden().pr_8().child(self.page_path).into_any_element(),
                self.controls
            ],
            right_content: if let Some(update) = self.update {
                vec![
                    Button::new("update")
                        .label(t::system::update::available())
                        .success()
                        .compact()
                        .small()
                        .ml_2()
                        .icon(PandoraIcon::Download)
                        .on_click({
                            let send = self.send.clone();
                            move |_, window, cx| {
                                crate::modals::update_prompt::open_update_prompt(update.clone(), send.clone(), window, cx);
                            }
                        })
                        .into_any_element()
                ]
            } else {
                vec![]
            },
            content_only: !crate::root::should_render_custom_titlebar(),
        }
    }
}
