//! pa-merida: GPUI desktop client for prime-agent (skeleton).
//!
//! One window, four regions composed from the shared [`AppState`]: status
//! bar, transcript view, composer, settings menu. The daemon transport
//! lands in a separate change and plugs into app state + `MessageSink`.

mod app_state;
mod composer;
mod settings_menu;
mod status_bar;
mod transcript_view;

use gpui::prelude::*;
use gpui::{
    div, px, rgb, size, App, Bounds, Entity, Focusable, Point, Render, Window, WindowBounds,
    WindowOptions,
};
use gpui_platform::application;

use crate::app_state::{AppState, NoopSink};
use crate::composer::Composer;
use crate::settings_menu::SettingsMenu;
use crate::status_bar::StatusBar;
use crate::transcript_view::TranscriptView;

/// Palette: the root surface behind all regions.
const BG: u32 = 0x10_10_14;
const TEXT: u32 = 0xd8_d8_df;

/// Root view: owns the shared state entity and composes the four regions.
struct MeridaApp {
    status_bar: Entity<StatusBar>,
    transcript: Entity<TranscriptView>,
    composer: Entity<Composer>,
    settings: Entity<SettingsMenu>,
}

impl Render for MeridaApp {
    fn render(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_start()
                    .child(self.status_bar.clone())
                    .child(self.settings.clone()),
            )
            .child(self.transcript.clone())
            .child(self.composer.clone())
    }
}

fn main() {
    application().run(|cx: &mut App| {
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(900.0), px(640.0)),
        };
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                let state = cx.new(|_| AppState::new());
                let status_bar = cx.new(|_| StatusBar::new(state.clone()));
                let transcript = cx.new(|_| TranscriptView::new(state.clone()));
                let composer = cx.new(|cx| Composer::new(state.clone(), Box::new(NoopSink), cx));
                let settings = cx.new(|_| SettingsMenu::new());
                window.focus(&composer.focus_handle(cx), cx);
                cx.new(|_| MeridaApp {
                    status_bar,
                    transcript,
                    composer,
                    settings,
                })
            },
        )
        .expect("pa-merida: failed to open window");
        cx.activate(true);
    });
}
