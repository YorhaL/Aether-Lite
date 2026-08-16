use crate::handlers::admin::shared::unix_secs_to_rfc3339;
use aether_data_contracts::repository::global_models::StoredAdminProviderModel;

const EMBEDDING_API_FORMATS: &[&str] = &["openai:embedding", "gemini:embedding"];

pub(crate) fn model_tiered_pricing_first_tier_value(
    tiered_pricing: Option<&serde_json::Value>,
    field_name: &str,
) -> Option<f64> {
    tiered_pricing
        .and_then(|value| value.get("tiers"))
        .and_then(serde_json::Value::as_array)
        .and_then(|tiers| tiers.first())
        .and_then(|tier| tier.get(field_name))
        .and_then(serde_json::Value::as_f64)
}

fn model_effective_capability(
    explicit: Option<bool>,
    global_model_config: Option<&serde_json::Value>,
    config_key: &str,
) -> bool {
    explicit.unwrap_or_else(|| {
        global_model_config
            .and_then(|value| value.get(config_key))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    })
}

fn value_contains_string(value: &serde_json::Value, expected: &str) -> bool {
    match value {
        serde_json::Value::String(value) => value.trim().eq_ignore_ascii_case(expected),
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| value_contains_string(value, expected)),
        serde_json::Value::Object(object) => object
            .values()
            .any(|value| value_contains_string(value, expected)),
        _ => false,
    }
}

fn value_has_true_key(value: &serde_json::Value, key: &str) -> bool {
    value
        .as_object()
        .and_then(|object| object.get(key))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn value_contains_embedding_metadata(value: &serde_json::Value) -> bool {
    value_has_true_key(value, "embedding")
        || value_contains_string(value, "embedding")
        || EMBEDDING_API_FORMATS
            .iter()
            .any(|api_format| value_contains_string(value, api_format))
}

fn model_effective_embedding_capability(model: &StoredAdminProviderModel) -> bool {
    model
        .config
        .as_ref()
        .is_some_and(value_contains_embedding_metadata)
        || model
            .global_model_supported_capabilities
            .as_ref()
            .is_some_and(value_contains_embedding_metadata)
        || model
            .global_model_config
            .as_ref()
            .is_some_and(value_contains_embedding_metadata)
}

pub(crate) fn timestamp_or_now(value: Option<u64>, now_unix_secs: u64) -> serde_json::Value {
    unix_secs_to_rfc3339(value.unwrap_or(now_unix_secs))
        .map(serde_json::Value::String)
        .unwrap_or(serde_json::Value::Null)
}

pub(crate) fn normalize_required_trimmed_string(
    value: &str,
    field_name: &str,
) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{field_name} 不能为空"));
    }
    Ok(trimmed.to_string())
}

pub(crate) fn normalize_optional_price(
    value: Option<f64>,
    field_name: &str,
) -> Result<Option<f64>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if !value.is_finite() || value < 0.0 {
        return Err(format!("{field_name} 必须是非负数"));
    }
    Ok(Some(value))
}

pub(crate) fn admin_provider_model_effective_input_price(
    model: &StoredAdminProviderModel,
) -> Option<f64> {
    model_tiered_pricing_first_tier_value(model.tiered_pricing.as_ref(), "input_price_per_1m")
        .or_else(|| {
            model_tiered_pricing_first_tier_value(
                model.global_model_default_tiered_pricing.as_ref(),
                "input_price_per_1m",
            )
        })
}

pub(crate) fn admin_provider_model_effective_output_price(
    model: &StoredAdminProviderModel,
) -> Option<f64> {
    model_tiered_pricing_first_tier_value(model.tiered_pricing.as_ref(), "output_price_per_1m")
        .or_else(|| {
            model_tiered_pricing_first_tier_value(
                model.global_model_default_tiered_pricing.as_ref(),
                "output_price_per_1m",
            )
        })
}

pub(crate) fn admin_provider_model_effective_capability(
    model: &StoredAdminProviderModel,
    capability: &str,
) -> bool {
    match capability {
        "vision" => model_effective_capability(
            model.supports_vision,
            model.global_model_config.as_ref(),
            "vision",
        ),
        "function_calling" => model_effective_capability(
            model.supports_function_calling,
            model.global_model_config.as_ref(),
            "function_calling",
        ),
        "streaming" => model_effective_capability(
            model.supports_streaming,
            model.global_model_config.as_ref(),
            "streaming",
        ),
        "extended_thinking" => model_effective_capability(
            model.supports_extended_thinking,
            model.global_model_config.as_ref(),
            "extended_thinking",
        ),
        "image_generation" => model_effective_capability(
            model.supports_image_generation,
            model.global_model_config.as_ref(),
            "image_generation",
        ),
        "embedding" => model_effective_embedding_capability(model),
        _ => false,
    }
}
