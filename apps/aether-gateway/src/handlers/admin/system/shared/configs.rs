use crate::handlers::admin::request::AdminAppState;
use crate::handlers::shared::unix_secs_to_rfc3339;
use crate::GatewayError;
use aether_admin::system::{
    admin_system_config_default_value as admin_system_config_default_value_pure,
    admin_system_config_delete_keys as admin_system_config_delete_keys_pure,
    build_admin_system_config_deleted_payload,
    build_admin_system_config_detail_payload as build_admin_system_config_detail_payload_pure,
    build_admin_system_config_updated_payload,
    build_admin_system_configs_payload as build_admin_system_configs_payload_pure,
    is_sensitive_admin_system_config_key as is_sensitive_admin_system_config_key_pure,
    normalize_admin_system_config_key as normalize_admin_system_config_key_pure,
    parse_admin_system_config_update,
};
use aether_crypto::encrypt_fernet_plaintext;
use aether_data::repository::admission::{
    AdmissionPolicyDocument, AdmissionScopeKind, SYSTEM_ADMISSION_POLICY_SUBJECT,
};
use axum::body::Bytes;
use axum::http;
use serde_json::json;

fn normalize_admin_system_config_key(requested_key: &str) -> String {
    normalize_admin_system_config_key_pure(requested_key)
}

fn admin_system_config_delete_keys(requested_key: &str) -> Vec<String> {
    admin_system_config_delete_keys_pure(requested_key)
}

pub(crate) fn is_sensitive_admin_system_config_key(key: &str) -> bool {
    is_sensitive_admin_system_config_key_pure(key)
}

fn admin_system_config_default_value(key: &str) -> Option<serde_json::Value> {
    admin_system_config_default_value_pure(key)
}

pub(crate) async fn build_admin_system_configs_payload(
    state: &AdminAppState<'_>,
    entries: &[aether_data::repository::system::StoredSystemConfigEntry],
) -> Result<serde_json::Value, GatewayError> {
    let entries = merge_admission_system_config_entries(state, entries).await?;
    Ok(build_admin_system_configs_payload_pure(&entries))
}

pub(crate) async fn merge_admission_system_config_entries(
    state: &AdminAppState<'_>,
    entries: &[aether_data::repository::system::StoredSystemConfigEntry],
) -> Result<Vec<aether_data::repository::system::StoredSystemConfigEntry>, GatewayError> {
    let mut entries = entries
        .iter()
        .filter(|entry| !is_admission_config_key(&entry.key))
        .cloned()
        .collect::<Vec<_>>();
    let document = system_admission_document(state).await?;
    if let Some(value) = document.requests_per_minute() {
        entries.push(aether_data::repository::system::StoredSystemConfigEntry {
            key: "rate_limit_per_minute".to_string(),
            value: json!(value),
            description: None,
            updated_at_unix_secs: None,
        });
    }
    if let Some(value) = document.daily_usage_limit_usd() {
        entries.push(aether_data::repository::system::StoredSystemConfigEntry {
            key: "daily_usage_limit_usd".to_string(),
            value: json!(value),
            description: None,
            updated_at_unix_secs: None,
        });
    }
    Ok(entries)
}

pub(crate) async fn build_admin_system_config_detail_payload(
    state: &AdminAppState<'_>,
    requested_key: &str,
) -> Result<Result<serde_json::Value, (http::StatusCode, serde_json::Value)>, GatewayError> {
    let requested_key = requested_key.trim();
    let normalized_key = normalize_admin_system_config_key(requested_key);
    let value = if is_admission_config_key(&normalized_key) {
        system_admission_config_value(state, &normalized_key).await?
    } else {
        state.read_system_config_json_value(&normalized_key).await?
    };
    let value = value.or_else(|| admin_system_config_default_value(&normalized_key));
    Ok(build_admin_system_config_detail_payload_pure(
        requested_key,
        value,
    ))
}

pub(crate) async fn apply_admin_system_config_update(
    state: &AdminAppState<'_>,
    requested_key: &str,
    request_body: &Bytes,
) -> Result<Result<serde_json::Value, (http::StatusCode, serde_json::Value)>, GatewayError> {
    let update = match parse_admin_system_config_update(requested_key, request_body) {
        Ok(update) => update,
        Err(err) => return Ok(Err(err)),
    };
    let mut value = update.value;
    let normalized_key = update.normalized_key;
    let description = update.description;

    if is_admission_config_key(&normalized_key) {
        let normalized = match normalized_admission_config_value(&normalized_key, &value) {
            Ok(value) => value,
            Err(detail) => {
                return Ok(Err((
                    http::StatusCode::BAD_REQUEST,
                    json!({ "detail": detail }),
                )))
            }
        };
        let mut document = system_admission_document(state).await?;
        document = match normalized_key.as_str() {
            "rate_limit_per_minute" => document.with_requests_per_minute(
                normalized
                    .as_u64()
                    .and_then(|value| u32::try_from(value).ok()),
            ),
            "daily_usage_limit_usd" => document.with_daily_usage_limit_usd(normalized.as_f64()),
            _ => unreachable!("admission config key checked above"),
        };
        state
            .app()
            .data
            .store_scoped_admission_document(
                AdmissionScopeKind::System,
                SYSTEM_ADMISSION_POLICY_SUBJECT,
                &document,
            )
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))?;
        state.app().invalidate_auth_context_cache();
        return Ok(Ok(build_admin_system_config_updated_payload(
            normalized_key,
            normalized,
            description,
            None,
        )));
    }

    if is_sensitive_admin_system_config_key(&normalized_key)
        && value.as_str().is_some_and(|raw| !raw.is_empty())
    {
        let Some(encryption_key) = state
            .encryption_key()
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(Err((
                http::StatusCode::SERVICE_UNAVAILABLE,
                json!({ "detail": "系统配置写入需要可用的加密密钥" }),
            )));
        };
        let plaintext = value.as_str().unwrap();
        value = json!(encrypt_fernet_plaintext(encryption_key, plaintext)
            .map_err(|err| GatewayError::Internal(err.to_string()))?);
    }

    let updated = state
        .upsert_system_config_entry(&normalized_key, &value, description.as_deref())
        .await?;
    let display_value = if is_sensitive_admin_system_config_key(&normalized_key) {
        json!("********")
    } else {
        updated.value.clone()
    };
    Ok(Ok(build_admin_system_config_updated_payload(
        updated.key,
        display_value,
        updated.description,
        updated.updated_at_unix_secs,
    )))
}

pub(crate) async fn delete_admin_system_config(
    state: &AdminAppState<'_>,
    requested_key: &str,
) -> Result<Result<serde_json::Value, (http::StatusCode, serde_json::Value)>, GatewayError> {
    let delete_keys = admin_system_config_delete_keys(requested_key);
    let mut deleted = false;
    for key in &delete_keys {
        if is_admission_config_key(key) {
            let mut document = system_admission_document(state).await?;
            let existed = system_admission_value(&document, key).is_some();
            document = match key.as_str() {
                "rate_limit_per_minute" => document.with_requests_per_minute(None),
                "daily_usage_limit_usd" => document.with_daily_usage_limit_usd(None),
                _ => unreachable!("admission config key checked above"),
            };
            if existed {
                state
                    .app()
                    .data
                    .store_scoped_admission_document(
                        AdmissionScopeKind::System,
                        SYSTEM_ADMISSION_POLICY_SUBJECT,
                        &document,
                    )
                    .await
                    .map_err(|err| GatewayError::Internal(err.to_string()))?;
                state.app().invalidate_auth_context_cache();
            }
            deleted |= existed;
        } else {
            deleted |= state.delete_system_config_value(key).await?;
        }
    }
    if !deleted {
        return Ok(Err((
            http::StatusCode::NOT_FOUND,
            json!({ "detail": format!("配置项 '{requested_key}' 不存在") }),
        )));
    }
    Ok(Ok(build_admin_system_config_deleted_payload(requested_key)))
}

fn is_admission_config_key(key: &str) -> bool {
    matches!(key, "rate_limit_per_minute" | "daily_usage_limit_usd")
}

async fn system_admission_document(
    state: &AdminAppState<'_>,
) -> Result<AdmissionPolicyDocument, GatewayError> {
    state
        .app()
        .data
        .scoped_admission_document(AdmissionScopeKind::System, SYSTEM_ADMISSION_POLICY_SUBJECT)
        .await
        .map_err(|err| GatewayError::Internal(err.to_string()))
}

async fn system_admission_config_value(
    state: &AdminAppState<'_>,
    key: &str,
) -> Result<Option<serde_json::Value>, GatewayError> {
    system_admission_document(state)
        .await
        .map(|document| system_admission_value(&document, key))
}

fn system_admission_value(
    document: &AdmissionPolicyDocument,
    key: &str,
) -> Option<serde_json::Value> {
    match key {
        "rate_limit_per_minute" => document.requests_per_minute().map(|value| json!(value)),
        "daily_usage_limit_usd" => document.daily_usage_limit_usd().map(|value| json!(value)),
        _ => None,
    }
}

fn normalized_admission_config_value(
    key: &str,
    value: &serde_json::Value,
) -> Result<serde_json::Value, &'static str> {
    match key {
        "rate_limit_per_minute" => {
            let parsed = value
                .as_u64()
                .or_else(|| {
                    value
                        .as_str()
                        .and_then(|raw| raw.trim().parse::<u64>().ok())
                })
                .and_then(|value| u32::try_from(value).ok())
                .ok_or("rate_limit_per_minute 必须是 0 到 4294967295 之间的整数")?;
            Ok(json!(parsed))
        }
        "daily_usage_limit_usd" => {
            let parsed = value
                .as_f64()
                .or_else(|| {
                    value
                        .as_str()
                        .and_then(|raw| raw.trim().parse::<f64>().ok())
                })
                .filter(|value| value.is_finite() && *value >= 0.0)
                .ok_or("daily_usage_limit_usd 必须是大于等于 0 的有限数值")?;
            Ok(json!(parsed))
        }
        _ => unreachable!("admission config key checked by caller"),
    }
}
