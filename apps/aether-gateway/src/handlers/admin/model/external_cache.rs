use crate::handlers::admin::request::AdminAppState;
use crate::handlers::shared::mark_external_models_official_providers;
use crate::GatewayError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::warn;

const ADMIN_EXTERNAL_MODELS_LEGACY_CACHE_KEY: &str = "aether:external:models_dev";
const ADMIN_EXTERNAL_MODELS_CACHE_KEY: &str = "aether:external:models_dev:v2";
const ADMIN_EXTERNAL_MODELS_CACHE_VERSION: u8 = 2;
const ADMIN_EXTERNAL_MODELS_CACHE_TTL_SECS: u64 = 15 * 60;
const ADMIN_EXTERNAL_MODELS_SOURCE_URL_ENV: &str = "AETHER_GATEWAY_EXTERNAL_MODELS_URL";
const ADMIN_EXTERNAL_MODELS_SOURCE_URL_DEFAULT: &str = "https://models.dev/api.json";
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdminExternalModelsCacheEnvelope {
    schema_version: u8,
    payload: Value,
}

#[cfg(test)]
pub(crate) struct AdminExternalModelsSourceUrlEnvGuard {
    previous: Option<String>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Drop for AdminExternalModelsSourceUrlEnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.as_deref() {
            std::env::set_var(ADMIN_EXTERNAL_MODELS_SOURCE_URL_ENV, previous);
        } else {
            std::env::remove_var(ADMIN_EXTERNAL_MODELS_SOURCE_URL_ENV);
        }
    }
}

#[cfg(test)]
pub(crate) fn set_admin_external_models_source_url_for_tests(
    value: &str,
) -> AdminExternalModelsSourceUrlEnvGuard {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    let lock = LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let previous = std::env::var(ADMIN_EXTERNAL_MODELS_SOURCE_URL_ENV).ok();
    std::env::set_var(ADMIN_EXTERNAL_MODELS_SOURCE_URL_ENV, value);
    AdminExternalModelsSourceUrlEnvGuard {
        previous,
        _lock: lock,
    }
}

fn admin_external_models_source_url() -> String {
    std::env::var(ADMIN_EXTERNAL_MODELS_SOURCE_URL_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| ADMIN_EXTERNAL_MODELS_SOURCE_URL_DEFAULT.to_string())
}

fn normalize_admin_external_models_payload(payload: serde_json::Value) -> serde_json::Value {
    mark_external_models_official_providers(&payload).unwrap_or(payload)
}

async fn store_admin_external_models_cache(
    state: &AdminAppState<'_>,
    payload: &serde_json::Value,
) -> Result<(), GatewayError> {
    let envelope = AdminExternalModelsCacheEnvelope {
        schema_version: ADMIN_EXTERNAL_MODELS_CACHE_VERSION,
        payload: payload.clone(),
    };
    let serialized =
        serde_json::to_string(&envelope).map_err(|err| GatewayError::Internal(err.to_string()))?;
    state
        .as_ref()
        .runtime_kv_setex(
            ADMIN_EXTERNAL_MODELS_CACHE_KEY,
            &serialized,
            ADMIN_EXTERNAL_MODELS_CACHE_TTL_SECS,
        )
        .await?;
    Ok(())
}

async fn clear_admin_external_models_cache_entries(
    state: &AdminAppState<'_>,
) -> Result<bool, GatewayError> {
    let cleared_current = state
        .as_ref()
        .runtime_kv_del(ADMIN_EXTERNAL_MODELS_CACHE_KEY)
        .await?;
    let cleared_legacy = state
        .as_ref()
        .runtime_kv_del(ADMIN_EXTERNAL_MODELS_LEGACY_CACHE_KEY)
        .await?;
    Ok(cleared_current || cleared_legacy)
}

async fn fetch_admin_external_models_from_source(
    state: &AdminAppState<'_>,
) -> Result<serde_json::Value, GatewayError> {
    let url = admin_external_models_source_url();
    let response = state
        .http_client()
        .get(&url)
        .send()
        .await
        .map_err(|err| GatewayError::Internal(err.to_string()))?;
    let response = response
        .error_for_status()
        .map_err(|err| GatewayError::Internal(err.to_string()))?;
    let payload = response
        .json::<serde_json::Value>()
        .await
        .map_err(|err| GatewayError::Internal(err.to_string()))?;
    Ok(normalize_admin_external_models_payload(payload))
}

pub(crate) async fn read_admin_external_models_cache(
    state: &AdminAppState<'_>,
    _request_id: &str,
) -> Result<Option<serde_json::Value>, GatewayError> {
    if let Some(raw) = state
        .as_ref()
        .runtime_kv_get(ADMIN_EXTERNAL_MODELS_CACHE_KEY)
        .await?
    {
        match serde_json::from_str::<AdminExternalModelsCacheEnvelope>(&raw) {
            Ok(envelope) if envelope.schema_version == ADMIN_EXTERNAL_MODELS_CACHE_VERSION => {
                return Ok(Some(normalize_admin_external_models_payload(
                    envelope.payload,
                )));
            }
            Ok(_) => {}
            Err(err) => {
                warn!(error = %err, "failed to parse cached external models payload");
            }
        }
    }

    match fetch_admin_external_models_from_source(state).await {
        Ok(payload) => {
            if let Err(err) = store_admin_external_models_cache(state, &payload).await {
                warn!(error = ?err, "failed to store fetched external models cache");
            }
            Ok(Some(payload))
        }
        Err(err) => {
            warn!(error = ?err, "failed to fetch external models catalog");
            Ok(None)
        }
    }
}

pub(crate) async fn clear_admin_external_models_cache(
    state: &AdminAppState<'_>,
) -> Result<serde_json::Value, GatewayError> {
    let deleted = clear_admin_external_models_cache_entries(state).await?;
    Ok(json!({
        "cleared": deleted,
        "message": if deleted { "缓存已清除" } else { "缓存不存在" },
    }))
}
