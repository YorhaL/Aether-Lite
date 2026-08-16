use aether_usage_runtime::{
    extract_gemini_file_mapping_entries, gemini_file_mapping_cache_key, normalize_gemini_file_name,
    GatewayStreamReportRequest, GatewaySyncReportRequest, GEMINI_FILE_MAPPING_TTL_SECONDS,
};
use serde_json::Value;
use tracing::warn;
use uuid::Uuid;

use crate::clock::current_unix_secs;
use crate::{AppState, GatewayError};

#[derive(Debug, Clone, Copy)]
pub(crate) enum LocalReportEffect<'a> {
    Sync {
        payload: &'a GatewaySyncReportRequest,
    },
    Stream {
        payload: &'a GatewayStreamReportRequest,
    },
}

pub(crate) async fn apply_local_report_effect(state: &AppState, effect: LocalReportEffect<'_>) {
    if let LocalReportEffect::Sync { payload } = effect {
        apply_gemini_file_mapping_effect(state, payload).await;
    }
}

async fn apply_gemini_file_mapping_effect(state: &AppState, payload: &GatewaySyncReportRequest) {
    match payload.report_kind.as_str() {
        "gemini_files_store_mapping" if payload.status_code < 300 => {
            let key_id = payload
                .report_context
                .as_ref()
                .and_then(|context| context.get("file_key_id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let user_id = payload
                .report_context
                .as_ref()
                .and_then(|context| context.get("user_id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let Some(key_id) = key_id else {
                return;
            };

            for entry in extract_gemini_file_mapping_entries(payload) {
                if let Err(error) = store_local_gemini_file_mapping(
                    state,
                    &entry.file_name,
                    key_id,
                    user_id,
                    entry.display_name.as_deref(),
                    entry.mime_type.as_deref(),
                )
                .await
                {
                    warn!(
                        event_name = "gemini_file_mapping_store_failed",
                        file_name = %entry.file_name,
                        error = ?error,
                        "gateway failed to persist gemini file mapping"
                    );
                }
            }
        }
        "gemini_files_delete_mapping" if payload.status_code < 300 => {
            let file_name = payload
                .report_context
                .as_ref()
                .and_then(|context| context.get("file_name"))
                .and_then(Value::as_str)
                .and_then(normalize_gemini_file_name);
            if let Some(file_name) = file_name {
                if let Err(error) = delete_local_gemini_file_mapping(state, &file_name).await {
                    warn!(
                        event_name = "gemini_file_mapping_delete_failed",
                        file_name = %file_name,
                        error = ?error,
                        "gateway failed to delete gemini file mapping"
                    );
                }
            }
        }
        _ => {}
    }
}

pub(crate) async fn store_local_gemini_file_mapping(
    state: &AppState,
    file_name: &str,
    key_id: &str,
    user_id: Option<&str>,
    display_name: Option<&str>,
    mime_type: Option<&str>,
) -> Result<(), GatewayError> {
    let Some(file_name) = normalize_gemini_file_name(file_name) else {
        return Ok(());
    };
    let expires_at_unix_secs = current_unix_secs().saturating_add(GEMINI_FILE_MAPPING_TTL_SECONDS);
    state
        .upsert_gemini_file_mapping(
            aether_data::repository::gemini_file_mappings::UpsertGeminiFileMappingRecord {
                id: Uuid::new_v4().to_string(),
                file_name: file_name.clone(),
                key_id: key_id.to_string(),
                user_id: user_id.map(ToOwned::to_owned),
                display_name: display_name.map(ToOwned::to_owned),
                mime_type: mime_type.map(ToOwned::to_owned),
                source_hash: None,
                expires_at_unix_secs,
            },
        )
        .await?;
    state
        .cache_set_string_with_ttl(
            &gemini_file_mapping_cache_key(&file_name),
            key_id,
            GEMINI_FILE_MAPPING_TTL_SECONDS,
        )
        .await?;
    Ok(())
}

async fn delete_local_gemini_file_mapping(
    state: &AppState,
    file_name: &str,
) -> Result<(), GatewayError> {
    let Some(file_name) = normalize_gemini_file_name(file_name) else {
        return Ok(());
    };
    state
        .delete_gemini_file_mapping_by_file_name(&file_name)
        .await?;
    state
        .cache_delete_key(&gemini_file_mapping_cache_key(&file_name))
        .await?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn clear_local_report_effect_caches_for_tests() {}
