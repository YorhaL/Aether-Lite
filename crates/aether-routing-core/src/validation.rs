use std::collections::BTreeSet;

use thiserror::Error;

use crate::model::RoutingGroupConfig;
use crate::mutations::{validate_header_patch, validate_json_patch_operations};
use crate::{RoutingAction, RoutingRulePhase};

pub const MAX_ROUTING_ALLOWED_KEYS: usize = 512;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RoutingValidationError {
    #[error("routing rule id is empty")]
    EmptyRuleId,
    #[error("duplicate routing rule id: {0}")]
    DuplicateRuleId(String),
    #[error("routing model policy selector is empty")]
    EmptyModelSelector,
    #[error("invalid mutation action: {0}")]
    InvalidMutation(String),
    #[error("routing rule {rule_id} uses unsupported {action} action in provider_request phase")]
    ProviderRequestActionNotAllowed {
        rule_id: String,
        action: &'static str,
    },
    #[error("routing key selector {selector} contains {count} entries; maximum is {max}")]
    TooManyAllowedKeys {
        selector: String,
        count: usize,
        max: usize,
    },
}

pub fn validate_routing_group_config(
    config: &RoutingGroupConfig,
) -> Result<(), RoutingValidationError> {
    let mut rule_ids = BTreeSet::new();
    for model_policy in &config.model_policies {
        if model_policy.model.trim().is_empty() {
            return Err(RoutingValidationError::EmptyModelSelector);
        }
        validate_allowed_key_count(
            format!("model:{}", model_policy.model.trim()),
            model_policy.allowed_keys.len(),
        )?;
    }
    for rule in &config.rules {
        if rule.id.trim().is_empty() {
            return Err(RoutingValidationError::EmptyRuleId);
        }
        if !rule_ids.insert(rule.id.clone()) {
            return Err(RoutingValidationError::DuplicateRuleId(rule.id.clone()));
        }
        for action in &rule.actions {
            if rule.phase == RoutingRulePhase::ProviderRequest
                && !matches!(
                    action,
                    RoutingAction::JsonPatchBody { .. } | RoutingAction::PatchHeaders { .. }
                )
            {
                return Err(RoutingValidationError::ProviderRequestActionNotAllowed {
                    rule_id: rule.id.clone(),
                    action: routing_action_name(action),
                });
            }
            match action {
                RoutingAction::JsonPatchBody { patch } => {
                    validate_json_patch_operations(patch).map_err(|error| {
                        RoutingValidationError::InvalidMutation(error.to_string())
                    })?;
                }
                RoutingAction::PatchHeaders { patch } => {
                    validate_header_patch(patch).map_err(|error| {
                        RoutingValidationError::InvalidMutation(error.to_string())
                    })?;
                }
                RoutingAction::RestrictKeys { key_ids } => {
                    validate_allowed_key_count(format!("rule:{}", rule.id), key_ids.len())?;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn validate_allowed_key_count(
    selector: String,
    count: usize,
) -> Result<(), RoutingValidationError> {
    if count > MAX_ROUTING_ALLOWED_KEYS {
        return Err(RoutingValidationError::TooManyAllowedKeys {
            selector,
            count,
            max: MAX_ROUTING_ALLOWED_KEYS,
        });
    }
    Ok(())
}

fn routing_action_name(action: &RoutingAction) -> &'static str {
    match action {
        RoutingAction::RestrictModels { .. } => "restrict_models",
        RoutingAction::RestrictProviders { .. } => "restrict_providers",
        RoutingAction::RestrictKeys { .. } => "restrict_keys",
        RoutingAction::SetScheduling { .. } => "set_scheduling",
        RoutingAction::SetProviderPriority { .. } => "set_provider_priority",
        RoutingAction::SetKeyPriority { .. } => "set_key_priority",
        RoutingAction::JsonPatchBody { .. } => "json_patch_body",
        RoutingAction::PatchHeaders { .. } => "patch_headers",
    }
}
