//! Shared client state: connection status, transcript, composer draft.
//!
//! No transport lives here — the daemon client (separate change) plugs into
//! these fields and the [`MessageSink`] seam.

/// Wire status of the daemon connection.
///
/// `Connecting`/`Connected` are constructed only by the daemon client (dead
/// here by design — no transport lives in the skeleton).
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ConnectionStatus {
    /// No daemon socket; nothing is being attempted.
    Disconnected,
    /// Handshake in flight.
    Connecting,
    /// Established; carries the session id the daemon assigned.
    Connected { session: String },
}

/// One row in the transcript.
///
/// `Assistant`/`Tool` arrive only from the daemon client (dead here by design
/// — no transport lives in the skeleton).
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum TranscriptRow {
    /// A message typed by the user.
    User { text: String },
    /// A reply produced by the agent.
    Assistant { text: String },
    /// A tool call or result, labeled with the tool name.
    Tool { name: String, text: String },
}

/// Sink for outgoing messages — the seam the composer sends through.
///
/// The daemon client provides the real implementation; until it lands,
/// [`NoopSink`] keeps Enter harmless.
pub trait MessageSink {
    /// Hand `text` to the transport layer.
    fn send_message(&mut self, text: &str);
}

/// Stand-in sink until the daemon client wires the real transport.
#[derive(Debug, Default)]
pub struct NoopSink;

impl MessageSink for NoopSink {
    fn send_message(&mut self, _text: &str) {}
}

/// Placeholder until the daemon client seeds the real endpoint; matches the
/// daemon's unix `socket_dir` default (`pa_daemon::platform::paths`).
pub const DEFAULT_SOCKET_PATH: &str = "/tmp/prime-agent-1/daemon.sock";

/// Everything the UI reads and writes; the root view owns it as an entity.
#[derive(Debug)]
pub struct AppState {
    status: ConnectionStatus,
    socket_path: String,
    transcript: Vec<TranscriptRow>,
    draft: String,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    /// Fresh state: disconnected, empty transcript and draft.
    #[must_use]
    pub fn new() -> Self {
        Self {
            status: ConnectionStatus::Disconnected,
            socket_path: DEFAULT_SOCKET_PATH.to_owned(),
            transcript: Vec::new(),
            draft: String::new(),
        }
    }

    /// Current connection status.
    #[must_use]
    pub fn status(&self) -> &ConnectionStatus {
        &self.status
    }

    /// Move to a new connection status.
    // dead until the daemon client starts driving transitions.
    #[allow(dead_code)]
    pub fn set_status(&mut self, status: ConnectionStatus) {
        self.status = status;
    }

    /// Daemon endpoint shown in the status bar.
    #[must_use]
    pub fn socket_path(&self) -> &str {
        &self.socket_path
    }

    /// Replace the displayed socket path.
    // dead until the daemon client seeds the endpoint.
    #[allow(dead_code)]
    pub fn set_socket_path(&mut self, path: String) {
        self.socket_path = path;
    }

    /// Transcript rows, oldest first.
    #[must_use]
    pub fn transcript(&self) -> &[TranscriptRow] {
        &self.transcript
    }

    /// Append a rendered row to the transcript.
    pub fn push_row(&mut self, row: TranscriptRow) {
        self.transcript.push(row);
    }

    /// Current composer draft.
    #[must_use]
    pub fn draft(&self) -> &str {
        &self.draft
    }

    /// Type one character into the draft.
    pub fn draft_push(&mut self, ch: char) {
        self.draft.push(ch);
    }

    /// Backspace: drop the last character; false when the draft was empty.
    pub fn draft_backspace(&mut self) -> bool {
        self.draft.pop().is_some()
    }

    /// Take the draft for sending, leaving it empty.
    pub fn take_draft(&mut self) -> String {
        std::mem::take(&mut self.draft)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_transitions() {
        let mut state = AppState::new();
        assert_eq!(state.status(), &ConnectionStatus::Disconnected);

        state.set_status(ConnectionStatus::Connecting);
        assert_eq!(state.status(), &ConnectionStatus::Connecting);

        state.set_status(ConnectionStatus::Connected {
            session: "sess-1".to_owned(),
        });
        assert_eq!(
            state.status(),
            &ConnectionStatus::Connected {
                session: "sess-1".to_owned()
            }
        );
    }

    #[test]
    fn draft_editing_and_take() {
        let mut state = AppState::new();
        assert!(!state.draft_backspace());

        state.draft_push('h');
        state.draft_push('i');
        assert_eq!(state.draft(), "hi");

        assert!(state.draft_backspace());
        assert_eq!(state.draft(), "h");

        assert_eq!(state.take_draft(), "h");
        assert_eq!(state.take_draft(), "");
    }

    #[test]
    fn transcript_push() {
        let mut state = AppState::new();
        state.push_row(TranscriptRow::User {
            text: "hi".to_owned(),
        });
        state.push_row(TranscriptRow::Assistant {
            text: "hello".to_owned(),
        });
        state.push_row(TranscriptRow::Tool {
            name: "bash".to_owned(),
            text: "ran once".to_owned(),
        });
        assert_eq!(state.transcript().len(), 3);
    }

    #[test]
    fn noop_sink_is_harmless() {
        let mut sink = NoopSink;
        sink.send_message("anything");
    }
}
