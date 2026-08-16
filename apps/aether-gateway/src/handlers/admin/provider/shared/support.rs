use crate::handlers::admin::request::AdminAppState;
use crate::LocalProviderDeleteTaskState;
use serde_json::json;

pub(crate) const ADMIN_PROVIDER_MAPPING_PREVIEW_MAX_KEYS: usize = 200;
pub(crate) const ADMIN_PROVIDER_MAPPING_PREVIEW_MAX_MODELS: usize = 500;
pub(crate) const ADMIN_PROVIDER_MAPPING_PREVIEW_FETCH_LIMIT: usize = 10_000;
pub(crate) fn build_admin_provider_delete_task_payload(
    task: &LocalProviderDeleteTaskState,
) -> serde_json::Value {
    json!({
        "task_id": task.task_id,
        "provider_id": task.provider_id,
        "status": task.status,
        "stage": task.stage,
        "total_keys": task.total_keys,
        "deleted_keys": task.deleted_keys,
        "total_endpoints": task.total_endpoints,
        "deleted_endpoints": task.deleted_endpoints,
        "message": task.message,
    })
}

pub(crate) fn put_admin_provider_delete_task(
    state: &AdminAppState<'_>,
    task: &LocalProviderDeleteTaskState,
) {
    state.as_ref().put_provider_delete_task(task.clone());
}

pub(crate) fn parse_optional_rfc3339_unix_secs(
    value: &str,
    field_name: &str,
) -> Result<u64, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{field_name} 不能为空"));
    }
    let parsed = chrono::DateTime::parse_from_rfc3339(trimmed)
        .map_err(|_| format!("{field_name} 必须是合法的 RFC3339 时间"))?;
    u64::try_from(parsed.timestamp()).map_err(|_| format!("{field_name} 超出有效时间范围"))
}
