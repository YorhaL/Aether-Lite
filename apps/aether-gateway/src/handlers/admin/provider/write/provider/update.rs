use crate::handlers::admin::provider::shared::payloads::AdminProviderUpdatePatch;
use crate::handlers::admin::provider::write::normalize::{
    normalize_chat_pii_redaction_config, retain_supported_provider_config,
    set_responses_websocket_enabled, validate_responses_websocket_config,
};
use crate::handlers::admin::request::AdminAppState;
use crate::handlers::admin::shared::normalize_json_object;
use aether_data_contracts::repository::provider_catalog::StoredProviderCatalogProvider;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) async fn build_admin_update_provider_record(
    state: &AdminAppState<'_>,
    existing: &StoredProviderCatalogProvider,
    patch: AdminProviderUpdatePatch,
) -> Result<StoredProviderCatalogProvider, String> {
    let state = state.as_ref();
    let mut updated = existing.clone();
    let (fields, payload) = patch.into_parts();

    if fields.contains("name") {
        let Some(name) = payload.name.as_deref() else {
            return Err(if fields.is_null("name") {
                "name 不能为空".to_string()
            } else {
                "name 必须是字符串".to_string()
            });
        };
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err("name 不能为空".to_string());
        }
        let duplicate = state
            .list_provider_catalog_providers(false)
            .await
            .map_err(|err| format!("{err:?}"))?
            .into_iter()
            .any(|provider| provider.id != existing.id && provider.name == trimmed);
        if duplicate {
            return Err(format!("提供商名称 '{trimmed}' 已存在"));
        }
        updated.name = trimmed.to_string();
    }

    if fields.contains("description") {
        updated.description = payload
            .description
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
    }

    if fields.contains("website") {
        updated.website = match payload.website {
            None => {
                if fields.is_null("website") {
                    None
                } else {
                    return Err("website 必须是字符串".to_string());
                }
            }
            Some(website) => {
                let trimmed = website.trim();
                if trimmed.is_empty() {
                    None
                } else if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
                    return Err("website 必须以 http:// 或 https:// 开头".to_string());
                } else {
                    Some(trimmed.to_string())
                }
            }
        };
    }

    if fields.contains("provider_priority") {
        let Some(provider_priority) = payload.provider_priority else {
            return Err(if fields.is_null("provider_priority") {
                "provider_priority 不能为空".to_string()
            } else {
                "provider_priority 必须是整数".to_string()
            });
        };
        if !(0..=10_000).contains(&provider_priority) {
            return Err("provider_priority 必须在 0 到 10000 之间".to_string());
        }
        updated.provider_priority = provider_priority;
    }

    if fields.contains("is_active") {
        let Some(is_active) = payload.is_active else {
            return Err("is_active 必须是布尔值".to_string());
        };
        updated.is_active = is_active;
    }

    if fields.contains("concurrent_limit") {
        updated.concurrent_limit = match payload.concurrent_limit {
            Some(value) if value >= 0 => Some(value),
            Some(_) => return Err("concurrent_limit 必须是非负整数".to_string()),
            None => None,
        };
    }

    if fields.contains("max_retries") {
        updated.max_retries = match payload.max_retries {
            Some(value) if (0..=999).contains(&value) => Some(value),
            Some(_) => return Err("max_retries 必须是 0 到 999 之间的整数".to_string()),
            None => None,
        };
    }

    if fields.contains("stream_first_byte_timeout") {
        updated.stream_first_byte_timeout_secs =
            super::normalize_provider_stream_first_byte_timeout(payload.stream_first_byte_timeout)?;
    }

    if fields.contains("request_timeout") {
        updated.request_timeout_secs =
            super::normalize_provider_request_timeout(payload.request_timeout)?;
    }

    let mut config_map = updated
        .config
        .clone()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    retain_supported_provider_config(&mut config_map);
    if fields.contains("config") {
        if fields.is_null("config") {
            config_map.clear();
        } else {
            let value = normalize_json_object(payload.config, "config")?
                .ok_or_else(|| "config 必须是 JSON 对象".to_string())?;
            let serde_json::Value::Object(patch_map) = value else {
                return Err("config 必须是 JSON 对象".to_string());
            };
            let mut patch_map = patch_map;
            retain_supported_provider_config(&mut patch_map);
            for (key, value) in patch_map {
                if value.is_null() {
                    config_map.remove(&key);
                } else {
                    config_map.insert(key, value);
                }
            }
        }
    }

    if fields.contains("failover_rules") {
        if fields.is_null("failover_rules") {
            config_map.remove("failover_rules");
        } else {
            let value = normalize_json_object(payload.failover_rules, "failover_rules")?
                .ok_or_else(|| "failover_rules 必须是 JSON 对象".to_string())?;
            config_map.insert("failover_rules".to_string(), value);
        }
    }

    if config_map.contains_key("chat_pii_redaction") {
        let value = normalize_chat_pii_redaction_config(config_map.remove("chat_pii_redaction"))?;
        if let Some(value) = value {
            config_map.insert("chat_pii_redaction".to_string(), value);
        }
    }

    if fields.contains("responses_websocket_enabled") {
        let enabled = payload
            .responses_websocket_enabled
            .ok_or_else(|| "responses_websocket_enabled 必须是布尔值".to_string())?;
        set_responses_websocket_enabled(&mut config_map, enabled)?;
    }
    validate_responses_websocket_config(&config_map)?;

    updated.config = (!config_map.is_empty()).then_some(serde_json::Value::Object(config_map));
    updated.updated_at_unix_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs());
    Ok(updated)
}
