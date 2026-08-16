use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::actions::{
    RoutingAction, RoutingRulePhase, RoutingSchedulingMode, RoutingSetPriorityMode,
};
use crate::conditions::RoutingConditionContext;
use crate::model::{RoutingGroupConfig, RoutingModelPolicy};
use crate::mutations::{validate_header_patch, validate_json_patch_operations, MutationPlan};
use crate::ranking::RankingOverlay;
use crate::validation::validate_routing_group_config;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RoutingPolicyError {
    #[error("routing group config is invalid: {0}")]
    InvalidConfig(String),
    #[error("model is not allowed by routing group: {0}")]
    ModelNotAllowed(String),
    #[error("mutation action is invalid: {0}")]
    InvalidMutation(String),
}

#[derive(Debug, Clone)]
pub struct RoutingPolicyInput<'a> {
    pub group_id: Option<&'a str>,
    pub group_version: Option<i64>,
    pub selection_source: &'a str,
    pub requested_model: &'a str,
    pub resolved_model: &'a str,
    pub api_format: &'a str,
    pub user_id: Option<&'a str>,
    pub api_key_id: Option<&'a str>,
    pub headers: &'a Value,
    pub body: &'a Value,
    pub phase: RoutingRulePhase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchedRoutingRule {
    pub id: String,
    pub priority: i32,
    pub phase: RoutingRulePhase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedRoutingPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_version: Option<i64>,
    pub selection_source: String,
    pub requested_model: String,
    pub resolved_model: String,
    pub priority_mode: RoutingSetPriorityMode,
    pub scheduling_mode: RoutingSchedulingMode,
    pub ranking_overlay: RankingOverlay,
    pub mutation_plan: MutationPlan,
    #[serde(default)]
    pub matched_rules: Vec<MatchedRoutingRule>,
}

pub fn resolve_routing_policy(
    config: &RoutingGroupConfig,
    input: RoutingPolicyInput<'_>,
) -> Result<ResolvedRoutingPolicy, RoutingPolicyError> {
    validate_routing_group_config(config)
        .map_err(|error| RoutingPolicyError::InvalidConfig(error.to_string()))?;

    if !model_allowed(&config.allowed_models, input.requested_model)
        && !model_allowed(&config.allowed_models, input.resolved_model)
    {
        return Err(RoutingPolicyError::ModelNotAllowed(
            input.requested_model.to_string(),
        ));
    }

    let mut policy = ResolvedRoutingPolicy {
        group_id: input.group_id.map(str::to_string),
        group_version: input.group_version,
        selection_source: input.selection_source.to_string(),
        requested_model: input.requested_model.to_string(),
        resolved_model: input.resolved_model.to_string(),
        priority_mode: config.default_policy.priority_mode,
        scheduling_mode: config.default_policy.scheduling_mode,
        ranking_overlay: RankingOverlay::default(),
        mutation_plan: MutationPlan::default(),
        matched_rules: Vec::new(),
    };

    for model_policy in matching_model_policies(config, input.requested_model, input.resolved_model)
    {
        apply_model_policy(&mut policy, model_policy);
    }

    let condition_context = RoutingConditionContext {
        model: input.requested_model,
        api_format: input.api_format,
        user_id: input.user_id,
        api_key_id: input.api_key_id,
        headers: input.headers,
        body: input.body,
    };

    let mut rules = config
        .rules
        .iter()
        .filter(|rule| rule.enabled && rule.phase == input.phase)
        .collect::<Vec<_>>();
    rules.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then(left.id.cmp(&right.id))
    });
    for rule in rules {
        if !rule.conditions.matches(&condition_context) {
            continue;
        }
        for action in &rule.actions {
            apply_action(
                &mut policy,
                action,
                input.requested_model,
                input.resolved_model,
            )?;
        }
        policy.matched_rules.push(MatchedRoutingRule {
            id: rule.id.clone(),
            priority: rule.priority,
            phase: rule.phase,
        });
        if rule.stop_processing {
            break;
        }
    }

    Ok(policy)
}

fn apply_model_policy(policy: &mut ResolvedRoutingPolicy, model_policy: &RoutingModelPolicy) {
    if !model_policy.allowed_providers.is_empty() {
        policy.ranking_overlay.allowed_providers = model_policy.allowed_providers.clone();
    }
    if !model_policy.allowed_keys.is_empty() {
        policy.ranking_overlay.allowed_keys = model_policy.allowed_keys.clone();
    }
    policy.ranking_overlay.provider_priority_overrides.extend(
        model_policy
            .provider_priority_overrides
            .iter()
            .map(|(key, value)| (key.clone(), *value)),
    );
    policy.ranking_overlay.key_priority_overrides.extend(
        model_policy
            .key_priority_overrides
            .iter()
            .map(|(key, value)| (key.clone(), *value)),
    );
}

fn apply_action(
    policy: &mut ResolvedRoutingPolicy,
    action: &RoutingAction,
    requested_model: &str,
    resolved_model: &str,
) -> Result<(), RoutingPolicyError> {
    match action {
        RoutingAction::RestrictModels { models } => {
            if !model_allowed(models, requested_model) && !model_allowed(models, resolved_model) {
                return Err(RoutingPolicyError::ModelNotAllowed(
                    requested_model.to_string(),
                ));
            }
        }
        RoutingAction::RestrictProviders { provider_ids } => {
            policy.ranking_overlay.allowed_providers = provider_ids.clone();
        }
        RoutingAction::RestrictKeys { key_ids } => {
            policy.ranking_overlay.allowed_keys = key_ids.clone();
        }
        RoutingAction::SetScheduling {
            priority_mode,
            scheduling_mode,
        } => {
            if let Some(priority_mode) = priority_mode {
                policy.priority_mode = *priority_mode;
            }
            if let Some(scheduling_mode) = scheduling_mode {
                policy.scheduling_mode = *scheduling_mode;
            }
        }
        RoutingAction::SetProviderPriority {
            provider_id,
            priority,
        } => {
            policy
                .ranking_overlay
                .provider_priority_overrides
                .insert(provider_id.clone(), *priority);
        }
        RoutingAction::SetKeyPriority { key_id, priority } => {
            policy
                .ranking_overlay
                .key_priority_overrides
                .insert(key_id.clone(), *priority);
        }
        RoutingAction::JsonPatchBody { patch } => {
            validate_json_patch_operations(patch)
                .map_err(|error| RoutingPolicyError::InvalidMutation(error.to_string()))?;
            policy.mutation_plan.body_patch.extend(patch.clone());
        }
        RoutingAction::PatchHeaders { patch } => {
            validate_header_patch(patch)
                .map_err(|error| RoutingPolicyError::InvalidMutation(error.to_string()))?;
            policy.mutation_plan.header_patch.extend(patch.clone());
        }
    }
    Ok(())
}

fn matching_model_policies<'a>(
    config: &'a RoutingGroupConfig,
    requested_model: &str,
    resolved_model: &str,
) -> Vec<&'a RoutingModelPolicy> {
    config
        .model_policies
        .iter()
        .filter(|policy| {
            model_pattern_matches(&policy.model, requested_model)
                || model_pattern_matches(&policy.model, resolved_model)
        })
        .collect()
}

fn model_allowed(patterns: &[String], requested_model: &str) -> bool {
    patterns.is_empty()
        || patterns
            .iter()
            .any(|pattern| model_pattern_matches(pattern, requested_model))
}

fn model_pattern_matches(pattern: &str, value: &str) -> bool {
    let pattern = pattern.trim();
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return value.starts_with(prefix);
    }
    pattern == value
}
