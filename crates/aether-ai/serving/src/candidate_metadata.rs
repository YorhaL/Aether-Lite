use aether_scheduler_core::SchedulerMinimalCandidateSelectionCandidate;
use serde_json::{Map, Value};

pub struct AiCandidateMetadataParts<'a> {
    pub provider_api_format: &'a str,
    pub client_api_format: &'a str,
    pub global_model_id: &'a str,
    pub global_model_name: &'a str,
    pub model_id: &'a str,
    pub selected_provider_model_name: &'a str,
    pub mapping_matched_model: Option<&'a str>,
    pub provider_name: &'a str,
    pub key_name: &'a str,
    pub extra_fields: Map<String, Value>,
}

pub fn build_ai_candidate_metadata(parts: AiCandidateMetadataParts<'_>) -> Value {
    let mut object = Map::new();
    for (key, value) in [
        ("provider_api_format", parts.provider_api_format),
        ("client_api_format", parts.client_api_format),
        ("global_model_id", parts.global_model_id),
        ("global_model_name", parts.global_model_name),
        ("model_id", parts.model_id),
        (
            "selected_provider_model_name",
            parts.selected_provider_model_name,
        ),
        ("provider_name", parts.provider_name),
        ("key_name", parts.key_name),
    ] {
        object.insert(key.to_string(), Value::String(value.to_string()));
    }
    object.insert(
        "mapping_matched_model".to_string(),
        parts
            .mapping_matched_model
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null),
    );
    object.extend(parts.extra_fields);
    Value::Object(object)
}

pub fn build_ai_candidate_metadata_from_candidate(
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
    provider_api_format: &str,
    client_api_format: &str,
    extra_fields: Map<String, Value>,
) -> Value {
    build_ai_candidate_metadata(AiCandidateMetadataParts {
        provider_api_format,
        client_api_format,
        global_model_id: candidate.global_model_id.as_str(),
        global_model_name: candidate.global_model_name.as_str(),
        model_id: candidate.model_id.as_str(),
        selected_provider_model_name: candidate.selected_provider_model_name.as_str(),
        mapping_matched_model: candidate.mapping_matched_model.as_deref(),
        provider_name: candidate.provider_name.as_str(),
        key_name: candidate.key_name.as_str(),
        extra_fields,
    })
}
