//! Default-lane session engine for the standard Responses WebSocket protocol.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use axum::extract::ws::{Message as AxumWsMessage, WebSocket};
use axum::http::StatusCode;
use futures_util::StreamExt;
use serde_json::Value;
use tracing::{info, warn};
use wreq::ws::message::Message as WreqWsMessage;

use crate::control::{
    execution_plan_balance_capacity_rejection, execution_plan_cost_is_proven_zero,
    request_model_local_rejection, GatewayControlDecision, GatewayLocalAuthRejection,
};
use crate::execution_runtime::acquire_upstream_execution_gate;
use crate::handlers::proxy::websocket::ingress::{
    WebSocketConnectionLog, WebSocketConnectionLogSpec, WebSocketRequestContext,
};
use crate::handlers::proxy::websocket::session::{
    CLOSE_INTERNAL_ERROR, CLOSE_POLICY_VIOLATION, CLOSE_TRY_AGAIN,
    RESPONSES_WEBSOCKET_SESSION_LIMITS, WEBSOCKET_LOG_TRANSPORT,
};
use crate::handlers::proxy::websocket::transport::{
    close_client_socket, close_upstream_socket, connect_upstream_websocket_plan,
    send_client_message, send_upstream_message, upstream_message_to_client,
    UpstreamWebSocketErrorCodes,
};
use crate::handlers::shared::ip_rules_allow;
use crate::privacy::{restore_sync_response_body, RedactionSession};
use crate::{AppState, FrontdoorUserRpmOutcome, GatewayError};

use super::audit::{ResponsesTurnAudit, ResponsesTurnOutcome};
use super::planner::{
    plan_responses_turn, responses_planning_parts, PlannedResponsesTurn, ResponsesUpstreamBinding,
};
use super::protocol::{
    gateway_error_event, observe_provider_event, parse_response_create_event,
    retain_owned_response_id, ProviderTerminalKind, ResponseCreateEvent, ResponsesProtocolError,
};

const LOG_TARGET: &str = "aether_gateway::handlers::proxy::responses_ws";
const DEFAULT_FIRST_EVENT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_TURN_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const CONNECTION_LOG_SPEC: WebSocketConnectionLogSpec = WebSocketConnectionLogSpec {
    opened_event_name: "openai_responses_websocket_connection_opened",
    closed_event_name: "openai_responses_websocket_connection_closed",
    opened_message: "gateway accepted OpenAI Responses WebSocket connection",
    closed_message: "gateway closed OpenAI Responses WebSocket connection",
    execution_path: "openai_responses_websocket_bridge",
    provider_type: "openai_responses",
};
const UPSTREAM_ERRORS: UpstreamWebSocketErrorCodes = UpstreamWebSocketErrorCodes {
    upstream_url_missing: "responses_websocket_upstream_url_missing",
    upstream_url_invalid: "responses_websocket_upstream_url_invalid",
    frontdoor_self_loop: "responses_websocket_frontdoor_self_loop",
    headers_invalid: "responses_websocket_headers_invalid",
    client_build_failed: "responses_websocket_client_build_failed",
    handshake_failed: "responses_websocket_handshake_failed",
    upgrade_rejected: "responses_websocket_upgrade_rejected",
    upgrade_failed: "responses_websocket_upgrade_failed",
};

struct UpstreamSession {
    socket: wreq::ws::WebSocket,
    binding: ResponsesUpstreamBinding,
}

#[derive(Default)]
struct ResponsesWebSocketRedactionRestorer {
    sessions: Vec<RedactionSession>,
    delta_carries: BTreeMap<String, String>,
}

impl ResponsesWebSocketRedactionRestorer {
    fn push_session(&mut self, session: RedactionSession) {
        self.sessions.push(session);
    }

    fn restore_event(&mut self, text: &str) -> Result<String, GatewayError> {
        let Ok(mut event) = serde_json::from_str::<Value>(text) else {
            return restore_text_with_sessions(text, &self.sessions);
        };
        if event
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|event_type| event_type.ends_with(".delta"))
        {
            let key = response_delta_stream_key(&event);
            if let Some(delta) = event
                .get("delta")
                .and_then(Value::as_str)
                .map(str::to_string)
            {
                let mut combined = self.delta_carries.remove(&key).unwrap_or_default();
                combined.push_str(&delta);
                let carry_len = longest_partial_sentinel_suffix(&combined, &self.sessions);
                let carry = combined.split_off(combined.len().saturating_sub(carry_len));
                if !carry.is_empty() {
                    self.delta_carries.insert(key, carry);
                }
                *event
                    .get_mut("delta")
                    .expect("delta field was present and string") =
                    Value::String(restore_text_with_sessions(&combined, &self.sessions)?);
            }
        }
        restore_json_event_with_sessions(event, &self.sessions)
    }

    fn finish_turn(&mut self) {
        self.delta_carries.clear();
    }
}

struct ResponsesTurnAdmission {
    _upstream_execution: Option<aether_runtime::ConcurrencyPermit>,
    _upstream_target: Option<crate::upstream_admission::UpstreamTargetAdmissionPermit>,
}

impl ResponsesTurnAdmission {
    async fn acquire(
        state: &AppState,
        plan: &aether_contracts::ExecutionPlan,
        trace_id: &str,
    ) -> Result<Self, GatewayError> {
        let upstream_execution = acquire_upstream_execution_gate(state, trace_id).await?;
        let upstream_target = match state
            .upstream_target_admission
            .acquire(plan, trace_id)
            .await
        {
            Ok(permit) => permit,
            Err(error) => {
                drop(upstream_execution);
                return Err(error);
            }
        };
        Ok(Self {
            _upstream_execution: upstream_execution,
            _upstream_target: upstream_target,
        })
    }
}

pub(super) async fn run_responses_websocket(
    mut client_socket: WebSocket,
    state: AppState,
    context: WebSocketRequestContext,
    (): (),
) {
    let connection_log = WebSocketConnectionLog::new(&context, CONNECTION_LOG_SPEC);
    connection_log.log_opened();
    let connection_started = Instant::now();
    let connection_deadline =
        connection_started + RESPONSES_WEBSOCKET_SESSION_LIMITS.max_connection_duration;
    let mut upstream: Option<UpstreamSession> = None;
    let mut owned_response_ids = BTreeSet::new();
    let mut redaction_restorer = ResponsesWebSocketRedactionRestorer::default();
    let mut turn_index = 0u64;

    loop {
        let idle = wait_for_idle_client_event(
            &mut client_socket,
            upstream.as_mut(),
            &context,
            &mut redaction_restorer,
            connection_deadline,
            (turn_index == 0).then_some(
                connection_started + RESPONSES_WEBSOCKET_SESSION_LIMITS.initial_message_timeout,
            ),
        )
        .await;
        let event = match idle {
            IdleOutcome::Event(event) => event,
            IdleOutcome::ProtocolError(error) => {
                send_protocol_error(&mut client_socket, error).await;
                continue;
            }
            IdleOutcome::ConnectionLimit => {
                close_client_socket(
                    &mut client_socket,
                    CLOSE_TRY_AGAIN,
                    "connection_duration_limit",
                )
                .await;
                break;
            }
            IdleOutcome::InitialMessageTimeout => {
                send_gateway_error(
                    &mut client_socket,
                    StatusCode::REQUEST_TIMEOUT,
                    "responses_websocket_initial_message_timeout",
                    "The first response.create event was not received in time",
                    None,
                )
                .await;
                close_client_socket(
                    &mut client_socket,
                    CLOSE_POLICY_VIOLATION,
                    "initial_message_timeout",
                )
                .await;
                break;
            }
            IdleOutcome::AdmissionLost => {
                close_client_socket(
                    &mut client_socket,
                    CLOSE_TRY_AGAIN,
                    "connection_admission_lost",
                )
                .await;
                break;
            }
            IdleOutcome::Closed => break,
            IdleOutcome::UpstreamFailed => {
                send_gateway_error(
                    &mut client_socket,
                    StatusCode::BAD_GATEWAY,
                    "responses_websocket_upstream_closed",
                    "The upstream Responses WebSocket closed",
                    None,
                )
                .await;
                break;
            }
        };
        let event = match parse_response_create_event(event.as_str(), &owned_response_ids) {
            Ok(event) => event,
            Err(error) => {
                send_protocol_error(&mut client_socket, error).await;
                continue;
            }
        };
        turn_index = turn_index.saturating_add(1);
        let expected_binding = upstream.as_ref().map(|upstream| &upstream.binding);
        let prepared = match prepare_turn(&state, &context, &event, expected_binding, turn_index)
            .await
        {
            Ok(prepared) => prepared,
            Err(rejection) => {
                send_gateway_error(
                    &mut client_socket,
                    rejection.status,
                    rejection.code,
                    rejection.message.as_str(),
                    rejection.param,
                )
                .await;
                if rejection.close_connection {
                    close_client_socket(&mut client_socket, rejection.close_code, rejection.code)
                        .await;
                    break;
                }
                continue;
            }
        };

        let PreparedTurn {
            planned:
                PlannedResponsesTurn {
                    plan,
                    report_context,
                    provider_event,
                    binding,
                    redaction_session,
                },
            admission,
        } = prepared;
        let audit = ResponsesTurnAudit::begin(&state, plan, report_context, turn_index).await;
        if upstream.is_none() {
            match connect_upstream_websocket_plan(
                audit.plan(),
                RESPONSES_WEBSOCKET_SESSION_LIMITS,
                UPSTREAM_ERRORS,
            )
            .await
            {
                Ok(connection) => {
                    upstream = Some(UpstreamSession {
                        socket: connection.socket,
                        binding,
                    });
                }
                Err(error_code) => {
                    warn!(
                        target: LOG_TARGET,
                        event_name = "responses_websocket_upstream_connect_failed",
                        log_type = "ops",
                        trace_id = %context.trace_id,
                        request_id = %audit.plan().request_id,
                        error_code,
                        "Responses WebSocket upstream connection failed"
                    );
                    audit
                        .finish(&state, ResponsesTurnOutcome::UpstreamConnectFailed, None)
                        .await;
                    drop(admission);
                    send_gateway_error(
                        &mut client_socket,
                        StatusCode::BAD_GATEWAY,
                        error_code,
                        "Responses upstream WebSocket connection failed",
                        None,
                    )
                    .await;
                    close_client_socket(&mut client_socket, CLOSE_TRY_AGAIN, error_code).await;
                    break;
                }
            }
        }
        let upstream_socket = &mut upstream.as_mut().expect("upstream was connected").socket;
        if provider_event.len() > RESPONSES_WEBSOCKET_SESSION_LIMITS.max_message_size
            || send_upstream_message(upstream_socket, WreqWsMessage::Text(provider_event.into()))
                .await
                .is_err()
        {
            audit
                .finish(&state, ResponsesTurnOutcome::UpstreamWriteFailed, None)
                .await;
            drop(admission);
            send_gateway_error(
                &mut client_socket,
                StatusCode::BAD_GATEWAY,
                "responses_websocket_upstream_write_failed",
                "Could not send response.create to the upstream",
                None,
            )
            .await;
            break;
        }
        if let Some(redaction_session) = redaction_session {
            redaction_restorer.push_session(redaction_session);
        }

        let outcome = relay_turn(
            &mut client_socket,
            upstream_socket,
            &state,
            &context,
            audit,
            &mut owned_response_ids,
            &mut redaction_restorer,
            connection_deadline,
        )
        .await;
        drop(admission);
        if outcome == TurnRelayOutcome::CloseConnection {
            break;
        }
    }

    if let Some(mut upstream) = upstream {
        close_upstream_socket(&mut upstream.socket, None).await;
    }
    info!(
        target: LOG_TARGET,
        event_name = "responses_websocket_session_finished",
        log_type = "event",
        transport = WEBSOCKET_LOG_TRANSPORT,
        websocket = true,
        trace_id = %context.trace_id,
        turns = turn_index,
        elapsed_ms = connection_started.elapsed().as_millis() as u64,
        "Responses WebSocket session finished"
    );
}

struct PreparedTurn {
    planned: PlannedResponsesTurn,
    admission: ResponsesTurnAdmission,
}

async fn prepare_turn(
    state: &AppState,
    context: &WebSocketRequestContext,
    event: &ResponseCreateEvent,
    expected_binding: Option<&ResponsesUpstreamBinding>,
    _turn_index: u64,
) -> Result<PreparedTurn, TurnRejection> {
    let decision = refresh_turn_control(state, context, event).await?;
    let planned = plan_responses_turn(state, context, &decision, event, expected_binding)
        .await
        .map_err(TurnRejection::from_gateway)?
        .ok_or_else(|| {
            if expected_binding.is_some() {
                TurnRejection::conflict(
                    "responses_websocket_binding_changed",
                    "No eligible turn candidate matches the upstream connection binding; open a new WebSocket",
                )
            } else {
                TurnRejection::unavailable(
                    "responses_websocket_provider_unavailable",
                    "No explicitly enabled OpenAI Responses WebSocket provider is available",
                )
            }
        })?;
    let daily = state.frontdoor_daily_usage().check(state, &decision).await;
    if matches!(
        daily,
        crate::daily_usage_limit::FrontdoorDailyUsageOutcome::Rejected(_)
    ) && !execution_plan_cost_is_proven_zero(
        state,
        &planned.plan,
        planned.report_context.as_ref(),
    )
    .await
    {
        return Err(TurnRejection::rate_limited(
            "daily_usage_limit_exceeded",
            "Daily usage limit exceeded",
        ));
    }
    if execution_plan_balance_capacity_rejection(
        state,
        &decision,
        &planned.plan,
        planned.report_context.as_ref(),
    )
    .await
    .map_err(TurnRejection::from_gateway)?
    .is_some()
    {
        return Err(TurnRejection::rate_limited(
            "insufficient_balance",
            "Insufficient balance for this response.create turn",
        ));
    }
    let admission =
        ResponsesTurnAdmission::acquire(state, &planned.plan, planned.plan.request_id.as_str())
            .await
            .map_err(TurnRejection::from_gateway)?;
    Ok(PreparedTurn { planned, admission })
}

async fn refresh_turn_control(
    state: &AppState,
    context: &WebSocketRequestContext,
    event: &ResponseCreateEvent,
) -> Result<GatewayControlDecision, TurnRejection> {
    if state
        .admin_security_ip_blacklisted(context.client_ip)
        .await
        .map_err(TurnRejection::from_gateway)?
    {
        return Err(TurnRejection::forbidden(
            "ip_blocked",
            "The current IP is blocked",
        ));
    }
    let mut decision = context.decision.clone();
    if let Some(auth) = decision.auth_context.take() {
        let refreshed = crate::control::refresh_execution_runtime_auth_context(
            state,
            auth,
            decision.auth_endpoint_signature.as_deref(),
        )
        .await
        .map_err(TurnRejection::from_gateway)?;
        decision.local_auth_rejection = refreshed.local_rejection.clone();
        decision.auth_context = Some(refreshed);
    }
    if let Some(rejection) = decision.local_auth_rejection.clone() {
        return Err(TurnRejection::from_auth(rejection));
    }
    let auth = decision
        .auth_context
        .as_ref()
        .ok_or_else(|| TurnRejection::unauthorized("invalid_api_key", "Invalid API key"))?;
    if !auth.access_allowed || auth.user_id.trim().is_empty() || auth.api_key_id.trim().is_empty() {
        return Err(TurnRejection::unauthorized(
            "invalid_api_key",
            "Invalid API key",
        ));
    }
    if !ip_rules_allow(auth.ip_rules.as_deref(), context.client_ip) {
        return Err(TurnRejection::unauthorized(
            "ip_not_allowed",
            "The current IP is not allowed for this API key",
        ));
    }
    let parts = responses_planning_parts(context);
    let body = serde_json::to_vec(&event.value)
        .map(axum::body::Bytes::from)
        .map_err(|error| TurnRejection::internal(error.to_string()))?;
    if let Some(rejection) =
        request_model_local_rejection(state, Some(&decision), &parts.uri, &parts.headers, &body)
            .await
            .map_err(TurnRejection::from_gateway)?
    {
        return Err(TurnRejection::from_auth(rejection));
    }
    let whitelisted = match state.admin_security_ip_whitelisted(context.client_ip).await {
        Ok(value) => value,
        Err(error) => {
            warn!(
                target: LOG_TARGET,
                event_name = "responses_websocket_ip_whitelist_check_failed",
                log_type = "ops",
                trace_id = %context.trace_id,
                error = ?error,
                "gateway continued with Responses WebSocket limits after whitelist lookup failed"
            );
            false
        }
    };
    if whitelisted {
        if let Some(auth) = decision.auth_context.as_mut() {
            auth.ip_bypass_limits = true;
        }
    } else if matches!(
        state
            .frontdoor_user_rpm()
            .check_and_consume(state, Some(&decision))
            .await
            .map_err(TurnRejection::from_gateway)?,
        FrontdoorUserRpmOutcome::Rejected(_)
    ) {
        return Err(TurnRejection::rate_limited(
            "rate_limit_exceeded",
            "Responses WebSocket turn rate limit exceeded",
        ));
    }
    Ok(decision)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnRelayOutcome {
    Continue,
    CloseConnection,
}

async fn relay_turn(
    client: &mut WebSocket,
    upstream: &mut wreq::ws::WebSocket,
    state: &AppState,
    context: &WebSocketRequestContext,
    audit: ResponsesTurnAudit,
    owned_response_ids: &mut BTreeSet<String>,
    redaction_restorer: &mut ResponsesWebSocketRedactionRestorer,
    connection_deadline: Instant,
) -> TurnRelayOutcome {
    let started_at = Instant::now();
    let first_event_deadline = started_at + first_event_timeout(audit.plan());
    let terminal_deadline = started_at + turn_timeout(audit.plan());
    let mut audit = Some(audit);
    let mut saw_provider_event = false;

    loop {
        let deadline = if saw_provider_event {
            terminal_deadline
        } else {
            first_event_deadline.min(terminal_deadline)
        };
        tokio::select! {
            client_message = client.recv() => {
                match client_message {
                    None | Some(Err(_)) | Some(Ok(AxumWsMessage::Close(_))) => {
                        finish_audit(&mut audit, state, ResponsesTurnOutcome::ClientDisconnected, None).await;
                        return TurnRelayOutcome::CloseConnection;
                    }
                    Some(Ok(AxumWsMessage::Ping(data))) => {
                        if send_upstream_message(upstream, WreqWsMessage::Ping(data)).await.is_err() {
                            finish_audit(&mut audit, state, ResponsesTurnOutcome::UpstreamWriteFailed, None).await;
                            return TurnRelayOutcome::CloseConnection;
                        }
                    }
                    Some(Ok(AxumWsMessage::Pong(data))) => {
                        if send_upstream_message(upstream, WreqWsMessage::Pong(data)).await.is_err() {
                            finish_audit(&mut audit, state, ResponsesTurnOutcome::UpstreamWriteFailed, None).await;
                            return TurnRelayOutcome::CloseConnection;
                        }
                    }
                    Some(Ok(AxumWsMessage::Text(text))) => {
                        let is_create = serde_json::from_str::<Value>(text.as_str()).ok()
                            .and_then(|value| value.get("type").and_then(Value::as_str).map(str::to_string))
                            .as_deref() == Some("response.create");
                        let (code, message) = if is_create {
                            ("responses_websocket_turn_in_progress", "Wait for the current response terminal event before sending another response.create")
                        } else {
                            ("invalid_response_create", "Only response.create JSON events are accepted from the client")
                        };
                        let status = if is_create {
                            StatusCode::CONFLICT
                        } else {
                            StatusCode::BAD_REQUEST
                        };
                        send_gateway_error(client, status, code, message, None).await;
                    }
                    Some(Ok(AxumWsMessage::Binary(_))) => {
                        send_gateway_error(client, StatusCode::BAD_REQUEST, "responses_websocket_binary_client_event", "Responses WebSocket client events must be JSON text", None).await;
                    }
                }
            }
            provider_message = upstream.next() => {
                let provider_message = match provider_message {
                    None => {
                        finish_audit(&mut audit, state, ResponsesTurnOutcome::UpstreamClosed, None).await;
                        return TurnRelayOutcome::CloseConnection;
                    }
                    Some(Err(_)) => {
                        finish_audit(&mut audit, state, ResponsesTurnOutcome::UpstreamReadFailed, None).await;
                        return TurnRelayOutcome::CloseConnection;
                    }
                    Some(Ok(message)) => message,
                };
                match provider_message {
                    WreqWsMessage::Text(text) => {
                        let parsed = serde_json::from_str::<Value>(text.as_str()).ok();
                        if parsed.as_ref().is_some_and(|event| event.get("stream_id").is_some()) {
                            send_gateway_error(client, StatusCode::BAD_GATEWAY, "responses_websocket_unexpected_stream_id", "Upstream sent a named-lane event on a default-lane connection", Some("stream_id")).await;
                            finish_audit(&mut audit, state, ResponsesTurnOutcome::ProviderError, None).await;
                            return TurnRelayOutcome::CloseConnection;
                        }
                        if !saw_provider_event {
                            saw_provider_event = true;
                            audit
                                .as_mut()
                                .expect("turn audit is active")
                                .observe_first_event(state)
                                .await;
                        }
                        let terminal = parsed.as_ref().and_then(observe_provider_event);
                        let client_text = match redaction_restorer.restore_event(text.as_str()) {
                            Ok(text) => text,
                            Err(error) => {
                                warn!(
                                    target: LOG_TARGET,
                                    event_name = "responses_websocket_redaction_restore_failed",
                                    log_type = "ops",
                                    trace_id = %context.trace_id,
                                    error = ?error,
                                    "Responses WebSocket response redaction restoration failed"
                                );
                                send_gateway_error(client, StatusCode::BAD_GATEWAY, "responses_websocket_redaction_restore_failed", "Could not restore the upstream Responses event", None).await;
                                if terminal.is_some() {
                                    finish_client_delivery_failure(&mut audit, state, terminal.as_ref()).await;
                                } else {
                                    finish_audit(&mut audit, state, ResponsesTurnOutcome::ProviderError, None).await;
                                }
                                return TurnRelayOutcome::CloseConnection;
                            }
                        };
                        if send_client_message(client, AxumWsMessage::Text(client_text.into())).await.is_err() {
                            finish_client_delivery_failure(&mut audit, state, terminal.as_ref()).await;
                            return TurnRelayOutcome::CloseConnection;
                        }
                        if let Some(terminal) = terminal {
                            redaction_restorer.finish_turn();
                            if matches!(terminal.kind, ProviderTerminalKind::Completed | ProviderTerminalKind::Incomplete) {
                                if let Some(response_id) = terminal.response_id.clone() {
                                    retain_owned_response_id(owned_response_ids, response_id);
                                }
                            }
                            let outcome = ResponsesTurnOutcome::from_provider_terminal(&terminal);
                            finish_audit(&mut audit, state, outcome, Some(&terminal)).await;
                            return TurnRelayOutcome::Continue;
                        }
                    }
                    WreqWsMessage::Ping(data) => {
                        if send_client_message(client, AxumWsMessage::Ping(data)).await.is_err() {
                            finish_audit(&mut audit, state, ResponsesTurnOutcome::ClientDisconnected, None).await;
                            return TurnRelayOutcome::CloseConnection;
                        }
                    }
                    WreqWsMessage::Pong(data) => {
                        if send_client_message(client, AxumWsMessage::Pong(data)).await.is_err() {
                            finish_audit(&mut audit, state, ResponsesTurnOutcome::ClientDisconnected, None).await;
                            return TurnRelayOutcome::CloseConnection;
                        }
                    }
                    WreqWsMessage::Close(frame) => {
                        let _ = send_client_message(client, upstream_message_to_client(WreqWsMessage::Close(frame))).await;
                        finish_audit(&mut audit, state, ResponsesTurnOutcome::UpstreamClosed, None).await;
                        return TurnRelayOutcome::CloseConnection;
                    }
                    WreqWsMessage::Binary(_) => {
                        send_gateway_error(client, StatusCode::BAD_GATEWAY, "responses_websocket_binary_upstream_event", "Upstream Responses events must be JSON text", None).await;
                        finish_audit(&mut audit, state, ResponsesTurnOutcome::ProviderError, None).await;
                        return TurnRelayOutcome::CloseConnection;
                    }
                }
            }
            _ = tokio::time::sleep_until(connection_deadline.into()) => {
                finish_audit(&mut audit, state, ResponsesTurnOutcome::ConnectionLimit, None).await;
                close_client_socket(client, CLOSE_TRY_AGAIN, "connection_duration_limit").await;
                return TurnRelayOutcome::CloseConnection;
            }
            _ = wait_for_connection_permit_loss(context.websocket_connection_permit.as_ref()) => {
                finish_audit(&mut audit, state, ResponsesTurnOutcome::ConnectionAdmissionLost, None).await;
                close_client_socket(client, CLOSE_TRY_AGAIN, "connection_admission_lost").await;
                return TurnRelayOutcome::CloseConnection;
            }
            _ = tokio::time::sleep_until(deadline.into()) => {
                let outcome = if saw_provider_event {
                    ResponsesTurnOutcome::TurnTimeout
                } else {
                    ResponsesTurnOutcome::FirstEventTimeout
                };
                finish_audit(&mut audit, state, outcome, None).await;
                send_gateway_error(client, StatusCode::GATEWAY_TIMEOUT, outcome.reason(), "Provider did not finish the response within the configured timeout", None).await;
                return TurnRelayOutcome::CloseConnection;
            }
        }
    }
}

fn restore_json_event_with_sessions(
    event: Value,
    redaction_sessions: &[RedactionSession],
) -> Result<String, GatewayError> {
    let text =
        serde_json::to_vec(&event).map_err(|error| GatewayError::Internal(error.to_string()))?;
    let text = restore_bytes_with_sessions(&text, redaction_sessions)?;
    String::from_utf8(text).map_err(|error| {
        GatewayError::Internal(format!("restored WebSocket event was not UTF-8: {error}"))
    })
}

fn restore_text_with_sessions(
    text: &str,
    redaction_sessions: &[RedactionSession],
) -> Result<String, GatewayError> {
    let text = restore_bytes_with_sessions(text.as_bytes(), redaction_sessions)?;
    String::from_utf8(text).map_err(|error| {
        GatewayError::Internal(format!("restored WebSocket text was not UTF-8: {error}"))
    })
}

fn restore_bytes_with_sessions(
    text: &[u8],
    redaction_sessions: &[RedactionSession],
) -> Result<Vec<u8>, GatewayError> {
    let mut text = text.to_vec();
    let mut headers =
        BTreeMap::from([("content-type".to_string(), "application/json".to_string())]);
    for redaction_session in redaction_sessions {
        text = restore_sync_response_body(&mut headers, &text, redaction_session)?.body;
    }
    Ok(text)
}

fn response_delta_stream_key(event: &Value) -> String {
    serde_json::to_string(&[
        event.get("type"),
        event.get("item_id"),
        event.get("output_index"),
        event.get("content_index"),
        event.get("summary_index"),
    ])
    .expect("Responses delta stream key should serialize")
}

fn longest_partial_sentinel_suffix(text: &str, sessions: &[RedactionSession]) -> usize {
    let text = text.as_bytes();
    sessions
        .iter()
        .flat_map(RedactionSession::mappings)
        .filter_map(|mapping| {
            let sentinel = mapping.sentinel.as_bytes();
            let max_prefix_len = text.len().min(sentinel.len().saturating_sub(1));
            (1..=max_prefix_len)
                .rev()
                .find(|prefix_len| text.ends_with(&sentinel[..*prefix_len]))
        })
        .max()
        .unwrap_or_default()
}

async fn finish_audit(
    audit: &mut Option<ResponsesTurnAudit>,
    state: &AppState,
    outcome: ResponsesTurnOutcome,
    terminal: Option<&super::protocol::ProviderTerminal>,
) {
    if let Some(audit) = audit.take() {
        audit.finish(state, outcome, terminal).await;
    }
}

async fn finish_client_delivery_failure(
    audit: &mut Option<ResponsesTurnAudit>,
    state: &AppState,
    terminal: Option<&super::protocol::ProviderTerminal>,
) {
    if let Some(audit) = audit.as_mut() {
        audit.observe_client_delivery_failed();
    }
    let outcome = client_delivery_failure_outcome(terminal);
    finish_audit(audit, state, outcome, terminal).await;
}

fn client_delivery_failure_outcome(
    terminal: Option<&super::protocol::ProviderTerminal>,
) -> ResponsesTurnOutcome {
    terminal
        .map(ResponsesTurnOutcome::from_provider_terminal)
        .unwrap_or(ResponsesTurnOutcome::ClientDisconnected)
}

enum IdleOutcome {
    Event(String),
    ProtocolError(ResponsesProtocolError),
    ConnectionLimit,
    InitialMessageTimeout,
    AdmissionLost,
    Closed,
    UpstreamFailed,
}

async fn wait_for_idle_client_event(
    client: &mut WebSocket,
    mut upstream: Option<&mut UpstreamSession>,
    context: &WebSocketRequestContext,
    redaction_restorer: &mut ResponsesWebSocketRedactionRestorer,
    connection_deadline: Instant,
    initial_message_deadline: Option<Instant>,
) -> IdleOutcome {
    loop {
        tokio::select! {
            message = client.recv() => match message {
                None | Some(Err(_)) | Some(Ok(AxumWsMessage::Close(_))) => return IdleOutcome::Closed,
                Some(Ok(AxumWsMessage::Text(text))) => return IdleOutcome::Event(text.to_string()),
                Some(Ok(AxumWsMessage::Binary(_))) => return IdleOutcome::ProtocolError(ResponsesProtocolError::InvalidEvent),
                Some(Ok(AxumWsMessage::Ping(data))) => {
                    if let Some(upstream) = upstream.as_mut() {
                        if send_upstream_message(&mut upstream.socket, WreqWsMessage::Ping(data)).await.is_err() {
                            return IdleOutcome::UpstreamFailed;
                        }
                    } else if send_client_message(client, AxumWsMessage::Pong(data)).await.is_err() {
                        return IdleOutcome::Closed;
                    }
                }
                Some(Ok(AxumWsMessage::Pong(data))) => {
                    if let Some(upstream) = upstream.as_mut() {
                        if send_upstream_message(&mut upstream.socket, WreqWsMessage::Pong(data)).await.is_err() {
                            return IdleOutcome::UpstreamFailed;
                        }
                    }
                }
            },
            provider = read_idle_upstream(upstream.as_mut()) => match provider {
                IdleUpstreamOutcome::Message(message @ (WreqWsMessage::Ping(_) | WreqWsMessage::Pong(_))) => {
                    if send_client_message(client, upstream_message_to_client(message)).await.is_err() {
                        return IdleOutcome::Closed;
                    }
                }
                IdleUpstreamOutcome::Message(WreqWsMessage::Text(text)) => {
                    let Ok(text) = redaction_restorer.restore_event(text.as_str()) else {
                        return IdleOutcome::UpstreamFailed;
                    };
                    if send_client_message(client, AxumWsMessage::Text(text.into())).await.is_err() {
                        return IdleOutcome::Closed;
                    }
                }
                IdleUpstreamOutcome::Message(_) | IdleUpstreamOutcome::Failed => return IdleOutcome::UpstreamFailed,
            },
            _ = tokio::time::sleep_until(connection_deadline.into()) => return IdleOutcome::ConnectionLimit,
            _ = wait_for_optional_deadline(initial_message_deadline) => return IdleOutcome::InitialMessageTimeout,
            _ = wait_for_connection_permit_loss(context.websocket_connection_permit.as_ref()) => return IdleOutcome::AdmissionLost,
        }
    }
}

async fn wait_for_optional_deadline(deadline: Option<Instant>) {
    let Some(deadline) = deadline else {
        std::future::pending::<()>().await;
        return;
    };
    tokio::time::sleep_until(deadline.into()).await;
}

enum IdleUpstreamOutcome {
    Message(WreqWsMessage),
    Failed,
}

async fn read_idle_upstream(upstream: Option<&mut &mut UpstreamSession>) -> IdleUpstreamOutcome {
    let Some(upstream) = upstream else {
        std::future::pending::<()>().await;
        unreachable!();
    };
    match upstream.socket.next().await {
        Some(Ok(message)) => IdleUpstreamOutcome::Message(message),
        Some(Err(_)) | None => IdleUpstreamOutcome::Failed,
    }
}

fn first_event_timeout(plan: &aether_contracts::ExecutionPlan) -> Duration {
    plan.timeouts
        .as_ref()
        .and_then(|timeouts| timeouts.first_byte_ms)
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_FIRST_EVENT_TIMEOUT)
}

fn turn_timeout(plan: &aether_contracts::ExecutionPlan) -> Duration {
    plan.timeouts
        .as_ref()
        .and_then(|timeouts| timeouts.total_ms)
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_TURN_TIMEOUT)
}

async fn send_protocol_error(client: &mut WebSocket, error: ResponsesProtocolError) {
    send_gateway_error(
        client,
        StatusCode::BAD_REQUEST,
        error.code(),
        error.message(),
        error.param(),
    )
    .await;
}

async fn send_gateway_error(
    client: &mut WebSocket,
    status: StatusCode,
    code: &str,
    message: &str,
    param: Option<&str>,
) {
    let event = gateway_error_event(status.as_u16(), code, message, param).to_string();
    let _ = send_client_message(client, AxumWsMessage::Text(event.into())).await;
}

async fn wait_for_connection_permit_loss(permit: Option<&aether_runtime::AdmissionPermit>) {
    let Some(permit) = permit else {
        std::future::pending::<()>().await;
        return;
    };
    let mut health = tokio::time::interval(Duration::from_secs(1));
    health.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        health.tick().await;
        if !permit.is_healthy() {
            return;
        }
    }
}

struct TurnRejection {
    status: StatusCode,
    code: &'static str,
    message: String,
    param: Option<&'static str>,
    close_connection: bool,
    close_code: u16,
}

impl TurnRejection {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            param: None,
            close_connection: status.is_server_error(),
            close_code: if status == StatusCode::TOO_MANY_REQUESTS {
                CLOSE_TRY_AGAIN
            } else if status.is_client_error() {
                CLOSE_POLICY_VIOLATION
            } else {
                CLOSE_INTERNAL_ERROR
            },
        }
    }

    fn unauthorized(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, code, message)
    }

    fn forbidden(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, code, message)
    }

    fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    fn unavailable(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, code, message)
    }

    fn rate_limited(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, code, message)
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "responses_websocket_internal_error",
            message,
        )
    }

    fn from_gateway(error: GatewayError) -> Self {
        match error {
            GatewayError::AdmissionTimeout { .. } => Self::rate_limited(
                "responses_websocket_admission_timeout",
                "Responses WebSocket turn admission timed out",
            ),
            GatewayError::Client { status, message } => {
                Self::new(status, "responses_websocket_request_rejected", message)
            }
            GatewayError::LocalExecutionPlanningTimeout { .. } => Self::new(
                StatusCode::GATEWAY_TIMEOUT,
                "responses_websocket_planning_timeout",
                "Responses WebSocket turn planning timed out",
            ),
            other => Self::internal(format!("Responses WebSocket turn failed: {other:?}")),
        }
    }

    fn from_auth(rejection: GatewayLocalAuthRejection) -> Self {
        match rejection {
            GatewayLocalAuthRejection::InvalidApiKey => {
                Self::unauthorized("invalid_api_key", "Invalid API key")
            }
            GatewayLocalAuthRejection::LockedApiKey => {
                Self::forbidden("api_key_locked", "The API key is locked")
            }
            GatewayLocalAuthRejection::WalletUnavailable => {
                Self::forbidden("wallet_unavailable", "The account wallet is unavailable")
            }
            GatewayLocalAuthRejection::BalanceDenied { .. } => {
                Self::rate_limited("insufficient_balance", "Insufficient balance")
            }
            GatewayLocalAuthRejection::ProviderNotAllowed { .. } => {
                Self::forbidden("provider_not_allowed", "Provider is not allowed")
            }
            GatewayLocalAuthRejection::ApiFormatNotAllowed { .. } => Self::forbidden(
                "api_format_not_allowed",
                "OpenAI Responses is not allowed for this API key",
            ),
            GatewayLocalAuthRejection::ModelNotAllowed { .. } => {
                let mut rejection = Self::forbidden("model_not_allowed", "Model is not allowed");
                rejection.param = Some("model");
                rejection
            }
            GatewayLocalAuthRejection::IpNotAllowed { .. } => {
                Self::unauthorized("ip_not_allowed", "IP is not allowed for this API key")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{client_delivery_failure_outcome, ResponsesWebSocketRedactionRestorer};
    use crate::privacy::{RedactionSession, RedactionSessionConfig};

    use super::super::audit::ResponsesTurnOutcome;
    use super::super::protocol::observe_provider_event;

    #[test]
    fn provider_events_restore_redaction_sentinels_across_turns() {
        let mut first_session = RedactionSession::new(RedactionSessionConfig::new(
            b"responses-websocket-test-key".to_vec(),
            600,
            1_000,
        ));
        let mut second_session = RedactionSession::new(RedactionSessionConfig::new(
            b"responses-websocket-test-key".to_vec(),
            600,
            1_000,
        ));
        let redacted_email = first_session.redact_text("alice@example.com");
        let redacted_second_email = second_session.redact_text("bob@example.net");
        assert_ne!(redacted_email.text, "alice@example.com");
        assert_ne!(redacted_second_email.text, "bob@example.net");
        let first_split = redacted_email.text.len() / 2;
        let second_split = redacted_second_email.text.len() / 2;
        let first_event = serde_json::json!({
            "type": "response.output_text.delta",
            "item_id": "msg_1",
            "delta": &redacted_email.text[..first_split],
        })
        .to_string();
        let second_event = serde_json::json!({
            "type": "response.output_text.delta",
            "item_id": "msg_1",
            "delta": format!(
                "{} {}",
                &redacted_email.text[first_split..],
                &redacted_second_email.text[..second_split]
            ),
        })
        .to_string();
        let third_event = serde_json::json!({
            "type": "response.output_text.delta",
            "item_id": "msg_1",
            "delta": &redacted_second_email.text[second_split..],
        })
        .to_string();

        let mut restorer = ResponsesWebSocketRedactionRestorer::default();
        restorer.push_session(first_session);
        restorer.push_session(second_session);
        let restored_deltas = [first_event, second_event, third_event]
            .into_iter()
            .map(|event| {
                let restored = restorer
                    .restore_event(&event)
                    .expect("provider event should restore");
                serde_json::from_str::<serde_json::Value>(&restored)
                    .expect("restored event should remain JSON")["delta"]
                    .as_str()
                    .expect("delta should remain a string")
                    .to_string()
            })
            .collect::<String>();
        assert_eq!(restored_deltas, "alice@example.com bob@example.net");
    }

    #[test]
    fn provider_terminal_audit_stays_redacted_and_delivery_failure_keeps_its_outcome() {
        let mut session = RedactionSession::new(RedactionSessionConfig::new(
            b"responses-websocket-test-key".to_vec(),
            600,
            1_000,
        ));
        let redacted = session.redact_text("alice@example.com");
        assert_ne!(redacted.text, "alice@example.com");
        let provider_text = serde_json::json!({
            "type": "response.completed",
            "response": {
                "id": "resp_1",
                "output": [{"type": "message", "text": redacted.text}],
                "usage": {"input_tokens": 2, "output_tokens": 3, "total_tokens": 5}
            }
        })
        .to_string();
        let provider_event: serde_json::Value =
            serde_json::from_str(&provider_text).expect("provider event should be JSON");
        let terminal =
            observe_provider_event(&provider_event).expect("provider event should be terminal");

        let mut restorer = ResponsesWebSocketRedactionRestorer::default();
        restorer.push_session(session);
        let client_text = restorer
            .restore_event(&provider_text)
            .expect("client event should restore");

        let audit_text = terminal.event.to_string();
        assert!(!audit_text.contains("alice@example.com"));
        assert!(client_text.contains("alice@example.com"));
        assert_eq!(
            client_delivery_failure_outcome(Some(&terminal)),
            ResponsesTurnOutcome::Completed
        );
        assert_eq!(
            client_delivery_failure_outcome(None),
            ResponsesTurnOutcome::ClientDisconnected
        );
    }
}
