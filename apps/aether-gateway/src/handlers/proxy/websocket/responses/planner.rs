//! Per-turn same-format planning for Responses WebSocket mode.

use std::collections::BTreeMap;

use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderValue, Method};
use serde_json::Value;

use crate::ai_serving::{
    build_local_same_format_stream_attempt_source, resolve_local_same_format_stream_spec,
    LocalExecutionAttemptSource, OPENAI_RESPONSES_STREAM_PLAN_KIND,
};
use crate::headers::request_origin_from_headers_and_remote_addr;
use crate::privacy::{RedactionSession, RedactionSessionSlot};
use crate::{AppState, GatewayError};

use super::protocol::ResponseCreateEvent;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ResponsesUpstreamBinding {
    provider_id: String,
    endpoint_id: String,
    key_id: String,
    url: String,
    handshake_headers: BTreeMap<String, String>,
    transport_profile: Option<aether_contracts::ResolvedTransportProfile>,
}

impl ResponsesUpstreamBinding {
    fn from_plan(plan: &aether_contracts::ExecutionPlan) -> Result<Self, GatewayError> {
        let mut headers =
            crate::handlers::proxy::websocket::transport::websocket_handshake_headers(
                &plan.headers,
                "responses_websocket_headers_invalid",
            )
            .map_err(|code| GatewayError::Internal(code.to_string()))?;
        headers.remove(crate::constants::TRACE_ID_HEADER);
        let handshake_headers = headers
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_ascii_lowercase(), value.to_string()))
            })
            .collect();
        let url = crate::handlers::proxy::websocket::transport::websocket_upstream_url(
            plan.url.as_str(),
            "responses_websocket_upstream_url_invalid",
        )
        .map_err(|code| GatewayError::Internal(code.to_string()))?
        .to_string();
        Ok(Self {
            provider_id: plan.provider_id.clone(),
            endpoint_id: plan.endpoint_id.clone(),
            key_id: plan.key_id.clone(),
            url,
            handshake_headers,
            transport_profile: plan.transport_profile.clone(),
        })
    }
}

pub(super) struct PlannedResponsesTurn {
    pub(super) plan: aether_contracts::ExecutionPlan,
    pub(super) report_context: Option<Value>,
    pub(super) provider_event: String,
    pub(super) binding: ResponsesUpstreamBinding,
    pub(super) redaction_session: Option<RedactionSession>,
}

pub(super) async fn plan_responses_turn(
    state: &AppState,
    context: &crate::handlers::proxy::websocket::ingress::WebSocketRequestContext,
    decision: &crate::control::GatewayControlDecision,
    event: &ResponseCreateEvent,
    expected_binding: Option<&ResponsesUpstreamBinding>,
) -> Result<Option<PlannedResponsesTurn>, GatewayError> {
    let redaction_slot = RedactionSessionSlot::default();
    let parts = responses_planning_parts_with_redaction_slot(context, redaction_slot.clone());
    let planner_body = responses_http_planning_body(&event.value)?;
    let spec = resolve_local_same_format_stream_spec(OPENAI_RESPONSES_STREAM_PLAN_KIND)
        .expect("OpenAI Responses stream spec must be registered");
    let request_id = uuid::Uuid::new_v4().to_string();
    let Some((mut source, _candidate_count)) = build_local_same_format_stream_attempt_source(
        state,
        &parts,
        request_id.as_str(),
        decision,
        &planner_body,
        spec,
    )
    .await?
    else {
        return Ok(None);
    };

    while let Some(mut attempt) = source.next_execution_attempt().await? {
        if crate::ai_serving::normalize_api_format_alias(&attempt.plan.provider_api_format)
            != "openai:responses"
        {
            continue;
        }
        let Some(transport) = state
            .read_provider_transport_snapshot(
                &attempt.plan.provider_id,
                &attempt.plan.endpoint_id,
                &attempt.plan.key_id,
            )
            .await?
        else {
            continue;
        };
        if !crate::orchestration::responses_websocket_enabled(transport.provider.config.as_ref()) {
            continue;
        }
        let binding = ResponsesUpstreamBinding::from_plan(&attempt.plan)?;
        if expected_binding.is_some_and(|expected| expected != &binding) {
            continue;
        }
        let provider_event = finish_provider_response_create(
            attempt.plan.body.json_body.clone().ok_or_else(|| {
                GatewayError::Internal(
                    "Responses WebSocket plan did not contain a JSON provider body".to_string(),
                )
            })?,
            &event.value,
        )?;
        attempt.plan.body = aether_contracts::RequestBody::from_json(provider_event.clone());
        let provider_event = serde_json::to_string(&provider_event)
            .map_err(|error| GatewayError::Internal(error.to_string()))?;
        let redaction_session =
            redaction_slot.take_for_candidate(attempt.plan.candidate_id.as_deref());
        return Ok(Some(PlannedResponsesTurn {
            plan: attempt.plan,
            report_context: attempt.report_context,
            provider_event,
            binding,
            redaction_session,
        }));
    }
    Ok(None)
}

pub(super) fn responses_planning_parts(
    context: &crate::handlers::proxy::websocket::ingress::WebSocketRequestContext,
) -> http::request::Parts {
    responses_planning_parts_with_redaction_slot(context, RedactionSessionSlot::default())
}

fn responses_planning_parts_with_redaction_slot(
    context: &crate::handlers::proxy::websocket::ingress::WebSocketRequestContext,
    redaction_slot: RedactionSessionSlot,
) -> http::request::Parts {
    let mut request = http::Request::builder()
        .method(Method::POST)
        .uri(context.uri.clone())
        .body(())
        .expect("the authenticated Responses URI must remain valid");
    *request.headers_mut() = context.headers.clone();
    request
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    request
        .extensions_mut()
        .insert(request_origin_from_headers_and_remote_addr(
            &context.headers,
            &context.remote_addr,
        ));
    request.extensions_mut().insert(redaction_slot);
    request.into_parts().0
}

fn responses_http_planning_body(event: &Value) -> Result<Value, GatewayError> {
    let mut body = event.clone();
    let object = body.as_object_mut().ok_or_else(|| {
        GatewayError::Internal("response.create event was not a JSON object".to_string())
    })?;
    object.remove("type");
    object.remove("stream_id");
    object.remove("generate");
    object.remove("background");
    object.insert("stream".to_string(), Value::Bool(true));
    Ok(body)
}

fn finish_provider_response_create(
    mut provider_body: Value,
    client_event: &Value,
) -> Result<Value, GatewayError> {
    let object = provider_body.as_object_mut().ok_or_else(|| {
        GatewayError::Internal("Responses WebSocket provider body was not an object".to_string())
    })?;
    object.insert(
        "type".to_string(),
        Value::String("response.create".to_string()),
    );
    object.remove("stream");
    object.remove("background");
    object.remove("stream_id");
    for field in ["generate", "previous_response_id"] {
        match client_event.get(field) {
            Some(value) => {
                object.insert(field.to_string(), value.clone());
            }
            None => {
                object.remove(field);
            }
        }
    }
    Ok(provider_body)
}

#[cfg(test)]
mod tests {
    use super::{finish_provider_response_create, responses_http_planning_body};

    #[test]
    fn websocket_fields_are_removed_for_planning_and_restored_for_provider() {
        let client = serde_json::json!({
            "type": "response.create",
            "model": "public-model",
            "generate": false,
            "previous_response_id": "resp_owned",
            "input": "hello"
        });
        let planning = responses_http_planning_body(&client).expect("planning body");
        assert_eq!(planning["stream"], true);
        assert!(planning.get("type").is_none());
        assert!(planning.get("generate").is_none());

        let provider = finish_provider_response_create(
            serde_json::json!({
                "model": "provider-model",
                "stream": true,
                "input": "hello"
            }),
            &client,
        )
        .expect("provider event");
        assert_eq!(provider["type"], "response.create");
        assert_eq!(provider["model"], "provider-model");
        assert_eq!(provider["generate"], false);
        assert_eq!(provider["previous_response_id"], "resp_owned");
        assert!(provider.get("stream").is_none());
    }
}
