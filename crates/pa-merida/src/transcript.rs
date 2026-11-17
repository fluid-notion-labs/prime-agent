//! The transcript state: a pure model over the session wire. The window
//! renders this; the daemon client feeds it. No transport, no rendering.

use serde_json::Value;

/// One row in the visible transcript.
#[derive(Debug, Clone, PartialEq)]
pub enum TranscriptRow {
    /// A user prompt.
    User { text: String },
    /// Assistant output, split into text and thinking blocks in wire order.
    Assistant {
        text: String,
        thinking: String,
        has_tool_calls: bool,
    },
    /// A pending tool call (its result has not arrived yet).
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    /// A finished tool call.
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        text: String,
        is_error: bool,
    },
}

impl TranscriptRow {
    pub fn key(&self) -> String {
        match self {
            Self::User { text } => format!("user:{text}"),
            Self::Assistant {
                text,
                thinking,
                has_tool_calls,
            } => {
                format!("assistant:{text}|{thinking}|{has_tool_calls}")
            }
            Self::ToolCall { id, .. } => format!("tool-call:{id}"),
            Self::ToolResult { tool_call_id, .. } => format!("tool-result:{tool_call_id}"),
        }
    }
}

/// The whole transcript, snapshot-filled then event-updated.
#[derive(Debug, Clone, Default)]
pub struct TranscriptModel {
    rows: Vec<TranscriptRow>,
    /// Set when a session replaced or resynced and the rows were rebuilt.
    generation: u64,
}

impl TranscriptModel {
    pub fn rows(&self) -> &[TranscriptRow] {
        &self.rows
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Replace the rows with a snapshot's message list (`messages` from a
    /// `session_snapshot_chunk`, or the `messages` array of a resync).
    pub fn fill_snapshot(&mut self, messages: &[Value]) {
        self.rows = messages.iter().filter_map(message_row).collect();
        self.generation += 1;
    }

    /// Apply one live session event; ignores shapes the model does not
    /// render (streaming deltas not needed yet, roster pings, and so on).
    pub fn apply_event(&mut self, event: &Value) {
        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match kind {
            "message_start" | "message" => {
                if let Some(row) =
                    message_row(&event.get("message").cloned().unwrap_or(Value::Null))
                {
                    let key = row.key();
                    if !self.rows.iter().any(|existing| existing.key() == key) {
                        self.rows.push(row);
                    }
                }
            }
            "message_delta" | "text_delta" => {
                // Streaming text: fold into the trailing assistant row.
                if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                    if let Some(TranscriptRow::Assistant { text, .. }) = self.rows.last_mut() {
                        text.push_str(delta);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Turn one wire message (`UserMessage`/`AssistantMessage`/…, either a
/// snapshot `AgentMessage` or an event's embedded message) into zero or
/// one transcript row. Unknown shapes map to nothing rather than erroring:
/// the transcript keeps flowing.
fn message_row(message: &Value) -> Option<TranscriptRow> {
    let role = message.get("role").and_then(Value::as_str)?;
    match role {
        "user" => {
            let text = user_text(message.get("content")?);
            (!text.is_empty()).then_some(TranscriptRow::User { text })
        }
        "assistant" => {
            let mut text = String::new();
            let mut thinking = String::new();
            let mut has_tool_calls = false;
            if let Some(blocks) = message.get("content").and_then(Value::as_array) {
                for block in blocks {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(chunk) = block.get("text").and_then(Value::as_str) {
                                text.push_str(chunk);
                            }
                        }
                        Some("thinking") => {
                            if let Some(chunk) = block.get("thinking").and_then(Value::as_str) {
                                thinking.push_str(chunk);
                            }
                        }
                        Some("toolUse") | Some("tool_use") => has_tool_calls = true,
                        _ => {}
                    }
                }
            }
            Some(TranscriptRow::Assistant {
                text,
                thinking,
                has_tool_calls,
            })
        }
        "toolResult" => {
            let text = user_text(message.get("content")?);
            Some(TranscriptRow::ToolResult {
                tool_call_id: message
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                tool_name: message
                    .get("toolName")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                text,
                is_error: message
                    .get("isError")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        }
        _ => None,
    }
}

/// User content is a string or a block array; take the text parts.
fn user_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| {
                let kind = block.get("type").and_then(Value::as_str)?;
                if kind == "text" {
                    block.get("text").and_then(Value::as_str)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn snapshot_fill_builds_rows_in_wire_order() {
        let mut model = TranscriptModel::default();
        model.fill_snapshot(&[
            json!({ "role": "user", "content": "list the files", "timestamp": 1 }),
            json!({ "role": "assistant", "content": [
                { "type": "thinking", "thinking": "plan first" },
                { "type": "text", "text": "here are the files" },
                { "type": "toolUse", "id": "t1", "name": "ls", "input": {} }
            ]}),
            json!({ "role": "toolResult", "toolCallId": "t1", "toolName": "ls", "content": [{ "type": "text", "text": "a.rs b.rs" }], "isError": false, "timestamp": 2 }),
        ]);
        assert_eq!(model.rows().len(), 3, "rows: {:?}", model.rows());
        assert!(
            matches!(&model.rows()[0], TranscriptRow::User { text } if text == "list the files"),
            "rows: {:?}",
            model.rows()
        );
        assert!(
            matches!(&model.rows()[1], TranscriptRow::Assistant { text, thinking, has_tool_calls }
            if text == "here are the files" && thinking == "plan first" && *has_tool_calls)
        );
        assert!(
            matches!(&model.rows()[2], TranscriptRow::ToolResult { tool_call_id, tool_name, text, is_error }
            if tool_call_id == "t1" && tool_name == "ls" && text == "a.rs b.rs" && !*is_error)
        );
        assert_eq!(model.generation(), 1);
    }

    #[test]
    fn live_events_append_and_dedup() {
        let mut model = TranscriptModel::default();
        model.apply_event(&json!({
            "type": "message",
            "message": { "role": "user", "content": "hello" }
        }));
        // The same event id replayed must not duplicate the row.
        model.apply_event(&json!({
            "type": "message",
            "message": { "role": "user", "content": "hello" }
        }));
        assert_eq!(model.rows().len(), 1);
        // A streaming delta folds into the trailing assistant row.
        model.apply_event(&json!({ "type": "message", "message": { "role": "assistant", "content": [{ "type": "text", "text": "answer" }] } }));
        model.apply_event(&json!({ "type": "message_delta", "delta": " continued" }));
        assert!(
            matches!(&model.rows()[1], TranscriptRow::Assistant { text, .. } if text == "answer continued")
        );
    }

    #[test]
    fn resync_replaces_rows() {
        let mut model = TranscriptModel::default();
        model.apply_event(
            &json!({ "type": "message", "message": { "role": "user", "content": "old session" } }),
        );
        model.fill_snapshot(&[json!({ "role": "user", "content": "replaced" })]);
        assert_eq!(model.rows().len(), 1);
        assert!(matches!(&model.rows()[0], TranscriptRow::User { text } if text == "replaced"));
        assert_eq!(model.generation(), 1);
    }

    #[test]
    fn tool_result_snapshot_row() {
        let mut model = TranscriptModel::default();
        model.fill_snapshot(&[json!({ "role": "toolResult", "toolCallId": "t1", "toolName": "ls", "content": [{ "type": "text", "text": "a.rs b.rs" }], "isError": false, "timestamp": 2 })]);
        assert_eq!(model.rows().len(), 1, "toolResult rows: {:?}", model.rows());
        assert!(
            matches!(&model.rows()[0], TranscriptRow::ToolResult { tool_call_id, .. } if tool_call_id == "t1")
        );
    }

    #[test]
    fn user_content_blocks_join_and_unknown_events_are_ignored() {
        let mut model = TranscriptModel::default();
        model.apply_event(&json!({
            "type": "message",
            "message": { "role": "user", "content": [{ "type": "text", "text": "two " }, { "type": "image", "url": "x" }, { "type": "text", "text": "parts" }] }
        }));
        assert!(matches!(&model.rows()[0], TranscriptRow::User { text } if text == "two parts"));
        model.apply_event(&json!({ "type": "session_resynced" }));
        assert_eq!(model.rows().len(), 1);
    }
}
