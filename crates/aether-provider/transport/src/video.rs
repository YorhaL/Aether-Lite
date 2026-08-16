use aether_data_contracts::repository::video_tasks::StoredVideoTask;
use aether_video_tasks_core::{
    LocalVideoTaskSnapshot, LocalVideoTaskTransport, LocalVideoTaskTransportBridgeInput,
};
use async_trait::async_trait;

use super::auth::{resolve_local_gemini_auth, resolve_local_openai_bearer_auth};
use super::network::{resolve_transport_execution_timeouts, resolve_transport_profile};
use super::policy::{supports_local_gemini_transport, supports_local_standard_transport};
use super::snapshot::GatewayProviderTransportSnapshot;

#[async_trait]
pub trait VideoTaskTransportSnapshotLookup: Send + Sync {
    async fn read_video_task_provider_transport_snapshot(
        &self,
        provider_id: &str,
        endpoint_id: &str,
        key_id: &str,
    ) -> Result<Option<GatewayProviderTransportSnapshot>, String>;
}

fn resolve_local_video_task_transport(
    transport: &GatewayProviderTransportSnapshot,
    api_format: &str,
    model_name: Option<String>,
) -> Option<LocalVideoTaskTransport> {
    let (auth_header, auth_value) = match api_format.trim() {
        "openai:video" if supports_local_standard_transport(transport, api_format) => {
            resolve_local_openai_bearer_auth(transport)?
        }
        "gemini:video" if supports_local_gemini_transport(transport, api_format) => {
            resolve_local_gemini_auth(transport)?
        }
        _ => return None,
    };

    Some(LocalVideoTaskTransport::from_bridge_input(
        LocalVideoTaskTransportBridgeInput {
            upstream_base_url: transport.endpoint.base_url.clone(),
            provider_name: Some(transport.provider.name.clone()),
            provider_id: transport.provider.id.clone(),
            endpoint_id: transport.endpoint.id.clone(),
            key_id: transport.key.id.clone(),
            auth_header,
            auth_value,
            content_type: Some("application/json".to_string()),
            model_name,
            transport_profile: resolve_transport_profile(transport),
            timeouts: resolve_transport_execution_timeouts(transport),
        },
    ))
}

pub async fn reconstruct_local_video_task_snapshot(
    lookup: &dyn VideoTaskTransportSnapshotLookup,
    task: &StoredVideoTask,
) -> Result<Option<LocalVideoTaskSnapshot>, String> {
    let provider_api_format = task
        .provider_api_format
        .as_deref()
        .unwrap_or_default()
        .trim();
    let (Some(provider_id), Some(endpoint_id), Some(key_id)) = (
        task.provider_id.as_deref(),
        task.endpoint_id.as_deref(),
        task.key_id.as_deref(),
    ) else {
        return Ok(None);
    };

    let Some(transport) = lookup
        .read_video_task_provider_transport_snapshot(provider_id, endpoint_id, key_id)
        .await?
    else {
        return Ok(None);
    };
    let Some(local_transport) =
        resolve_local_video_task_transport(&transport, provider_api_format, task.model.clone())
    else {
        return Ok(None);
    };

    Ok(LocalVideoTaskSnapshot::from_stored_task_with_transport(
        task,
        local_transport,
    ))
}
