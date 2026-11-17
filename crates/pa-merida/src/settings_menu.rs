//! Top-right hamburger button with a small inert dropdown.

use gpui::prelude::*;
use gpui::{div, px, rgb, ClickEvent, Context, Render, Stateful, Window};

/// Palette: dark surfaces and menu items.
const BG: u32 = 0x1e_1e_24;
const ITEM_HOVER: u32 = 0x2a_2a_33;
const TEXT: u32 = 0xd8_d8_df;
const ICON: u32 = 0x9a_9a_a5;

/// Menu entries, inert for now — real actions land with the daemon client.
const ITEMS: [&str; 3] = ["Settings", "Model", "About"];

/// Hamburger button opening a small dropdown with inert items.
pub struct SettingsMenu {
    open: bool,
}

impl SettingsMenu {
    /// New menu, closed.
    #[must_use]
    pub fn new() -> Self {
        Self { open: false }
    }

    /// One inert dropdown row; clicking only closes the menu.
    fn item(label: &'static str, cx: &mut Context<Self>) -> Stateful<gpui::Div> {
        div()
            .id(format!("menu-item-{label}"))
            .px_3()
            .py_1()
            .text_size(px(12.0))
            .text_color(rgb(TEXT))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(ITEM_HOVER)))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.open = false;
                cx.notify();
            }))
            .child(label)
    }

    /// The three-bar hamburger glyph.
    fn hamburger() -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap(px(3.0))
            .children((0..3).map(|_| div().w(px(14.0)).h(px(2.0)).rounded_full().bg(rgb(ICON))))
    }
}

impl Default for SettingsMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for SettingsMenu {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .child(
                div()
                    .id("menu-button")
                    .h(px(28.0))
                    .px_3()
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(BG)))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.open = !this.open;
                        cx.notify();
                    }))
                    .child(Self::hamburger()),
            )
            .when(self.open, |menu| {
                menu.child(
                    div()
                        .absolute()
                        .top(px(30.0))
                        .right(px(0.0))
                        .w(px(160.0))
                        .flex()
                        .flex_col()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(0x2a_2a_33))
                        .bg(rgb(BG))
                        .py_1()
                        .shadow_lg()
                        .children(ITEMS.map(|label| Self::item(label, cx))),
                )
            })
    }
}
