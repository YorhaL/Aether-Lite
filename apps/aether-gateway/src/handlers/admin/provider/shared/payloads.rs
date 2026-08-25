use crate::handlers::admin::shared::{
    deserialize_optional_f64_from_number_or_string, AdminTypedObjectPatch,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct AdminProviderKeyCreateRequest {
    #[serde(default)]
    pub(crate) api_formats: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) api_key: Option<String>,
    #[serde(default)]
    pub(crate) auth_type: Option<String>,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) rate_multipliers: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) internal_priority: Option<i32>,
    #[serde(default)]
    pub(crate) rpm_limit: Option<u32>,
    #[serde(default)]
    pub(crate) concurrent_limit: Option<i32>,
    #[serde(default)]
    pub(crate) allowed_models: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) capabilities: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) cache_ttl_minutes: Option<i32>,
    #[serde(default)]
    pub(crate) max_probe_interval_minutes: Option<i32>,
    #[serde(default)]
    pub(crate) note: Option<String>,
    #[serde(default)]
    pub(crate) auto_fetch_models: Option<bool>,
    #[serde(default)]
    pub(crate) locked_models: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) model_include_patterns: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) model_exclude_patterns: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) fingerprint: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdminProviderKeyUpdateRequest {
    #[serde(default)]
    pub(crate) api_formats: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) api_key: Option<String>,
    #[serde(default)]
    pub(crate) auth_type: Option<String>,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) rate_multipliers: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) internal_priority: Option<i32>,
    #[serde(default)]
    pub(crate) global_priority_by_format: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) rpm_limit: Option<u32>,
    #[serde(default)]
    pub(crate) concurrent_limit: Option<i32>,
    #[serde(default)]
    pub(crate) allowed_models: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) capabilities: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) cache_ttl_minutes: Option<i32>,
    #[serde(default)]
    pub(crate) max_probe_interval_minutes: Option<i32>,
    #[serde(default)]
    pub(crate) is_active: Option<bool>,
    #[serde(default)]
    pub(crate) note: Option<String>,
    #[serde(default)]
    pub(crate) auto_fetch_models: Option<bool>,
    #[serde(default)]
    pub(crate) locked_models: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) model_include_patterns: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) model_exclude_patterns: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) fingerprint: Option<serde_json::Value>,
}

pub(crate) type AdminProviderKeyUpdatePatch = AdminTypedObjectPatch<AdminProviderKeyUpdateRequest>;

#[derive(Debug, Deserialize)]
pub(crate) struct AdminProviderKeyBatchUpdateRequest {
    pub(crate) key_ids: Vec<String>,
    pub(crate) patch: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdminProviderKeyBatchDeleteRequest {
    pub(crate) ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdminProviderCreateRequest {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) website: Option<String>,
    #[serde(default)]
    pub(crate) provider_priority: Option<i32>,
    #[serde(default)]
    pub(crate) responses_websocket_enabled: Option<bool>,
    #[serde(default)]
    pub(crate) is_active: Option<bool>,
    #[serde(default)]
    pub(crate) concurrent_limit: Option<i32>,
    #[serde(default)]
    pub(crate) max_retries: Option<i32>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_f64_from_number_or_string"
    )]
    pub(crate) stream_first_byte_timeout: Option<f64>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_f64_from_number_or_string"
    )]
    pub(crate) request_timeout: Option<f64>,
    #[serde(default)]
    pub(crate) failover_rules: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) config: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdminProviderUpdateRequest {
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) website: Option<String>,
    #[serde(default)]
    pub(crate) provider_priority: Option<i32>,
    #[serde(default)]
    pub(crate) responses_websocket_enabled: Option<bool>,
    #[serde(default)]
    pub(crate) is_active: Option<bool>,
    #[serde(default)]
    pub(crate) concurrent_limit: Option<i32>,
    #[serde(default)]
    pub(crate) max_retries: Option<i32>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_f64_from_number_or_string"
    )]
    pub(crate) stream_first_byte_timeout: Option<f64>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_f64_from_number_or_string"
    )]
    pub(crate) request_timeout: Option<f64>,
    #[serde(default)]
    pub(crate) failover_rules: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) config: Option<serde_json::Value>,
}

pub(crate) type AdminProviderUpdatePatch = AdminTypedObjectPatch<AdminProviderUpdateRequest>;

#[derive(Debug, Deserialize)]
pub(crate) struct AdminProviderModelCreateRequest {
    pub(crate) provider_model_name: String,
    #[serde(default)]
    pub(crate) provider_model_mappings: Option<serde_json::Value>,
    pub(crate) global_model_id: String,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_f64_from_number_or_string"
    )]
    pub(crate) price_per_request: Option<f64>,
    #[serde(default)]
    pub(crate) tiered_pricing: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) supports_vision: Option<bool>,
    #[serde(default)]
    pub(crate) supports_function_calling: Option<bool>,
    #[serde(default)]
    pub(crate) supports_streaming: Option<bool>,
    #[serde(default)]
    pub(crate) supports_extended_thinking: Option<bool>,
    #[serde(default)]
    pub(crate) supports_image_generation: Option<bool>,
    #[serde(default)]
    pub(crate) is_active: Option<bool>,
    #[serde(default)]
    pub(crate) config: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdminProviderModelUpdateRequest {
    #[serde(default)]
    pub(crate) provider_model_name: Option<String>,
    #[serde(default)]
    pub(crate) provider_model_mappings: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) global_model_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_f64_from_number_or_string"
    )]
    pub(crate) price_per_request: Option<f64>,
    #[serde(default)]
    pub(crate) tiered_pricing: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) supports_vision: Option<bool>,
    #[serde(default)]
    pub(crate) supports_function_calling: Option<bool>,
    #[serde(default)]
    pub(crate) supports_streaming: Option<bool>,
    #[serde(default)]
    pub(crate) supports_extended_thinking: Option<bool>,
    #[serde(default)]
    pub(crate) supports_image_generation: Option<bool>,
    #[serde(default)]
    pub(crate) is_active: Option<bool>,
    #[serde(default)]
    pub(crate) is_available: Option<bool>,
    #[serde(default)]
    pub(crate) config: Option<serde_json::Value>,
}

pub(crate) type AdminProviderModelUpdatePatch =
    AdminTypedObjectPatch<AdminProviderModelUpdateRequest>;

#[derive(Debug, Deserialize)]
pub(crate) struct AdminBatchAssignGlobalModelsRequest {
    pub(crate) global_model_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdminImportProviderModelsRequest {
    pub(crate) model_ids: Vec<String>,
    #[serde(default)]
    pub(crate) tiered_pricing: Option<serde_json::Value>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_f64_from_number_or_string"
    )]
    pub(crate) price_per_request: Option<f64>,
}
