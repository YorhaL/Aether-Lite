use std::collections::BTreeSet;

pub(crate) fn retain_supported_provider_config(
    config: &mut serde_json::Map<String, serde_json::Value>,
) {
    config.retain(|key, _| {
        matches!(
            key.as_str(),
            "chat_pii_redaction" | "failover_rules" | "responses_websocket"
        )
    });
}

pub(crate) fn normalize_api_format_list(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::new();
    for value in values {
        let canonical = crate::ai_serving::normalize_api_format_alias(&value);
        if seen.insert(canonical.clone()) {
            normalized.push(canonical);
        }
    }
    normalized
}

pub(crate) fn normalize_api_format_json_object_keys(
    value: Option<serde_json::Value>,
    field_name: &str,
) -> Result<Option<serde_json::Value>, String> {
    let Some(value) = normalize_json_like_object(value, field_name)? else {
        return Ok(None);
    };
    let serde_json::Value::Object(map) = value else {
        return Ok(Some(value));
    };
    let mut normalized = serde_json::Map::new();
    for (key, value) in map {
        let canonical = crate::ai_serving::normalize_api_format_alias(&key);
        normalized.insert(canonical, value);
    }
    Ok(Some(serde_json::Value::Object(normalized)))
}

pub(crate) fn normalize_rate_multipliers(
    value: Option<serde_json::Value>,
) -> Result<Option<serde_json::Value>, String> {
    let Some(value) = normalize_json_like_object(value, "rate_multipliers")? else {
        return Ok(None);
    };
    let serde_json::Value::Object(map) = value else {
        return Ok(Some(value));
    };
    let mut normalized = serde_json::Map::new();
    for (key, value) in map {
        let canonical = crate::ai_serving::normalize_api_format_alias(&key);
        let multiplier = value
            .as_f64()
            .filter(|value| value.is_finite() && *value >= 0.0)
            .ok_or_else(|| format!("rate_multipliers.{canonical} 必须是大于或等于 0 的有限数值"))?;
        normalized.insert(canonical, serde_json::Value::from(multiplier));
    }
    if normalized.is_empty() {
        Ok(None)
    } else {
        Ok(Some(serde_json::Value::Object(normalized)))
    }
}

pub(crate) fn normalize_auth_type(value: Option<&str>) -> Result<String, String> {
    let auth_type = value.unwrap_or("api_key").trim().to_ascii_lowercase();
    match auth_type.as_str() {
        "api_key" | "bearer" => Ok(auth_type),
        _ => Err("auth_type 必须是 api_key 或 bearer".to_string()),
    }
}

pub(crate) fn normalize_max_probe_interval_minutes(value: i32) -> Result<i32, String> {
    if (0..=32).contains(&value) {
        Ok(value)
    } else {
        Err("max_probe_interval_minutes 必须在 0 到 32 之间".to_string())
    }
}

pub(crate) fn normalize_chat_pii_redaction_config(
    value: Option<serde_json::Value>,
) -> Result<Option<serde_json::Value>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Object(mut map) => {
            if map.len() != 1 || !map.contains_key("enabled") {
                return Err("chat_pii_redaction 仅支持 enabled 布尔配置".to_string());
            }
            let enabled = map
                .remove("enabled")
                .and_then(|value| value.as_bool())
                .ok_or_else(|| "chat_pii_redaction.enabled 必须是布尔值".to_string())?;
            Ok(Some(serde_json::json!({ "enabled": enabled })))
        }
        _ => Err("chat_pii_redaction 必须是 JSON 对象".to_string()),
    }
}

pub(crate) fn set_responses_websocket_enabled(
    config: &mut serde_json::Map<String, serde_json::Value>,
    enabled: bool,
) -> Result<(), String> {
    let mut responses = match config.remove("responses_websocket") {
        None => serde_json::Map::new(),
        Some(serde_json::Value::Object(config)) => config,
        Some(_) => return Err("config.responses_websocket 必须是 JSON 对象".to_string()),
    };
    responses.insert("enabled".to_string(), serde_json::Value::Bool(enabled));
    config.insert(
        "responses_websocket".to_string(),
        serde_json::Value::Object(responses),
    );
    Ok(())
}

pub(crate) fn validate_responses_websocket_config(
    config: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let Some(value) = config.get("responses_websocket") else {
        return Ok(());
    };
    let responses = value
        .as_object()
        .ok_or_else(|| "config.responses_websocket 必须是 JSON 对象".to_string())?;
    let enabled = responses
        .get("enabled")
        .ok_or_else(|| "config.responses_websocket.enabled 为必填布尔值".to_string())?;
    if !enabled.is_boolean() {
        return Err("config.responses_websocket.enabled 必须是布尔值".to_string());
    }
    Ok(())
}

fn normalize_json_like_object(
    value: Option<serde_json::Value>,
    field_name: &str,
) -> Result<Option<serde_json::Value>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Object(map) => Ok(Some(serde_json::Value::Object(map))),
        _ => Err(format!("{field_name} 必须是 JSON 对象")),
    }
}

#[cfg(test)]
mod tests {
    use super::{set_responses_websocket_enabled, validate_responses_websocket_config};

    #[test]
    fn responses_websocket_setting_requires_an_explicit_boolean() {
        let mut config = serde_json::Map::new();
        set_responses_websocket_enabled(&mut config, true).expect("setting should normalize");
        assert_eq!(
            config.get("responses_websocket"),
            Some(&serde_json::json!({"enabled": true}))
        );
        validate_responses_websocket_config(&config).expect("setting should validate");

        config.insert(
            "responses_websocket".to_string(),
            serde_json::json!({"enabled": "yes"}),
        );
        assert!(validate_responses_websocket_config(&config).is_err());
    }
}
