//! Scrolling list of transcript rows rendered from the shared state.

use gpui::prelude::*;
use gpui::{div, px, relative, rgb, AnyElement, Context, Entity, Render, Window};

use crate::app_state::{AppState, TranscriptRow};

/// Palette: dark surfaces, muted labels, per-variant text tones.
const BG: u32 = 0x10_10_14;
const LABEL: u32 = 0x7c_7c_88;
const TEXT: u32 = 0xd8_d8_df;
const TOOL_TEXT: u32 = 0xa8_c4_e0;

/// One transcript row as a styled line: label plus body.
fn row_element(index: usize, row: &TranscriptRow) -> AnyElement {
    let (label, text, body_color) = match row {
        TranscriptRow::User { text } => ("you".to_owned(), text.clone(), TEXT),
        TranscriptRow::Assistant { text } => ("assistant".to_owned(), text.clone(), TEXT),
        TranscriptRow::Tool { name, text } => (name.clone(), text.clone(), TOOL_TEXT),
    };
    div()
        .id(format!("row-{index}"))
        .flex()
        .gap_2()
        .text_size(px(13.0))
        .text_color(rgb(body_color))
        .child(
            div()
                .w(px(72.0))
                .flex_shrink_0()
                .text_color(rgb(LABEL))
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(text))
        .into_any_element()
}

/// Scrolling transcript: all rows from app state, oldest first.
pub struct TranscriptView {
    state: Entity<AppState>,
}

impl TranscriptView {
    /// New transcript view reading the shared state entity.
    #[must_use]
    pub fn new(state: Entity<AppState>) -> Self {
        Self { state }
    }
}

impl Render for TranscriptView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.state.read(cx).transcript().to_vec();

        div()
            .id("transcript")
            .flex_1()
            .w(relative(1.0))
            .overflow_y_scroll()
            .overflow_x_hidden()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgb(BG))
            .children(rows.is_empty().then(|| {
                div()
                    .text_size(px(13.0))
                    .text_color(rgb(LABEL))
                    .child("No messages yet")
            }))
            .children(
                rows.iter()
                    .enumerate()
                    .map(|(index, row)| row_element(index, row)),
            )
    }
}
