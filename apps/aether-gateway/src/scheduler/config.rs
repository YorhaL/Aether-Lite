use aether_scheduler_core::SchedulerPriorityMode;

use crate::{AppState, GatewayError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SchedulerSchedulingMode {
    FixedOrder,
    #[default]
    CacheAffinity,
    LoadBalance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SchedulerOrderingConfig {
    pub(crate) priority_mode: SchedulerPriorityMode,
    pub(crate) scheduling_mode: SchedulerSchedulingMode,
}

impl Default for SchedulerOrderingConfig {
    fn default() -> Self {
        Self {
            priority_mode: SchedulerPriorityMode::Provider,
            scheduling_mode: SchedulerSchedulingMode::CacheAffinity,
        }
    }
}

pub(crate) fn parse_scheduler_priority_mode(
    value: Option<&serde_json::Value>,
) -> SchedulerPriorityMode {
    match value
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("global_key") => SchedulerPriorityMode::GlobalKey,
        _ => SchedulerPriorityMode::Provider,
    }
}

pub(crate) fn parse_scheduler_scheduling_mode(
    value: Option<&serde_json::Value>,
) -> SchedulerSchedulingMode {
    match value
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("fixed_order") => SchedulerSchedulingMode::FixedOrder,
        Some("load_balance") => SchedulerSchedulingMode::LoadBalance,
        _ => SchedulerSchedulingMode::CacheAffinity,
    }
}

pub(crate) async fn read_scheduler_ordering_config(
    state: &AppState,
) -> Result<SchedulerOrderingConfig, GatewayError> {
    let priority_mode = parse_scheduler_priority_mode(
        state
            .read_system_config_json_value("provider_priority_mode")
            .await?
            .as_ref(),
    );
    let scheduling_mode = parse_scheduler_scheduling_mode(
        state
            .read_system_config_json_value("scheduling_mode")
            .await?
            .as_ref(),
    );
    Ok(SchedulerOrderingConfig {
        priority_mode,
        scheduling_mode,
    })
}
