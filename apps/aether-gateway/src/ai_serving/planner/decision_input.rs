use std::collections::BTreeMap;
use std::time::Duration;

use aether_ai_serving::{run_ai_authenticated_decision_input, AiAuthenticatedDecisionInputPort};
use aether_routing_core::{
    rank_vector_for_candidate, CandidateKind, ResolvedRoutingPolicy, RoutingCandidateFacts,
    RoutingCandidateTrace, RoutingDecisionTrace, RoutingRulePhase,
};
use aether_scheduler_core::ClientSessionAffinity;
use async_trait::async_trait;
use http::StatusCode;
use http::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{json, Value};

use crate::ai_serving::planner::common::extract_standard_requested_model;
use crate::ai_serving::{
    ClientSurface, ExecutionRuntimeAuthContext, GatewayAuthApiKeySnapshot,
    GatewayCredentialCarrier, GatewayProviderTransportSnapshot, PlannerAppState,
};
use crate::cache::CacheLoadObserver;
use crate::client_session_affinity::client_session_affinity_from_api_request;
use crate::clock::current_unix_secs;
use crate::routing::{
    apply_routing_mutation_plan, build_routing_trace_seed, resolve_gateway_routing_policy,
    resolve_gateway_static_default_routing_policy, select_gateway_routing_group,
    GatewayRoutingPolicyInput, GatewayRoutingSelectionError, GatewayRoutingSelectionInput,
    GatewayStaticRoutingPolicyInput, ROUTING_GROUP_HEADER,
};
use crate::stage_metrics::observe_gateway_stage_ms;
use crate::{AiExecutionDecision, AppState, GatewayError};

// Keep normal freshness bounded for cross-node routing changes. Stale values
// are served while a single background refresh updates the cache.
const ROUTING_GROUP_SELECTION_CACHE_TTL: Duration = Duration::from_secs(30);
const ROUTING_GROUP_SELECTION_CACHE_STALE_TTL: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub(crate) struct ResolvedLocalDecisionAuthInput {
    pub(crate) auth_context: ExecutionRuntimeAuthContext,
    pub(crate) auth_snapshot: GatewayAuthApiKeySnapshot,
    pub(crate) required_capabilities: Option<serde_json::Value>,
    pub(crate) model_directive_policy: crate::system_features::ModelDirectivePolicySnapshot,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalRequestedModelDecisionInput {
    pub(crate) auth_context: ExecutionRuntimeAuthContext,
    pub(crate) requested_model: String,
    pub(crate) auth_snapshot: GatewayAuthApiKeySnapshot,
    pub(crate) required_capabilities: Option<serde_json::Value>,
    pub(crate) request_auth_channel: Option<String>,
    pub(crate) client_surface: Option<ClientSurface>,
    pub(crate) gateway_credential_carrier: Option<GatewayCredentialCarrier>,
    pub(crate) client_session_affinity: Option<ClientSessionAffinity>,
    pub(crate) original_client_session_id: Option<String>,
    pub(crate) routing_policy: Option<ResolvedRoutingPolicy>,
    pub(crate) routing_trace_seed: Option<RoutingDecisionTrace>,
    pub(crate) routing_context: Option<LocalRoutingRequestContext>,
    pub(crate) model_directive_policy: crate::system_features::ModelDirectivePolicySnapshot,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalAuthenticatedDecisionInput {
    pub(crate) auth_context: ExecutionRuntimeAuthContext,
    pub(crate) auth_snapshot: GatewayAuthApiKeySnapshot,
    pub(crate) required_capabilities: Option<serde_json::Value>,
    pub(crate) client_session_affinity: Option<ClientSessionAffinity>,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalRoutingRequestContext {
    pub(crate) group_id: Option<String>,
    pub(crate) group_version: Option<i64>,
    pub(crate) group_config_json: Value,
    pub(crate) selection_source: String,
    pub(crate) client_api_format: String,
    pub(crate) effective_body_json: Value,
    pub(crate) effective_headers: HeaderMap,
}

impl LocalRequestedModelDecisionInput {
    pub(crate) fn effective_body_json<'a>(&'a self, fallback: &'a Value) -> &'a Value {
        self.routing_context
            .as_ref()
            .map(|context| &context.effective_body_json)
            .unwrap_or(fallback)
    }

    pub(crate) fn effective_headers<'a>(&'a self, fallback: &'a HeaderMap) -> &'a HeaderMap {
        self.routing_context
            .as_ref()
            .map(|context| &context.effective_headers)
            .unwrap_or(fallback)
    }
}

pub(crate) fn apply_provider_request_routing_policy_to_decision(
    input: &LocalRequestedModelDecisionInput,
    decision: &mut AiExecutionDecision,
    _transport: Option<&GatewayProviderTransportSnapshot>,
) -> Result<(), GatewayError> {
    let Some(context) = input.routing_context.as_ref() else {
        return Ok(());
    };

    let provider_api_format = decision
        .provider_api_format
        .as_deref()
        .unwrap_or(context.client_api_format.as_str());
    let resolved_model = decision
        .mapped_model
        .as_deref()
        .or(decision.model_name.as_deref())
        .unwrap_or(input.requested_model.as_str());
    let original_provider_request_body = decision.provider_request_body.clone();
    let mut provider_request_body = original_provider_request_body
        .clone()
        .unwrap_or(serde_json::Value::Null);
    let mut provider_headers = btree_headers_to_header_map(&decision.provider_request_headers)?;
    let provider_headers_json = headers_to_routing_value(&provider_headers);
    let policy = resolve_gateway_routing_policy(GatewayRoutingPolicyInput {
        group_id: context.group_id.as_deref(),
        group_version: context.group_version,
        group_config_json: &context.group_config_json,
        selection_source: context.selection_source.as_str(),
        requested_model: input.requested_model.as_str(),
        resolved_model,
        api_format: provider_api_format,
        user_id: Some(input.auth_context.user_id.as_str()),
        api_key_id: Some(input.auth_context.api_key_id.as_str()),
        headers: &provider_headers_json,
        body: &provider_request_body,
        phase: RoutingRulePhase::ProviderRequest,
    })?;
    ensure_report_context_routing_trace(input, decision, &policy);
    if policy.mutation_plan.is_empty() {
        return Ok(());
    }
    if original_provider_request_body.is_none() && !policy.mutation_plan.body_patch.is_empty() {
        return Err(GatewayError::Client {
            status: StatusCode::BAD_REQUEST,
            message: "routing provider_request body patch cannot be applied to a binary or empty upstream body".to_string(),
        });
    }

    apply_routing_mutation_plan(
        &mut provider_request_body,
        &mut provider_headers,
        &policy.mutation_plan,
    )?;
    decision.provider_request_headers = header_map_to_btree_headers(&provider_headers);
    if original_provider_request_body.is_some() {
        decision.provider_request_body = Some(provider_request_body);
    }
    update_report_context_provider_request_mutation(decision, &policy);
    Ok(())
}

struct GatewayAuthenticatedDecisionInputPort<'a> {
    state: PlannerAppState<'a>,
    now_unix_secs: u64,
    model_directive_policy: &'a crate::system_features::ModelDirectivePolicySnapshot,
    model_directive_base_model: Option<String>,
}

#[async_trait]
impl AiAuthenticatedDecisionInputPort for GatewayAuthenticatedDecisionInputPort<'_> {
    type AuthContext = ExecutionRuntimeAuthContext;
    type AuthSnapshot = GatewayAuthApiKeySnapshot;
    type RequiredCapabilities = serde_json::Value;
    type ResolvedInput = ResolvedLocalDecisionAuthInput;
    type Error = GatewayError;

    async fn read_auth_snapshot(
        &self,
        auth_context: &Self::AuthContext,
    ) -> Result<Option<Self::AuthSnapshot>, Self::Error> {
        self.state
            .read_auth_api_key_snapshot(
                &auth_context.user_id,
                &auth_context.api_key_id,
                self.now_unix_secs,
            )
            .await
    }

    async fn resolve_required_capabilities(
        &self,
        auth_context: &Self::AuthContext,
        requested_model: Option<&str>,
        explicit_required_capabilities: Option<&Self::RequiredCapabilities>,
    ) -> Result<Option<Self::RequiredCapabilities>, Self::Error> {
        Ok(self
            .state
            .resolve_request_candidate_required_capabilities(
                &auth_context.user_id,
                &auth_context.api_key_id,
                requested_model,
                explicit_required_capabilities,
                self.model_directive_base_model.as_deref(),
            )
            .await)
    }

    fn build_resolved_input(
        &self,
        auth_context: Self::AuthContext,
        auth_snapshot: Self::AuthSnapshot,
        required_capabilities: Option<Self::RequiredCapabilities>,
    ) -> Self::ResolvedInput {
        ResolvedLocalDecisionAuthInput {
            auth_context,
            auth_snapshot,
            required_capabilities,
            model_directive_policy: self.model_directive_policy.clone(),
        }
    }
}

pub(crate) fn build_local_requested_model_decision_input(
    resolved_input: ResolvedLocalDecisionAuthInput,
    requested_model: String,
) -> LocalRequestedModelDecisionInput {
    LocalRequestedModelDecisionInput {
        auth_context: resolved_input.auth_context,
        requested_model,
        auth_snapshot: resolved_input.auth_snapshot,
        required_capabilities: resolved_input.required_capabilities,
        request_auth_channel: None,
        client_surface: None,
        gateway_credential_carrier: None,
        client_session_affinity: None,
        original_client_session_id: None,
        routing_policy: None,
        routing_trace_seed: None,
        routing_context: None,
        model_directive_policy: resolved_input.model_directive_policy,
    }
}

pub(crate) async fn attach_routing_policy_to_local_requested_model_input(
    state: &AppState,
    parts: &http::request::Parts,
    input: &mut LocalRequestedModelDecisionInput,
    body_json: &Value,
    client_api_format: &str,
) -> Result<(), GatewayError> {
    input.original_client_session_id = routing_header_value_str(&parts.headers, "session-id")
        .or_else(|| routing_header_value_str(&parts.headers, "session_id"));
    let explicit_group = routing_header_value_str(&parts.headers, ROUTING_GROUP_HEADER);
    let selected_group = match state.routing_group_read_repository() {
        Some(repository) => {
            // Explicit non-default groups are authorized against principal
            // bindings, so both selection and its cache key must retain the
            // caller context. Only the implicit no-binding system-default
            // path is global and can skip the membership lookup.
            let principal_context_required = if explicit_group.is_some() {
                true
            } else {
                repository
                    .has_any_routing_group_binding()
                    .await
                    .map_err(|error| {
                        routing_selection_error(GatewayRoutingSelectionError::Repository(
                            error.to_string(),
                        ))
                    })?
            };
            let user_group_ids = if principal_context_required {
                let user_groups_lookup_started_at = std::time::Instant::now();
                let user_groups = state
                    .list_user_groups_for_user(&input.auth_context.user_id)
                    .await;
                observe_gateway_stage_ms(
                    "routing_user_groups_lookup",
                    user_groups_lookup_started_at.elapsed().as_millis() as u64,
                );
                user_groups?
                    .into_iter()
                    .map(|group| group.into_stored().id)
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            let selection_user_id =
                principal_context_required.then(|| input.auth_context.user_id.clone());
            let selection_api_key_id =
                principal_context_required.then(|| input.auth_context.api_key_id.clone());
            let selection_cache_key = routing_group_selection_cache_key(
                explicit_group.as_deref(),
                selection_user_id.as_deref(),
                selection_api_key_id.as_deref(),
                &user_group_ids,
            );
            let group_selection_started_at = std::time::Instant::now();
            let selection = state
                .routing_group_selection_cache
                .get_or_load_once_stale_while_revalidating(
                    selection_cache_key,
                    ROUTING_GROUP_SELECTION_CACHE_TTL,
                    ROUTING_GROUP_SELECTION_CACHE_STALE_TTL,
                    || async {
                        let selection_load_started_at = std::time::Instant::now();
                        let selection = select_gateway_routing_group(
                            repository.as_ref(),
                            GatewayRoutingSelectionInput {
                                explicit_group: explicit_group.as_deref(),
                                user_id: selection_user_id.as_deref(),
                                api_key_id: selection_api_key_id.as_deref(),
                                user_group_ids: &user_group_ids,
                            },
                        )
                        .await
                        .map_err(routing_selection_error)?;
                        observe_gateway_stage_ms(
                            "routing_group_selection_load",
                            selection_load_started_at.elapsed().as_millis() as u64,
                        );
                        Ok::<_, GatewayError>(Some(selection))
                    },
                    || {
                        let repository = repository.clone();
                        let explicit_group = explicit_group.clone();
                        let user_id = selection_user_id.clone();
                        let api_key_id = selection_api_key_id.clone();
                        let user_group_ids = user_group_ids.clone();
                        async move {
                            let selection_load_started_at = std::time::Instant::now();
                            let selection = select_gateway_routing_group(
                                repository.as_ref(),
                                GatewayRoutingSelectionInput {
                                    explicit_group: explicit_group.as_deref(),
                                    user_id: user_id.as_deref(),
                                    api_key_id: api_key_id.as_deref(),
                                    user_group_ids: &user_group_ids,
                                },
                            )
                            .await
                            .map_err(routing_selection_error)?;
                            observe_gateway_stage_ms(
                                "routing_group_selection_load",
                                selection_load_started_at.elapsed().as_millis() as u64,
                            );
                            Ok::<_, GatewayError>(Some(selection))
                        }
                    },
                    CacheLoadObserver::default(),
                )
                .await?
                .unwrap_or_default();
            observe_gateway_stage_ms(
                "routing_group_selection",
                group_selection_started_at.elapsed().as_millis() as u64,
            );
            selection.group.map(|group| {
                (
                    Some(group.id),
                    Some(group.version),
                    group.config_json,
                    selection.source,
                )
            })
        }
        None => {
            if explicit_group
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
            {
                return Err(routing_selection_error(
                    GatewayRoutingSelectionError::NotFound(explicit_group.unwrap_or_default()),
                ));
            }
            None
        }
    };

    let Some((group_id, group_version, group_config_json, selection_source)) = selected_group
    else {
        input.client_session_affinity = client_session_affinity_from_api_request(
            client_api_format,
            &parts.headers,
            Some(body_json),
        );
        input.routing_policy = None;
        input.routing_trace_seed = None;
        input.routing_context = None;
        return Ok(());
    };

    if try_attach_static_default_routing_policy_to_input(
        input,
        parts,
        body_json,
        client_api_format,
        group_id.as_deref(),
        group_version,
        &group_config_json,
        selection_source.as_str(),
    )? {
        return Ok(());
    }

    let headers_json = headers_to_routing_value(&parts.headers);
    let policy_resolve_started_at = std::time::Instant::now();
    let policy = resolve_gateway_routing_policy(GatewayRoutingPolicyInput {
        group_id: group_id.as_deref(),
        group_version,
        group_config_json: &group_config_json,
        selection_source: selection_source.as_str(),
        requested_model: input.requested_model.as_str(),
        resolved_model: input.requested_model.as_str(),
        api_format: client_api_format,
        user_id: Some(input.auth_context.user_id.as_str()),
        api_key_id: Some(input.auth_context.api_key_id.as_str()),
        headers: &headers_json,
        body: body_json,
        phase: RoutingRulePhase::ClientRequest,
    })?;
    observe_gateway_stage_ms(
        "routing_policy_resolve",
        policy_resolve_started_at.elapsed().as_millis() as u64,
    );
    let mut effective_body_json = body_json.clone();
    let mut effective_headers = parts.headers.clone();
    let mutation_apply_started_at = std::time::Instant::now();
    apply_routing_mutation_plan(
        &mut effective_body_json,
        &mut effective_headers,
        &policy.mutation_plan,
    )?;
    observe_gateway_stage_ms(
        "routing_mutation_apply",
        mutation_apply_started_at.elapsed().as_millis() as u64,
    );

    let mut requested_model_changed = false;
    if let Some(mut mutated_model) = extract_standard_requested_model(&effective_body_json) {
        mutated_model = mutated_model.trim().to_string();
        if !mutated_model.is_empty() && mutated_model != input.requested_model {
            input.requested_model = mutated_model;
            requested_model_changed = true;
        }
    }
    if requested_model_changed {
        let model_directive_resolution = input
            .model_directive_policy
            .resolve_reasoning(client_api_format, Some(input.requested_model.as_str()));
        input.required_capabilities = PlannerAppState::new(state)
            .resolve_request_candidate_required_capabilities(
                &input.auth_context.user_id,
                &input.auth_context.api_key_id,
                Some(input.requested_model.as_str()),
                input.required_capabilities.as_ref(),
                model_directive_resolution.base_model(),
            )
            .await;
    }

    let effective_headers_json = headers_to_routing_value(&effective_headers);
    input.client_session_affinity = client_session_affinity_from_api_request(
        client_api_format,
        &effective_headers,
        Some(&effective_body_json),
    );
    let final_policy_resolve_started_at = std::time::Instant::now();
    let mut final_policy = resolve_gateway_routing_policy(GatewayRoutingPolicyInput {
        group_id: group_id.as_deref(),
        group_version,
        group_config_json: &group_config_json,
        selection_source: selection_source.as_str(),
        requested_model: input.requested_model.as_str(),
        resolved_model: input.requested_model.as_str(),
        api_format: client_api_format,
        user_id: Some(input.auth_context.user_id.as_str()),
        api_key_id: Some(input.auth_context.api_key_id.as_str()),
        headers: &effective_headers_json,
        body: &effective_body_json,
        phase: RoutingRulePhase::ClientRequest,
    })?;
    observe_gateway_stage_ms(
        "routing_policy_resolve",
        final_policy_resolve_started_at.elapsed().as_millis() as u64,
    );
    final_policy.mutation_plan = policy.mutation_plan.clone();
    input.routing_trace_seed = Some(build_routing_trace_seed(&final_policy, client_api_format));
    input.routing_policy = Some(final_policy);
    input.routing_context = Some(LocalRoutingRequestContext {
        group_id,
        group_version,
        group_config_json,
        selection_source,
        client_api_format: client_api_format.to_string(),
        effective_body_json,
        effective_headers,
    });
    Ok(())
}

fn try_attach_static_default_routing_policy_to_input(
    input: &mut LocalRequestedModelDecisionInput,
    parts: &http::request::Parts,
    body_json: &Value,
    client_api_format: &str,
    group_id: Option<&str>,
    group_version: Option<i64>,
    group_config_json: &Value,
    selection_source: &str,
) -> Result<bool, GatewayError> {
    let static_policy_resolve_started_at = std::time::Instant::now();
    let Some(policy) =
        resolve_gateway_static_default_routing_policy(GatewayStaticRoutingPolicyInput {
            group_id,
            group_version,
            group_config_json,
            selection_source,
            requested_model: input.requested_model.as_str(),
            resolved_model: input.requested_model.as_str(),
        })?
    else {
        observe_gateway_stage_ms(
            "routing_static_policy_resolve",
            static_policy_resolve_started_at.elapsed().as_millis() as u64,
        );
        return Ok(false);
    };
    observe_gateway_stage_ms(
        "routing_static_policy_resolve",
        static_policy_resolve_started_at.elapsed().as_millis() as u64,
    );

    input.client_session_affinity = client_session_affinity_from_api_request(
        client_api_format,
        &parts.headers,
        Some(body_json),
    );
    input.routing_trace_seed = Some(build_routing_trace_seed(&policy, client_api_format));
    input.routing_policy = Some(policy);
    input.routing_context = None;
    Ok(true)
}

pub(crate) fn build_local_authenticated_decision_input(
    resolved_input: ResolvedLocalDecisionAuthInput,
) -> LocalAuthenticatedDecisionInput {
    LocalAuthenticatedDecisionInput {
        auth_context: resolved_input.auth_context,
        auth_snapshot: resolved_input.auth_snapshot,
        required_capabilities: resolved_input.required_capabilities,
        client_session_affinity: None,
    }
}

pub(crate) async fn resolve_local_authenticated_decision_input(
    state: &AppState,
    auth_context: ExecutionRuntimeAuthContext,
    requested_model: Option<&str>,
    requested_model_api_format: Option<&str>,
    explicit_required_capabilities: Option<&serde_json::Value>,
    model_directive_policy: &crate::system_features::ModelDirectivePolicySnapshot,
) -> Result<Option<ResolvedLocalDecisionAuthInput>, GatewayError> {
    let model_directive_base_model = match (requested_model, requested_model_api_format) {
        (Some(model), Some(api_format)) => model_directive_policy
            .resolve_reasoning(api_format, Some(model))
            .base_model()
            .map(str::to_owned),
        _ => None,
    };
    let port = GatewayAuthenticatedDecisionInputPort {
        state: PlannerAppState::new(state),
        now_unix_secs: current_unix_secs(),
        model_directive_policy,
        model_directive_base_model,
    };

    run_ai_authenticated_decision_input(
        &port,
        auth_context,
        requested_model,
        explicit_required_capabilities,
    )
    .await
}

fn routing_selection_error(error: GatewayRoutingSelectionError) -> GatewayError {
    match error {
        GatewayRoutingSelectionError::Repository(message) => {
            GatewayError::Internal(format!("routing group repository lookup failed: {message}"))
        }
        error => GatewayError::Client {
            status: StatusCode::FORBIDDEN,
            message: error.to_string(),
        },
    }
}

fn headers_to_routing_value(headers: &http::HeaderMap) -> Value {
    let mut object = serde_json::Map::new();
    for (name, value) in headers {
        if let Ok(value) = value.to_str() {
            object.insert(name.as_str().to_ascii_lowercase(), json!(value));
        }
    }
    Value::Object(object)
}

fn routing_header_value_str(headers: &http::HeaderMap, key: &str) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn routing_group_selection_cache_key(
    explicit_group: Option<&str>,
    user_id: Option<&str>,
    api_key_id: Option<&str>,
    user_group_ids: &[String],
) -> String {
    let groups = user_group_ids
        .iter()
        .map(|value| escape_cache_key_part(value))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "v1|explicit={}|user={}|api_key={}|groups={}",
        escape_cache_key_part(explicit_group.unwrap_or_default()),
        escape_cache_key_part(user_id.unwrap_or_default()),
        escape_cache_key_part(api_key_id.unwrap_or_default()),
        groups
    )
}

fn escape_cache_key_part(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('|', "%7C")
        .replace(',', "%2C")
}

fn btree_headers_to_header_map(
    headers: &BTreeMap<String, String>,
) -> Result<HeaderMap, GatewayError> {
    let mut output = HeaderMap::new();
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|err| GatewayError::Client {
            status: StatusCode::BAD_REQUEST,
            message: format!("invalid provider request header name in routing mutation: {err}"),
        })?;
        let value = HeaderValue::from_str(value).map_err(|err| GatewayError::Client {
            status: StatusCode::BAD_REQUEST,
            message: format!("invalid provider request header value in routing mutation: {err}"),
        })?;
        output.insert(name, value);
    }
    Ok(output)
}

fn header_map_to_btree_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

fn update_report_context_provider_request_mutation(
    decision: &mut AiExecutionDecision,
    policy: &ResolvedRoutingPolicy,
) {
    let Some(serde_json::Value::Object(object)) = decision.report_context.as_mut() else {
        return;
    };
    let body_paths = policy
        .mutation_plan
        .body_patch
        .iter()
        .map(|operation| operation.path().to_string())
        .collect::<Vec<_>>();
    let header_names = policy
        .mutation_plan
        .header_patch
        .iter()
        .map(|operation| operation.name().to_string())
        .collect::<Vec<_>>();
    let trace_patch_summary = serde_json::json!({
        "body_paths": body_paths,
        "header_names": header_names,
    });
    if let Some(serde_json::Value::Object(routing_trace)) = object.get_mut("routing_trace") {
        routing_trace.insert(
            "provider_request_patch_summary".to_string(),
            trace_patch_summary.clone(),
        );
    }
    object.insert(
        "provider_request_headers".to_string(),
        serde_json::json!(decision.provider_request_headers),
    );
    object.insert(
        "routing_provider_request_patch_summary".to_string(),
        serde_json::json!({
            "body_paths": trace_patch_summary["body_paths"].clone(),
            "header_names": trace_patch_summary["header_names"].clone(),
            "matched_rules": policy
                .matched_rules
                .iter()
                .map(|rule| rule.id.clone())
                .collect::<Vec<_>>()
        }),
    );
}

fn ensure_report_context_routing_trace(
    input: &LocalRequestedModelDecisionInput,
    decision: &mut AiExecutionDecision,
    policy: &ResolvedRoutingPolicy,
) {
    let Some(serde_json::Value::Object(object)) = decision.report_context.as_mut() else {
        return;
    };
    if object.get("routing_trace").is_some() {
        return;
    }

    let client_api_format = decision
        .client_api_format
        .as_deref()
        .or_else(|| {
            input
                .routing_context
                .as_ref()
                .map(|context| context.client_api_format.as_str())
        })
        .unwrap_or_default();
    let mut trace = input
        .routing_trace_seed
        .clone()
        .unwrap_or_else(|| build_routing_trace_seed(policy, client_api_format));

    let candidate_kind = CandidateKind::Provider;
    let provider_id = decision.provider_id.clone().unwrap_or_default();
    let endpoint_id = decision.endpoint_id.clone().unwrap_or_default();
    let model_id = object
        .get("model_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| decision.mapped_model.clone())
        .or_else(|| decision.model_name.clone())
        .unwrap_or_else(|| input.requested_model.clone());
    let key_id = decision.key_id.clone();
    let provider_priority = object
        .get("provider_priority")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or_default();
    let key_priority = object
        .get("priority_slot")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or_default();
    trace.global_candidates.push(RoutingCandidateTrace {
        candidate_kind,
        provider_id: provider_id.clone(),
        endpoint_id,
        model_id: model_id.clone(),
        key_id: key_id.clone(),
        ranking_vector: rank_vector_for_candidate(
            &policy.ranking_overlay,
            &RoutingCandidateFacts {
                candidate_kind,
                provider_id: provider_id.clone(),
                endpoint_id: decision.endpoint_id.clone().unwrap_or_default(),
                model_id,
                key_id,
                provider_priority,
                key_priority,
            },
        ),
        skip_reason: None,
        selected_order: object
            .get("candidate_index")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
    });

    object.insert("routing_trace".to_string(), serde_json::json!(trace));
}
