use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::actions::{
    RoutingAction, RoutingRulePhase, RoutingSchedulingMode, RoutingSetPriorityMode,
};
use crate::conditions::RoutingCondition;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RoutingDefaultPolicy {
    #[serde(default)]
    pub priority_mode: RoutingSetPriorityMode,
    #[serde(default)]
    pub scheduling_mode: RoutingSchedulingMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RoutingModelPolicy {
    pub model: String,
    #[serde(default)]
    pub allowed_providers: Vec<String>,
    #[serde(default)]
    pub allowed_keys: Vec<String>,
    #[serde(default)]
    pub provider_priority_overrides: BTreeMap<String, i32>,
    #[serde(default)]
    pub key_priority_overrides: BTreeMap<String, i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingRule {
    pub id: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub phase: RoutingRulePhase,
    #[serde(default)]
    pub conditions: RoutingCondition,
    #[serde(default)]
    pub actions: Vec<RoutingAction>,
    #[serde(default)]
    pub stop_processing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RoutingGroupConfig {
    #[serde(default)]
    pub allowed_models: Vec<String>,
    #[serde(default)]
    pub default_policy: RoutingDefaultPolicy,
    #[serde(default)]
    pub model_policies: Vec<RoutingModelPolicy>,
    #[serde(default)]
    pub rules: Vec<RoutingRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingGroupRecord {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub enabled: bool,
    pub is_system_default: bool,
    pub config_json: Value,
    pub version: i64,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingGroupBindingSubject {
    User,
    ApiKey,
    UserGroup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingGroupBinding {
    pub id: String,
    pub group_id: String,
    pub subject_type: RoutingGroupBindingSubject,
    pub subject_id: String,
    pub is_default: bool,
    pub allow_explicit_select: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingGroupVersionRecord {
    pub id: String,
    pub group_id: String,
    pub version: i64,
    pub config_json: Value,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
}

fn default_true() -> bool {
    true
}
