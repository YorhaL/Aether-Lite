//! Connection-scoped limits shared by AI WebSocket sessions.

use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub(crate) struct WebSocketSessionLimits {
    pub(crate) max_frame_size: usize,
    pub(crate) max_message_size: usize,
    pub(crate) initial_message_timeout: Duration,
    pub(crate) max_connection_duration: Duration,
}

pub(crate) const RESPONSES_WEBSOCKET_SESSION_LIMITS: WebSocketSessionLimits =
    WebSocketSessionLimits {
        max_frame_size: 16 << 20,
        max_message_size: 16 << 20,
        initial_message_timeout: Duration::from_secs(60),
        max_connection_duration: Duration::from_secs(60 * 60),
    };

pub(crate) const REALTIME_WEBSOCKET_SESSION_LIMITS: WebSocketSessionLimits =
    WebSocketSessionLimits {
        max_frame_size: 16 << 20,
        max_message_size: 16 << 20,
        initial_message_timeout: Duration::from_secs(60),
        max_connection_duration: Duration::from_secs(60 * 60),
    };

pub(crate) const RELAY_WRITE_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const TEARDOWN_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) const CLOSE_POLICY_VIOLATION: u16 = 1008;
pub(crate) const CLOSE_INTERNAL_ERROR: u16 = 1011;
pub(crate) const CLOSE_TRY_AGAIN: u16 = 1013;
pub(crate) const WEBSOCKET_LOG_TRANSPORT: &str = "websocket";
