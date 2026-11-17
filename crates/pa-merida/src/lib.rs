//! pa-merida: the pa-merida desktop client's daemon-facing core.
pub mod daemon_client;
pub mod transcript;

pub use daemon_client::{DaemonClient, DaemonEvent};
