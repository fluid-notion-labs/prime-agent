//! Top thin bar: connection state indicator and daemon socket path.

use gpui::prelude::*;
use gpui::{div, px, relative, rgb, Context, Entity, Render, Window};

use crate::app_state::{AppState, ConnectionStatus};

/// Palette: dark surfaces and one accent per connection state.
const BAR_BG: u32 = 0x16_16_1b;
const TEXT_DIM: u32 = 0x9a_9a_a5;
const GREEN: u32 = 0x3f_bf_6f;
const AMBER: u32 = 0xd9_a2_4a;
const GRAY: u32 = 0x5a_5a_65;

/// Dot + label color for the current connection state.
fn status_color(status: &ConnectionStatus) -> u32 {
    match status {
        ConnectionStatus::Connected { .. } => GREEN,
        ConnectionStatus::Connecting => AMBER,
        ConnectionStatus::Disconnected => GRAY,
    }
}

/// Human label for the current connection state.
fn status_label(status: &ConnectionStatus) -> &'static str {
    match status {
        ConnectionStatus::Connected { .. } => "Connected",
        ConnectionStatus::Connecting => "Connecting",
        ConnectionStatus::Disconnected => "Disconnected",
    }
}

/// Thin top bar rendering the connection state and the socket path.
pub struct StatusBar {
    state: Entity<AppState>,
}

impl StatusBar {
    /// New bar reading the shared state entity.
    #[must_use]
    pub fn new(state: Entity<AppState>) -> Self {
        Self { state }
    }
}

impl Render for StatusBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let status = state.status().clone();
        let socket_path = state.socket_path().to_owned();
        let color = status_color(&status);

        div()
            .h(px(28.0))
            .w(relative(1.0))
            .flex()
            .items_center()
            .justify_between()
            .px_3()
            .bg(rgb(BAR_BG))
            .text_size(px(12.0))
            .text_color(rgb(TEXT_DIM))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().size(px(8.0)).rounded_full().bg(rgb(color)))
                    .child(status_label(&status)),
            )
            .child(div().truncate().child(format!("socket: {socket_path}")))
    }
}
