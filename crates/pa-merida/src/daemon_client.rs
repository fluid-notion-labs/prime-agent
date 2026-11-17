//! Daemon client for pa-merida: connect, hello handshake, request/response
//! demux, and a live session-event stream. Built against the wire contract
//! in `pa-types` (`DaemonCommandEnvelope`, `DaemonResponse`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use pa_types::daemon::{
    DaemonCommand, DaemonCommandEnvelope, DaemonCommandFrameType, DaemonProtocolInfo,
    DaemonResponse, DAEMON_PROTOCOL_NAME, DAEMON_PROTOCOL_VERSION,
};
use serde_json::{Map, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex};

pub const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 30_000;
/// Turn-bounded requests (`prompt_and_wait`): a long turn must not
/// expire the request early.
pub const LONG_RUNNING_REQUEST_TIMEOUT_MS: u64 = 600_000;
const CONNECT_TIMEOUT_MS: u64 = 3_000;
/// The daemon can take a while to greet when loading a large session.
const HELLO_TIMEOUT_MS: u64 = 15_000;

/// One event frame from the daemon, demuxed for the app.
#[derive(Debug, Clone)]
pub enum DaemonEvent {
    /// A live session-plane event (`session_event`).
    SessionEvent {
        active_session_id: String,
        event: Value,
    },
    /// Snapshot chunk replayed on attach or resync.
    SnapshotChunk {
        active_session_id: String,
        snapshot_id: String,
        messages: Vec<Value>,
    },
    /// The daemon is shutting down (update restart, explicit close).
    DaemonClosing { reason: String },
    /// Any other frame, passed through for the app to ignore or render.
    Other(Value),
}

/// A live supervisor connection. Clone for concurrent use; requests are
/// serialized through the writer task and matched to responses by
/// envelope id.
#[derive(Clone)]
pub struct DaemonClient {
    socket_path: PathBuf,
    protocol: DaemonProtocolInfo,
    next_request_id: Arc<AtomicU64>,
    shared: Arc<Shared>,
    writer: mpsc::UnboundedSender<String>,
}

#[derive(Default)]
struct Shared {
    pending: Mutex<std::collections::HashMap<String, oneshot::Sender<Result<DaemonResponse>>>>,
}

impl Shared {
    async fn resolve(&self, id: &str, response: DaemonResponse) {
        if let Some(reply) = self.pending.lock().await.remove(id) {
            let _ = reply.send(Ok(response));
        }
    }

    async fn fail_all(&self, error: &str) {
        let mut pending = self.pending.lock().await;
        for (_, reply) in pending.drain() {
            let _ = reply.send(Err(anyhow!("{}", error)));
        }
    }
}

/// Pull typed fields out of one JSONL frame.
fn string_field(frame: &Value, key: &str) -> String {
    frame
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

impl DaemonClient {
    /// Connect to `socket_path`, wait for the daemon's `daemon_hello`, and
    /// return the client plus its event stream. The receiver must be polled
    /// or the reader stalls once the channel's buffer fills.
    ///
    /// # Errors
    ///
    /// Connect or hello timeout, transport failure, or unknown protocol.
    pub async fn connect(
        socket_path: &Path,
    ) -> Result<(Self, mpsc::UnboundedReceiver<DaemonEvent>)> {
        let connect = tokio::time::timeout(
            Duration::from_millis(CONNECT_TIMEOUT_MS),
            pa_types::platform::transport::connect_transport(socket_path),
        )
        .await
        .map_err(|_| {
            anyhow!(
                "Timed out after {CONNECT_TIMEOUT_MS}ms connecting to the Prime Agent daemon. Socket: {}.",
                socket_path.display()
            )
        })?
        .with_context(|| {
            format!(
                "Failed to connect to the Prime Agent daemon. Socket: {}.",
                socket_path.display()
            )
        })?;

        let (reader_half, writer_half) = connect.split();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (line_tx, mut line_rx) = mpsc::unbounded_channel::<String>();
        let (hello_tx, hello_rx) = oneshot::channel::<Result<Value>>();
        let shared: Arc<Shared> = Arc::new(Shared::default());

        let reader_shared = Arc::clone(&shared);
        let reader_task = tokio::spawn(async move {
            let mut reader = BufReader::new(reader_half);
            let mut line = String::new();
            let mut hello_reply = Some(hello_tx);
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        reader_shared
                            .fail_all("The daemon connection closed.")
                            .await;
                        break;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        reader_shared.fail_all(&error.to_string()).await;
                        break;
                    }
                }
                let frame: Value = match serde_json::from_str(line.trim()) {
                    Ok(frame) => frame,
                    Err(_) => continue,
                };
                match frame
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                {
                    "daemon_hello" => {
                        if let Some(reply) = hello_reply.take() {
                            let _ = reply.send(Ok(frame));
                        }
                    }
                    "session_event" => {
                        let _ = event_tx.send(DaemonEvent::SessionEvent {
                            active_session_id: string_field(&frame, "activeSessionId"),
                            event: frame.get("event").cloned().unwrap_or(Value::Null),
                        });
                    }
                    "session_snapshot_chunk" => {
                        let _ = event_tx.send(DaemonEvent::SnapshotChunk {
                            active_session_id: string_field(&frame, "activeSessionId"),
                            snapshot_id: string_field(&frame, "snapshotId"),
                            messages: frame
                                .get("messages")
                                .and_then(Value::as_array)
                                .cloned()
                                .unwrap_or_default(),
                        });
                    }
                    "daemon_closing" => {
                        let _ = event_tx.send(DaemonEvent::DaemonClosing {
                            reason: string_field(&frame, "reason"),
                        });
                    }
                    _ => {
                        // Response frames carry a string envelope id.
                        let id = frame.get("id").and_then(Value::as_str).map(str::to_owned);
                        if let Some(id) = id {
                            if let Ok(response) =
                                serde_json::from_value::<DaemonResponse>(frame.clone())
                            {
                                reader_shared.resolve(&id, response).await;
                                continue;
                            }
                        }
                        let _ = event_tx.send(DaemonEvent::Other(frame));
                    }
                }
            }
        });

        let writer_task = tokio::spawn(async move {
            let mut writer = writer_half;
            while let Some(line) = line_rx.recv().await {
                let mut payload = line.into_bytes();
                payload.push(b'\n');
                if writer.write_all(&payload).await.is_err() {
                    break;
                }
                if writer.flush().await.is_err() {
                    break;
                }
            }
        });
        // When the reader dies, stop the writer task.
        tokio::spawn(async move {
            let _ = reader_task.await;
            writer_task.abort();
        });

        let hello = tokio::time::timeout(Duration::from_millis(HELLO_TIMEOUT_MS), hello_rx)
            .await
            .map_err(|_| {
                anyhow!(
                    "Timed out waiting for the daemon's hello frame. Socket: {}.",
                    socket_path.display()
                )
            })?
            .map_err(|_| anyhow!("The reader task died before the daemon greeted."))??;
        let protocol: DaemonProtocolInfo =
            serde_json::from_value(hello.get("protocol").cloned().unwrap_or(Value::Null))
                .context("Daemon hello frame had no usable protocol block")?;
        if protocol.name != DAEMON_PROTOCOL_NAME {
            return Err(anyhow!(
                "Unknown daemon protocol: {} (expected {DAEMON_PROTOCOL_NAME}).",
                protocol.name
            ));
        }

        Ok((
            Self {
                socket_path: socket_path.to_owned(),
                protocol,
                next_request_id: Arc::new(AtomicU64::new(1)),
                shared,
                writer: line_tx,
            },
            event_rx,
        ))
    }

    pub fn protocol(&self) -> &DaemonProtocolInfo {
        &self.protocol
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Send one command and await its response.
    ///
    /// # Errors
    ///
    /// Timeout or transport failure. A daemon refusal is *not* an error —
    /// it arrives as a `success: false` response.
    pub async fn request(&self, command: DaemonCommand) -> Result<DaemonResponse> {
        self.request_with_timeout(command, Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS))
            .await
    }

    /// A turn-bounded request (`prompt_and_wait`): a long turn cannot
    /// expire it.
    pub async fn request_long(&self, command: DaemonCommand) -> Result<DaemonResponse> {
        self.request_with_timeout(
            command,
            Duration::from_millis(LONG_RUNNING_REQUEST_TIMEOUT_MS),
        )
        .await
    }

    async fn request_with_timeout(
        &self,
        command: DaemonCommand,
        timeout: Duration,
    ) -> Result<DaemonResponse> {
        let id = format!(
            "pa-merida-{}",
            self.next_request_id.fetch_add(1, Ordering::Relaxed)
        );
        let envelope = DaemonCommandEnvelope {
            frame_type: DaemonCommandFrameType::Command,
            id,
            protocol: self.protocol.clone(),
            client_id: None,
            command,
        };
        let line = serde_json::to_string(&envelope).context("Serialize command envelope")?;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.shared
            .pending
            .lock()
            .await
            .insert(envelope.id.clone(), reply_tx);
        self.writer
            .send(line)
            .map_err(|_| anyhow!("The daemon connection is closed."))?;

        match tokio::time::timeout(timeout, reply_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(anyhow!("The daemon connection closed before replying.")),
            Err(_) => {
                self.shared.pending.lock().await.remove(&envelope.id);
                Err(anyhow!(
                    "Timed out after {}ms waiting for a daemon response.",
                    timeout.as_millis()
                ))
            }
        }
    }

    /// Attach to a session by id.
    pub async fn attach(&self, active_session_id: &str) -> Result<DaemonResponse> {
        self.request(DaemonCommand::Attach {
            id: None,
            active_session_id: active_session_id.to_owned(),
            client_id: None,
            capabilities: None,
            resume_cursor: None,
            telemetry_disabled: None,
            recovery_config: None,
            env: None,
            launch_env: None,
            rest: Map::new(),
        })
        .await
    }

    /// List live sessions.
    pub async fn list(&self) -> Result<Value> {
        let response = self
            .request(DaemonCommand::List {
                id: None,
                all: None,
                cwd: None,
                session_dir: None,
                include_client_owned: None,
                rest: Map::new(),
            })
            .await?;
        response
            .data
            .ok_or_else(|| anyhow!("Daemon list response carried no data."))
    }

    /// Send a user prompt and wait for the turn to end.
    pub async fn send_prompt(&self, active_session_id: &str, text: &str) -> Result<DaemonResponse> {
        self.request_long(DaemonCommand::PromptAndWait {
            id: None,
            active_session_id: active_session_id.to_owned(),
            message: text.to_owned(),
            input: pa_types::daemon::PromptInput {
                content: None,
                images: None,
                streaming_behavior: None,
                queue_if_busy: None,
                expand_prompt_templates: None,
                source: None,
                agent_message_id: None,
                custom_message: None,
                queue_key: None,
                prefix_messages: None,
                admission_id: None,
                rlm_notice_nonce: None,
            },
            rest: Map::new(),
        })
        .await
    }
}

/// The protocol version the client speaks, for the fake daemon's tests.
pub const CLIENT_PROTOCOL_VERSION: u64 = DAEMON_PROTOCOL_VERSION;

/// A mock supervisor used by tests: serves JSONL frames from a channel.
#[cfg(test)]
pub(crate) mod fake_daemon {
    use super::*;

    /// Serve `frames` (plus a leading hello) on a fresh unix socket; echo
    /// every command envelope with a canned success response.
    pub async fn serve(dir: &std::path::Path, extra_frames: Vec<Value>) -> std::path::PathBuf {
        use tokio::net::UnixListener;
        let socket_path = dir.join("daemon.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind fake daemon");
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let (read_half, mut write_half) = stream.into_split();
            let hello = serde_json::json!({
                "type": "daemon_hello",
                "protocol": { "name": DAEMON_PROTOCOL_NAME, "version": DAEMON_PROTOCOL_VERSION }
            });
            let _ = write_half
                .write_all(format!("{}\n", hello).as_bytes())
                .await;
            for frame in extra_frames {
                let _ = write_half
                    .write_all(format!("{}\n", frame).as_bytes())
                    .await;
            }
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let envelope: Value = match serde_json::from_str(line.trim()) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                let id = envelope.get("id").cloned().unwrap_or(Value::Null);
                // DaemonCommand is internally tagged: the variant name is
                // the value at /command/type, not a bare key.
                let command = envelope
                    .pointer("/command/type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let reply = serde_json::json!({
                    "id": id,
                    "command": command,
                    "success": true,
                    "data": { "echoed": command }
                });
                let _ = write_half
                    .write_all(format!("{}\n", reply).as_bytes())
                    .await;
            }
        });
        socket_path
    }
}

#[cfg(test)]
mod tests {
    use super::fake_daemon::serve;
    use super::*;

    #[tokio::test]
    async fn connects_and_completes_hello_handshake() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = serve(dir.path(), vec![]).await;
        let (client, mut events) = DaemonClient::connect(&socket_path).await.unwrap();
        assert_eq!(client.protocol().name, DAEMON_PROTOCOL_NAME);
        assert!(events.try_recv().is_err(), "no events before attach");
    }

    #[tokio::test]
    async fn request_gets_response_matched_by_envelope_id() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = serve(dir.path(), vec![]).await;
        let (client, _events) = DaemonClient::connect(&socket_path).await.unwrap();
        let response = client
            .request(DaemonCommand::List {
                id: None,
                all: None,
                cwd: None,
                session_dir: None,
                include_client_owned: None,
                rest: Map::new(),
            })
            .await
            .unwrap();
        assert!(response.success);
        assert_eq!(response.data.unwrap()["echoed"], "list");
    }

    #[tokio::test]
    async fn events_stream_in_order_after_connect() {
        let dir = tempfile::tempdir().unwrap();
        let chunk = serde_json::json!({
            "type": "session_snapshot_chunk",
            "activeSessionId": "abc",
            "snapshotId": "s1",
            "messages": []
        });
        let event = serde_json::json!({
            "type": "session_event",
            "activeSessionId": "abc",
            "event": { "type": "message" }
        });
        let socket_path = serve(dir.path(), vec![chunk, event]).await;
        let (client, mut events) = DaemonClient::connect(&socket_path).await.unwrap();
        let _ = client.list().await.unwrap();
        match events.recv().await {
            Some(DaemonEvent::SnapshotChunk {
                active_session_id, ..
            }) => {
                assert_eq!(active_session_id, "abc");
            }
            other => panic!("expected snapshot chunk, got {other:?}"),
        }
        match events.recv().await {
            Some(DaemonEvent::SessionEvent { event, .. }) => {
                assert_eq!(event["type"], "message");
            }
            other => panic!("expected session event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn connect_times_out_on_missing_socket() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("nothing-here.sock");
        let result = DaemonClient::connect(&socket_path).await;
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("connect should fail on a missing socket"),
        };
        assert!(error.to_string().contains("Failed to connect"));
    }

    #[tokio::test]
    async fn unknown_protocol_is_rejected() {
        use tokio::net::UnixListener;
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("wrong-proto.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read_half, mut write_half) = stream.into_split();
            let hello = serde_json::json!({
                "type": "daemon_hello",
                "protocol": { "name": "wrong-daemon", "version": 1 }
            });
            let _ = write_half
                .write_all(format!("{}\n", hello).as_bytes())
                .await;
            drop(read_half);
        });
        let error = match DaemonClient::connect(&socket_path).await {
            Err(error) => error,
            Ok(_) => panic!("connect should reject an unknown protocol"),
        };
        assert!(error.to_string().contains("Unknown daemon protocol"));
    }
}
