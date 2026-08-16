use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::actions::{RoutingSchedulingMode, RoutingSetPriorityMode};
use crate::ranking::{CandidateKind, RoutingCandidateRankVector};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingPatchSummary {
    #[serde(default)]
    pub body_paths: Vec<String>,
    #[serde(default)]
    pub header_names: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingCandidateTrace {
    pub candidate_kind: CandidateKind,
    pub provider_id: String,
    pub endpoint_id: String,
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    pub ranking_vector: RoutingCandidateRankVector,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_order: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingRuntimeFacts {
    #[serde(default)]
    pub cache_affinity_hit: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sticky_key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_balance_seed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduler_mode: Option<RoutingSchedulingMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority_mode: Option<RoutingSetPriorityMode>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingDecisionTrace {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_version: Option<i64>,
    pub selection_source: String,
    #[serde(default)]
    pub selected_rules: Vec<String>,
    pub original_model: String,
    pub resolved_model: String,
    pub client_api_format: String,
    #[serde(default)]
    pub client_request_patch_summary: RoutingPatchSummary,
    #[serde(default)]
    pub provider_request_patch_summary: RoutingPatchSummary,
    #[serde(default)]
    pub global_candidates: Vec<RoutingCandidateTrace>,
    #[serde(default)]
    pub runtime_facts: RoutingRuntimeFacts,
}

impl RoutingDecisionTrace {
    pub fn to_extra_data_value(&self) -> Value {
        serde_json::json!({ "routing_trace": self })
    }
}
