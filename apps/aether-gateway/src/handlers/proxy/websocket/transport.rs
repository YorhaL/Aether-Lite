//! Upstream WebSocket handshake and bounded frame relay utilities.

use std::collections::BTreeMap;
use std::time::Duration;

use axum::extract::ws::{CloseFrame as AxumCloseFrame, Message as AxumWsMessage, WebSocket};
use axum::http::header::{
    ACCEPT, ACCEPT_ENCODING, CONNECTION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HOST,
    PROXY_AUTHORIZATION, TE, TRAILER, TRANSFER_ENCODING, UPGRADE,
};
use axum::http::{HeaderMap, HeaderName};
use futures_util::{SinkExt, TryFutureExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use url::Url;
use wreq::ws::message::{CloseFrame as WreqCloseFrame, Message as WreqWsMessage};

use crate::ai_serving::AiExecutionDecision;
use crate::execution_runtime::transport::{
    build_browser_wreq_client, build_request_headers, ExecutionTransportControls,
};
use crate::frontdoor_loop_guard::gateway_frontdoor_self_loop_guard_error;
use crate::handlers::proxy::websocket::session::{
    WebSocketSessionLimits, RELAY_WRITE_TIMEOUT, TEARDOWN_WRITE_TIMEOUT,
};

#[derive(Clone, Copy)]
pub(crate) struct UpstreamWebSocketErrorCodes {
    pub(crate) upstream_url_missing: &'static str,
    pub(crate) upstream_url_invalid: &'static str,
    pub(crate) frontdoor_self_loop: &'static str,
    pub(crate) headers_invalid: &'static str,
    pub(crate) client_build_failed: &'static str,
    pub(crate) handshake_failed: &'static str,
    pub(crate) upgrade_rejected: &'static str,
    pub(crate) upgrade_failed: &'static str,
}

pub(crate) struct UpstreamWebSocketConnection {
    pub(crate) socket: wreq::ws::WebSocket,
    pub(crate) response_headers: BTreeMap<String, String>,
}

pub(crate) async fn connect_upstream_websocket(
    decision: &AiExecutionDecision,
    limits: WebSocketSessionLimits,
    errors: UpstreamWebSocketErrorCodes,
) -> Result<UpstreamWebSocketConnection, &'static str> {
    let upstream_url = decision
        .upstream_url
        .as_deref()
        .ok_or(errors.upstream_url_missing)?;
    let upstream_url = guarded_websocket_upstream_url(
        upstream_url,
        errors.upstream_url_invalid,
        errors.frontdoor_self_loop,
    )?;
    let headers =
        websocket_handshake_headers(&decision.provider_request_headers, errors.headers_invalid)?;
    let client = build_websocket_client(decision, errors)?;
    let response = client
        .websocket(upstream_url.as_str())
        .headers(headers)
        .max_frame_size(limits.max_frame_size)
        .max_message_size(limits.max_message_size)
        .send()
        .await
        .map_err(|_| errors.handshake_failed)?;
    if response.status().as_u16() != 101 {
        return Err(errors.upgrade_rejected);
    }
    let response_headers = websocket_response_headers(response.headers());
    let socket = response
        .into_websocket()
        .await
        .map_err(|_| errors.upgrade_failed)?;
    Ok(UpstreamWebSocketConnection {
        socket,
        response_headers,
    })
}

fn guarded_websocket_upstream_url(
    raw: &str,
    invalid_code: &'static str,
    frontdoor_self_loop_code: &'static str,
) -> Result<Url, &'static str> {
    let upstream_url = websocket_upstream_url(raw, invalid_code)?;
    if gateway_frontdoor_self_loop_guard_error(upstream_url.as_str()).is_some() {
        return Err(frontdoor_self_loop_code);
    }
    Ok(upstream_url)
}

fn websocket_response_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter(|(name, _)| websocket_response_header_is_safe_to_retain(name))
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

fn websocket_response_header_is_safe_to_retain(name: &HeaderName) -> bool {
    !matches!(
        name.as_str(),
        "authorization"
            | "proxy-authorization"
            | "www-authenticate"
            | "proxy-authenticate"
            | "authentication-info"
            | "proxy-authentication-info"
            | "cookie"
            | "set-cookie"
            | "set-cookie2"
            | "x-api-key"
            | "api-key"
            | "x-goog-api-key"
    )
}

pub(crate) fn websocket_upstream_url(
    raw: &str,
    invalid_code: &'static str,
) -> Result<Url, &'static str> {
    let mut url = Url::parse(raw).map_err(|_| invalid_code)?;
    if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err(invalid_code);
    }
    let websocket_scheme = match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        "wss" | "ws" => return Ok(url),
        _ => return Err(invalid_code),
    };
    url.set_scheme(websocket_scheme).map_err(|_| invalid_code)?;
    Ok(url)
}

pub(crate) fn websocket_handshake_headers(
    provider_headers: &BTreeMap<String, String>,
    invalid_code: &'static str,
) -> Result<HeaderMap, &'static str> {
    let connection_scoped_names = provider_headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case(CONNECTION.as_str()))
        .flat_map(|(_, value)| value.split(','))
        .filter_map(|name| HeaderName::from_bytes(name.trim().as_bytes()).ok())
        .collect::<Vec<_>>();
    let mut headers =
        build_request_headers(provider_headers, None, false).map_err(|_| invalid_code)?;
    for name in connection_scoped_names {
        headers.remove(name);
    }
    for header in [
        ACCEPT,
        ACCEPT_ENCODING,
        CONNECTION,
        CONTENT_ENCODING,
        CONTENT_LENGTH,
        CONTENT_TYPE,
        HOST,
        PROXY_AUTHORIZATION,
        TE,
        TRAILER,
        TRANSFER_ENCODING,
        UPGRADE,
    ] {
        headers.remove(header);
    }
    for header in ["keep-alive", "proxy-connection"] {
        headers.remove(header);
    }
    let websocket_managed_names = headers
        .keys()
        .filter(|name| name.as_str().starts_with("sec-websocket-"))
        .cloned()
        .collect::<Vec<_>>();
    for name in websocket_managed_names {
        headers.remove(name);
    }
    Ok(headers)
}

fn build_websocket_client(
    decision: &AiExecutionDecision,
    errors: UpstreamWebSocketErrorCodes,
) -> Result<wreq::Client, &'static str> {
    let timeouts = websocket_timeouts(decision);
    if let Some(profile) = decision.transport_profile.as_ref() {
        return build_browser_wreq_client(
            timeouts.as_ref(),
            profile,
            ExecutionTransportControls::default(),
            false,
        )
        .map_err(|_| errors.client_build_failed);
    }

    let mut builder = wreq::Client::builder();
    if let Some(connect_ms) = timeouts.as_ref().and_then(|timeouts| timeouts.connect_ms) {
        builder = builder.connect_timeout(Duration::from_millis(connect_ms));
    }
    builder.build().map_err(|_| errors.client_build_failed)
}

pub(crate) fn websocket_timeouts(
    decision: &AiExecutionDecision,
) -> Option<aether_contracts::ExecutionTimeouts> {
    let mut timeouts = decision.timeouts.clone()?;
    timeouts.read_ms = None;
    timeouts.first_byte_ms = None;
    timeouts.total_ms = None;
    Some(timeouts)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WebSocketWriteError {
    Failed,
    TimedOut,
    Cancelled,
}

impl WebSocketWriteError {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Failed => "write_failed",
            Self::TimedOut => "write_timeout",
            Self::Cancelled => "write_cancelled",
        }
    }
}

pub(crate) const RELAY_FRAME_QUEUE_CAPACITY: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WebSocketRelayQueueError {
    Closed,
    Cancelled,
}

#[derive(Clone, Default)]
pub(crate) struct WebSocketRelayPumpControl {
    cancellation: CancellationToken,
}

impl WebSocketRelayPumpControl {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn cancel(&self) {
        self.cancellation.cancel();
    }

    pub(crate) async fn cancelled(&self) {
        self.cancellation.cancelled().await;
    }

    pub(crate) async fn enqueue<T>(
        &self,
        sender: &mpsc::Sender<T>,
        message: T,
    ) -> Result<(), WebSocketRelayQueueError> {
        tokio::select! {
            biased;
            _ = self.cancellation.cancelled() => Err(WebSocketRelayQueueError::Cancelled),
            result = sender.send(message) => result.map_err(|_| WebSocketRelayQueueError::Closed),
        }
    }

    pub(crate) async fn send<F>(&self, write: F) -> Result<(), WebSocketWriteError>
    where
        F: std::future::Future<Output = Result<(), ()>>,
    {
        tokio::select! {
            biased;
            _ = self.cancellation.cancelled() => Err(WebSocketWriteError::Cancelled),
            result = bounded_send(RELAY_WRITE_TIMEOUT, write) => result,
        }
    }
}

pub(crate) fn websocket_relay_frame_queue<T>() -> (mpsc::Sender<T>, mpsc::Receiver<T>) {
    mpsc::channel(RELAY_FRAME_QUEUE_CAPACITY)
}

pub(crate) async fn send_client_message(
    client_socket: &mut WebSocket,
    message: AxumWsMessage,
) -> Result<(), WebSocketWriteError> {
    bounded_send(
        RELAY_WRITE_TIMEOUT,
        client_socket.send(message).map_err(|_| ()),
    )
    .await
}

pub(crate) async fn send_upstream_message(
    upstream: &mut wreq::ws::WebSocket,
    message: WreqWsMessage,
) -> Result<(), WebSocketWriteError> {
    bounded_send(RELAY_WRITE_TIMEOUT, upstream.send(message).map_err(|_| ())).await
}

async fn send_teardown_message<F>(write: F)
where
    F: std::future::Future<Output = Result<(), ()>>,
{
    let _ = bounded_send(TEARDOWN_WRITE_TIMEOUT, write).await;
}

async fn bounded_send<F>(budget: Duration, write: F) -> Result<(), WebSocketWriteError>
where
    F: std::future::Future<Output = Result<(), ()>>,
{
    match tokio::time::timeout(budget, write).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(())) => Err(WebSocketWriteError::Failed),
        Err(_) => Err(WebSocketWriteError::TimedOut),
    }
}

pub(crate) async fn close_upstream_socket(
    upstream: &mut wreq::ws::WebSocket,
    frame: Option<WreqCloseFrame>,
) {
    send_teardown_message(upstream.send(WreqWsMessage::Close(frame)).map_err(|_| ())).await;
}

pub(crate) fn upstream_message_to_client(message: WreqWsMessage) -> AxumWsMessage {
    match message {
        WreqWsMessage::Text(text) => AxumWsMessage::Text(text.to_string().into()),
        WreqWsMessage::Binary(data) => AxumWsMessage::Binary(data),
        WreqWsMessage::Ping(data) => AxumWsMessage::Ping(data),
        WreqWsMessage::Pong(data) => AxumWsMessage::Pong(data),
        WreqWsMessage::Close(frame) => AxumWsMessage::Close(frame.map(|frame| AxumCloseFrame {
            code: frame.code.into(),
            reason: frame.reason.to_string().into(),
        })),
    }
}

pub(crate) fn client_message_to_upstream(message: AxumWsMessage) -> WreqWsMessage {
    match message {
        AxumWsMessage::Text(text) => WreqWsMessage::Text(text.to_string().into()),
        AxumWsMessage::Binary(data) => WreqWsMessage::Binary(data),
        AxumWsMessage::Ping(data) => WreqWsMessage::Ping(data),
        AxumWsMessage::Pong(data) => WreqWsMessage::Pong(data),
        AxumWsMessage::Close(frame) => WreqWsMessage::Close(frame.map(|frame| WreqCloseFrame {
            code: frame.code.into(),
            reason: frame.reason.to_string().into(),
        })),
    }
}

pub(crate) async fn close_client_socket(client_socket: &mut WebSocket, code: u16, reason: &str) {
    send_teardown_message(
        client_socket
            .send(AxumWsMessage::Close(Some(AxumCloseFrame {
                code,
                reason: reason.to_string().into(),
            })))
            .map_err(|_| ()),
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stalled_writes_time_out_and_queue_is_bounded() {
        let stalled = std::future::pending::<Result<(), ()>>();
        assert_eq!(
            bounded_send(Duration::from_millis(1), stalled).await,
            Err(WebSocketWriteError::TimedOut)
        );

        let (sender, _receiver) = websocket_relay_frame_queue();
        for frame in 0..RELAY_FRAME_QUEUE_CAPACITY {
            sender.try_send(frame).expect("queue should accept frame");
        }
        assert!(matches!(
            sender.try_send(RELAY_FRAME_QUEUE_CAPACITY),
            Err(tokio::sync::mpsc::error::TrySendError::Full(_))
        ));
    }

    #[tokio::test]
    async fn cancellation_interrupts_a_stalled_write() {
        let control = WebSocketRelayPumpControl::new();
        let write = control.send(std::future::pending::<Result<(), ()>>());
        tokio::pin!(write);
        assert!(tokio::time::timeout(Duration::from_millis(5), &mut write)
            .await
            .is_err());
        control.cancel();
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(100), write)
                .await
                .expect("cancellation should wake the writer"),
            Err(WebSocketWriteError::Cancelled)
        );
    }

    #[test]
    fn converts_http_urls_and_rejects_embedded_credentials() {
        let url = websocket_upstream_url("https://example.test/v1/realtime?x=1", "invalid")
            .expect("URL should convert");
        assert_eq!(url.as_str(), "wss://example.test/v1/realtime?x=1");
        assert!(websocket_upstream_url("https://token@example.test/realtime", "invalid").is_err());
    }

    #[test]
    fn upstream_headers_keep_provider_auth_and_drop_handshake_state() {
        let provider_headers = BTreeMap::from([
            ("authorization".to_string(), "Bearer provider".to_string()),
            (
                "connection".to_string(),
                "keep-alive, x-provider-hop".to_string(),
            ),
            ("x-provider-hop".to_string(), "secret".to_string()),
            ("sec-websocket-key".to_string(), "nonce".to_string()),
            ("x-provider-header".to_string(), "safe".to_string()),
        ]);
        let headers = websocket_handshake_headers(&provider_headers, "invalid")
            .expect("headers should build");
        assert_eq!(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer provider")
        );
        assert!(headers.get("x-provider-hop").is_none());
        assert!(headers.get("sec-websocket-key").is_none());
        assert!(headers.get("x-provider-header").is_some());
    }
}
