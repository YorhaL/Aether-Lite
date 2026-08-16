use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const ROUTING_PRIORITY_UNSPECIFIED: i32 = i32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKind {
    Provider,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankingOverlay {
    #[serde(default)]
    pub allowed_providers: Vec<String>,
    #[serde(default)]
    pub allowed_keys: Vec<String>,
    #[serde(default)]
    pub provider_priority_overrides: BTreeMap<String, i32>,
    #[serde(default)]
    pub key_priority_overrides: BTreeMap<String, i32>,
}

impl RankingOverlay {
    pub fn provider_priority(&self, provider_id: &str, fallback: i32) -> i32 {
        self.provider_priority_overrides
            .get(provider_id)
            .copied()
            .unwrap_or(fallback)
    }

    pub fn key_priority(&self, key_id: &str, fallback: i32) -> i32 {
        self.key_priority_overrides
            .get(key_id)
            .copied()
            .unwrap_or(fallback)
    }

    pub fn provider_priority_or_unspecified(&self, provider_id: &str) -> i32 {
        self.provider_priority_overrides
            .get(provider_id)
            .copied()
            .unwrap_or(ROUTING_PRIORITY_UNSPECIFIED)
    }

    pub fn key_priority_or_unspecified(&self, key_id: &str) -> i32 {
        self.key_priority_overrides
            .get(key_id)
            .copied()
            .unwrap_or(ROUTING_PRIORITY_UNSPECIFIED)
    }

    pub fn provider_allowed(&self, provider_id: &str) -> bool {
        self.allowed_providers.is_empty()
            || self
                .allowed_providers
                .iter()
                .any(|item| item == provider_id)
    }

    pub fn key_allowed(&self, key_id: &str) -> bool {
        self.allowed_keys.is_empty() || self.allowed_keys.iter().any(|item| item == key_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingCandidateFacts {
    pub candidate_kind: CandidateKind,
    pub provider_id: String,
    pub endpoint_id: String,
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    pub provider_priority: i32,
    pub key_priority: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingCandidateRankVector {
    pub provider_priority_before: i32,
    pub provider_priority_after: i32,
    pub key_priority_before: i32,
    pub key_priority_after: i32,
}

pub fn rank_vector_for_candidate(
    overlay: &RankingOverlay,
    facts: &RoutingCandidateFacts,
) -> RoutingCandidateRankVector {
    RoutingCandidateRankVector {
        provider_priority_before: facts.provider_priority,
        provider_priority_after: overlay
            .provider_priority(&facts.provider_id, facts.provider_priority),
        key_priority_before: facts.key_priority,
        key_priority_after: facts
            .key_id
            .as_deref()
            .map(|key_id| overlay.key_priority(key_id, facts.key_priority))
            .unwrap_or(facts.key_priority),
    }
}
