//! Bottom message box: draft editing and the send seam.

use gpui::prelude::*;
use gpui::{
    div, px, rgb, App, Context, Entity, FocusHandle, Focusable, KeyDownEvent, Render, Window,
};

use crate::app_state::{AppState, MessageSink, TranscriptRow};

/// Palette: dark surfaces, dim placeholder.
const BG: u32 = 0x16_16_1b;
const FIELD_BG: u32 = 0x1e_1e_24;
const TEXT: u32 = 0xd8_d8_df;
const PLACEHOLDER: u32 = 0x6a_6a_75;

/// Bottom message box: edits the shared draft and sends on Enter.
pub struct Composer {
    state: Entity<AppState>,
    /// Outgoing-message seam; the daemon client replaces `NoopSink`.
    sink: Box<dyn MessageSink>,
    focus_handle: FocusHandle,
}

impl Composer {
    /// New composer over the shared state, sending through `sink`.
    pub fn new(
        state: Entity<AppState>,
        sink: Box<dyn MessageSink>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            state,
            sink,
            focus_handle: cx.focus_handle(),
        }
    }

    /// Enter: send the draft through the seam, echo it into the transcript,
    /// and clear the field.
    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;

        let plain = !keystroke.modifiers.modified();
        if keystroke.key == "enter" && plain {
            let text = self.state.update(cx, |state, _| state.take_draft());
            if text.is_empty() {
                return;
            }
            self.sink.send_message(&text);
            self.state.update(cx, |state, _| {
                state.push_row(TranscriptRow::User { text: text.clone() });
            });
            cx.notify();
            return;
        }

        if keystroke.key == "backspace" {
            self.state.update(cx, |state, _| state.draft_backspace());
            cx.notify();
            return;
        }

        if plain {
            if let Some(typed) = keystroke.key_char.as_ref() {
                self.state.update(cx, |state, _| {
                    typed.chars().for_each(|ch| state.draft_push(ch));
                });
                cx.notify();
            }
        }
    }
}

impl Focusable for Composer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Composer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let draft = self.state.read(cx).draft().to_owned();
        let empty = draft.is_empty();

        div().w_full().px_3().py_2().bg(rgb(BG)).child(
            div()
                .id("composer-field")
                .key_context("Composer")
                .track_focus(&self.focus_handle)
                .on_key_down(cx.listener(Self::on_key_down))
                .h(px(40.0))
                .w_full()
                .flex()
                .items_center()
                .px_3()
                .rounded_md()
                .bg(rgb(FIELD_BG))
                .text_size(px(13.0))
                .text_color(rgb(TEXT))
                .when(empty, |field| {
                    field
                        .text_color(rgb(PLACEHOLDER))
                        .child("Message prime-agent…")
                })
                .when(!empty, |field| field.child(draft)),
        )
    }
}
