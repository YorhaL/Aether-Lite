use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use aether_data_contracts::repository::usage::StoredRequestUsageAudit;
use aether_data_contracts::repository::usage::UsageDailyActualCostRollupQuery;
use aether_runtime_state::{
    DailyUsageLimitCountInput, DailyUsageLimitIncrementInput, DailyUsageLimitRestoreEntry,
    DailyUsageLimitRestoreInput, RuntimeState,
};
use chrono::{DateTime, SecondsFormat, Utc};
use tracing::warn;

use crate::app_timezone::{app_timezone, local_day_window};
use crate::control::GatewayControlDecision;
use crate::stage_metrics::observe_gateway_stage_ms;
use crate::{AppState, GatewayError};

const LIMIT_EPSILON_USD: f64 = 0.000_000_01;
const USD_UNITS_PER_DOLLAR: f64 = 100_000_000.0;
const COUNTER_EXPIRY_GRACE_SECONDS: u64 = 60;
const DAILY_USAGE_RUNTIME_STATE_KEY: &str = "daily_usage_limit:runtime_state";
const DAILY_USAGE_RECOVERY_LOCK_KEY: &str = "daily_usage_limit:recovery";
const DAILY_USAGE_RECOVERY_LOCK_OWNER: &str = "gateway-daily-usage-recovery";
const DAILY_USAGE_RECOVERY_LOCK_TTL: Duration = Duration::from_secs(600);
const DAILY_USAGE_RECOVERY_RETRY_DELAY: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DailyUsageScopeStatus {
    pub(crate) scope: &'static str,
    pub(crate) limit_usd: f64,
    pub(crate) used_usd: f64,
    pub(crate) remaining_usd: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrontdoorDailyUsageStatus {
    pub(crate) available: bool,
    pub(crate) timezone: String,
    pub(crate) window_start: String,
    pub(crate) window_end: String,
    pub(crate) reset_at_unix_secs: u64,
    pub(crate) user: Option<DailyUsageScopeStatus>,
    pub(crate) key: Option<DailyUsageScopeStatus>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrontdoorPrincipalDailyUsageStatus {
    pub(crate) available: bool,
    pub(crate) timezone: String,
    pub(crate) window_start: String,
    pub(crate) window_end: String,
    pub(crate) reset_at_unix_secs: u64,
    pub(crate) used_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrontdoorDailyUsageRejection {
    pub(crate) scope: &'static str,
    pub(crate) limit_usd: f64,
    pub(crate) used_usd: f64,
    pub(crate) remaining_usd: f64,
    pub(crate) retry_after: u64,
    pub(crate) reset_at_unix_secs: u64,
    pub(crate) timezone: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FrontdoorDailyUsageOutcome {
    NotApplicable,
    Allowed,
    Rejected(FrontdoorDailyUsageRejection),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DailyUsageLimitedResponse;

#[derive(Debug, Clone)]
pub(crate) struct FrontdoorDailyUsageLimiter {
    recovery_inflight: Arc<AtomicBool>,
    runtime_failures: Arc<AtomicU64>,
}

impl Default for FrontdoorDailyUsageLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl FrontdoorDailyUsageLimiter {
    pub(crate) fn new() -> Self {
        Self {
            recovery_inflight: Arc::new(AtomicBool::new(false)),
            runtime_failures: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(crate) fn runtime_failure_count(&self) -> u64 {
        self.runtime_failures.load(Ordering::Relaxed)
    }

    pub(crate) async fn check(
        &self,
        state: &AppState,
        decision: &GatewayControlDecision,
    ) -> FrontdoorDailyUsageOutcome {
        let started_at = Instant::now();
        let status_result = self.current_status(state, decision).await;
        observe_gateway_stage_ms(
            "daily_usage_limit_total",
            started_at.elapsed().as_millis() as u64,
        );
        let status = match status_result {
            Ok(Some(status)) => status,
            Ok(None) => return FrontdoorDailyUsageOutcome::NotApplicable,
            Err(err) => {
                let failure_count = self.runtime_failures.fetch_add(1, Ordering::Relaxed) + 1;
                let auth = decision.auth_context.as_ref();
                warn!(
                    event_name = "frontdoor_daily_usage_check_failed",
                    log_type = "ops",
                    error = ?err,
                    runtime_failures_total = failure_count,
                    user_id = auth.map(|auth| auth.user_id.as_str()).unwrap_or("-"),
                    api_key_id = auth.map(|auth| auth.api_key_id.as_str()).unwrap_or("-"),
                    "daily usage limit check failed; allowing request"
                );
                return FrontdoorDailyUsageOutcome::Allowed;
            }
        };
        if !status.available {
            return FrontdoorDailyUsageOutcome::Allowed;
        }
        let exceeded = status
            .user
            .as_ref()
            .filter(|scope| scope.used_usd + LIMIT_EPSILON_USD >= scope.limit_usd)
            .or_else(|| {
                status
                    .key
                    .as_ref()
                    .filter(|scope| scope.used_usd + LIMIT_EPSILON_USD >= scope.limit_usd)
            });
        let Some(exceeded) = exceeded else {
            return FrontdoorDailyUsageOutcome::Allowed;
        };
        let now = Utc::now().timestamp().max(0) as u64;
        FrontdoorDailyUsageOutcome::Rejected(FrontdoorDailyUsageRejection {
            scope: exceeded.scope,
            limit_usd: exceeded.limit_usd,
            used_usd: exceeded.used_usd,
            remaining_usd: exceeded.remaining_usd,
            retry_after: status.reset_at_unix_secs.saturating_sub(now).max(1),
            reset_at_unix_secs: status.reset_at_unix_secs,
            timezone: status.timezone,
        })
    }

    pub(crate) async fn current_status(
        &self,
        state: &AppState,
        decision: &GatewayControlDecision,
    ) -> Result<Option<FrontdoorDailyUsageStatus>, GatewayError> {
        let Some(auth) = decision.auth_context.as_ref() else {
            return Ok(None);
        };
        if decision.route_class.as_deref() != Some("ai_public")
            || auth.local_rejection.is_some()
            || auth.user_id.is_empty()
            || auth.api_key_id.is_empty()
            || auth.admin_bypass_limits
            || auth.ip_bypass_limits
        {
            return Ok(None);
        }

        let (user_limit, key_limit) = resolve_scope_limits(
            auth.api_key_is_standalone,
            auth.admission_policy.principal.daily_usage_limit_usd(),
            auth.admission_policy.api_key.daily_usage_limit_usd(),
        );
        if user_limit.is_none() && key_limit.is_none() {
            return Ok(None);
        }

        let timezone = app_timezone();
        let now = Utc::now();
        let (_, start, end) = local_day_window(now, timezone);
        let bucket = start.timestamp().max(0) as u64;
        let user_scope_key = daily_usage_user_scope_key(&auth.user_id, bucket);
        let key_scope_key = daily_usage_key_scope_key(&auth.api_key_id, bucket);
        let runtime_started_at = Instant::now();
        let counts_result = state
            .runtime_state
            .daily_usage_limit_counts(DailyUsageLimitCountInput {
                state_key: DAILY_USAGE_RUNTIME_STATE_KEY,
                user_key: (!auth.api_key_is_standalone).then_some(user_scope_key.as_str()),
                key_key: &key_scope_key,
                bucket,
            })
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()));
        observe_gateway_stage_ms(
            "daily_usage_limit_runtime_read",
            runtime_started_at.elapsed().as_millis() as u64,
        );
        let counts = counts_result?;
        if !counts.state_ready {
            self.trigger_runtime_recovery(state);
            return Ok(Some(FrontdoorDailyUsageStatus {
                available: false,
                timezone: timezone.name().to_string(),
                window_start: rfc3339(start),
                window_end: rfc3339(end),
                reset_at_unix_secs: end.timestamp().max(0) as u64,
                user: None,
                key: None,
            }));
        }
        let user = user_limit
            .map(|limit_usd| scope_status("user", limit_usd, units_to_usd(counts.user_units)));
        let key = key_limit
            .map(|limit_usd| scope_status("key", limit_usd, units_to_usd(counts.key_units)));
        Ok(Some(FrontdoorDailyUsageStatus {
            available: true,
            timezone: timezone.name().to_string(),
            window_start: rfc3339(start),
            window_end: rfc3339(end),
            reset_at_unix_secs: end.timestamp().max(0) as u64,
            user,
            key,
        }))
    }

    pub(crate) async fn current_principal_status(
        &self,
        state: &AppState,
        user_id: &str,
    ) -> Result<FrontdoorPrincipalDailyUsageStatus, GatewayError> {
        let timezone = app_timezone();
        let now = Utc::now();
        let (_, start, end) = local_day_window(now, timezone);
        let bucket = start.timestamp().max(0) as u64;
        let user_scope_key = daily_usage_user_scope_key(user_id, bucket);
        let unused_key_scope_key = daily_usage_key_scope_key("account-status", bucket);
        let mut counts = state
            .runtime_state
            .daily_usage_limit_counts(DailyUsageLimitCountInput {
                state_key: DAILY_USAGE_RUNTIME_STATE_KEY,
                user_key: Some(user_scope_key.as_str()),
                key_key: unused_key_scope_key.as_str(),
                bucket,
            })
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))?;
        if !counts.state_ready && recover_daily_usage_runtime(state).await? {
            counts = state
                .runtime_state
                .daily_usage_limit_counts(DailyUsageLimitCountInput {
                    state_key: DAILY_USAGE_RUNTIME_STATE_KEY,
                    user_key: Some(user_scope_key.as_str()),
                    key_key: unused_key_scope_key.as_str(),
                    bucket,
                })
                .await
                .map_err(|err| GatewayError::Internal(err.to_string()))?;
        }
        if !counts.state_ready {
            self.trigger_runtime_recovery(state);
            return Ok(FrontdoorPrincipalDailyUsageStatus {
                available: false,
                timezone: timezone.name().to_string(),
                window_start: rfc3339(start),
                window_end: rfc3339(end),
                reset_at_unix_secs: end.timestamp().max(0) as u64,
                used_usd: None,
            });
        }
        Ok(FrontdoorPrincipalDailyUsageStatus {
            available: true,
            timezone: timezone.name().to_string(),
            window_start: rfc3339(start),
            window_end: rfc3339(end),
            reset_at_unix_secs: end.timestamp().max(0) as u64,
            used_usd: Some(units_to_usd(counts.user_units)),
        })
    }

    fn trigger_runtime_recovery(&self, state: &AppState) {
        if self
            .recovery_inflight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return;
        }
        let limiter = self.clone();
        let state = state.clone();
        tokio::spawn(async move {
            let started_at = Instant::now();
            let result = recover_daily_usage_runtime(&state).await;
            observe_gateway_stage_ms(
                "daily_usage_limit_recovery",
                started_at.elapsed().as_millis() as u64,
            );
            match result {
                Ok(true) => {}
                Ok(false) => tokio::time::sleep(DAILY_USAGE_RECOVERY_RETRY_DELAY).await,
                Err(err) => {
                    let failure_count =
                        limiter.runtime_failures.fetch_add(1, Ordering::Relaxed) + 1;
                    warn!(
                        event_name = "frontdoor_daily_usage_recovery_failed",
                        log_type = "ops",
                        error = ?err,
                        runtime_failures_total = failure_count,
                        "daily usage runtime recovery failed; limits remain fail-open"
                    );
                    tokio::time::sleep(DAILY_USAGE_RECOVERY_RETRY_DELAY).await;
                }
            }
            limiter.recovery_inflight.store(false, Ordering::Release);
        });
    }
}

async fn recover_daily_usage_runtime(state: &AppState) -> Result<bool, GatewayError> {
    let Some(lease) = state
        .runtime_state
        .lock_try_acquire(
            DAILY_USAGE_RECOVERY_LOCK_KEY,
            DAILY_USAGE_RECOVERY_LOCK_OWNER,
            DAILY_USAGE_RECOVERY_LOCK_TTL,
        )
        .await
        .map_err(|err| GatewayError::Internal(err.to_string()))?
    else {
        return Ok(false);
    };

    let recovery_result = async {
        state
            .runtime_state
            .kv_set(DAILY_USAGE_RUNTIME_STATE_KEY, "recovering", None)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))?;

        let timezone = app_timezone();
        let now = Utc::now();
        let (_, start, end) = local_day_window(now, timezone);
        let bucket = start.timestamp().max(0) as u64;
        let rollups = state
            .background_data
            .summarize_usage_daily_actual_cost_rollups(&UsageDailyActualCostRollupQuery {
                finalized_from_unix_secs: bucket,
                finalized_until_unix_secs: end.timestamp().max(0) as u64,
            })
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))?;

        let mut user_totals = HashMap::<String, f64>::new();
        let mut key_totals = HashMap::<String, (Option<String>, bool, f64)>::new();
        for rollup in rollups {
            let Some(api_key_id) = non_empty(&rollup.api_key_id) else {
                continue;
            };
            let amount = rollup.actual_total_cost_usd;
            if !amount.is_finite() || amount <= 0.0 {
                continue;
            }
            let user_id = rollup
                .user_id
                .as_deref()
                .and_then(non_empty)
                .map(ToOwned::to_owned);
            if !rollup.api_key_is_standalone {
                if let Some(user_id) = user_id.as_ref() {
                    *user_totals.entry(user_id.clone()).or_default() += amount;
                }
            }
            let key_total = key_totals.entry(api_key_id.to_string()).or_insert((
                user_id.clone(),
                rollup.api_key_is_standalone,
                0.0,
            ));
            key_total.2 += amount;
        }

        let entries = key_totals
            .into_iter()
            .map(|(api_key_id, (user_id, is_standalone, key_total))| {
                let user_id = (!is_standalone).then_some(user_id).flatten();
                let user_units = user_id
                    .as_ref()
                    .and_then(|user_id| user_totals.get(user_id))
                    .copied()
                    .map(usd_to_units)
                    .unwrap_or_default();
                DailyUsageLimitRestoreEntry {
                    user_key: user_id
                        .as_deref()
                        .map(|user_id| daily_usage_user_scope_key(user_id, bucket)),
                    key_key: daily_usage_key_scope_key(&api_key_id, bucket),
                    user_units,
                    key_units: usd_to_units(key_total),
                }
            })
            .collect::<Vec<_>>();
        let ttl_seconds = (end.timestamp().max(0) as u64)
            .saturating_sub(Utc::now().timestamp().max(0) as u64)
            .saturating_add(COUNTER_EXPIRY_GRACE_SECONDS)
            .max(1);
        state
            .runtime_state
            .restore_daily_usage_limits(DailyUsageLimitRestoreInput {
                entries: &entries,
                bucket,
                ttl_seconds,
            })
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))?;
        state
            .runtime_state
            .kv_set(DAILY_USAGE_RUNTIME_STATE_KEY, "ready", None)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))?;
        Ok::<(), GatewayError>(())
    }
    .await;

    if let Err(err) = state.runtime_state.lock_release(&lease).await {
        warn!(
            event_name = "frontdoor_daily_usage_recovery_lock_release_failed",
            log_type = "ops",
            error = ?err,
            "daily usage runtime recovery lock release failed"
        );
    }
    recovery_result.map(|()| true)
}

pub(crate) async fn record_finalized_daily_usage(
    runtime_state: &RuntimeState,
    usage: &StoredRequestUsageAudit,
) -> Result<(), aether_runtime_state::DataLayerError> {
    if usage.status != "completed" {
        return Ok(());
    }
    let amount_units = usd_to_units(usage.actual_total_cost_usd);
    if amount_units == 0 {
        return Ok(());
    }
    let Some(api_key_id) = usage.api_key_id.as_deref().and_then(non_empty) else {
        return Ok(());
    };
    let finalized_at = usage
        .finalized_at_unix_secs
        .unwrap_or(usage.updated_at_unix_secs);
    let Some(finalized_at) = DateTime::<Utc>::from_timestamp(finalized_at as i64, 0) else {
        return Ok(());
    };
    let timezone = app_timezone();
    let (_, start, end) = local_day_window(finalized_at, timezone);
    let bucket = start.timestamp().max(0) as u64;
    let key_scope_key = daily_usage_key_scope_key(api_key_id, bucket);
    let is_standalone = usage
        .request_metadata
        .as_ref()
        .and_then(|metadata| metadata.get("api_key_is_standalone"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let user_scope_key = (!is_standalone)
        .then(|| usage.user_id.as_deref().and_then(non_empty))
        .flatten()
        .map(|user_id| daily_usage_user_scope_key(user_id, bucket));
    let now = Utc::now().timestamp().max(0) as u64;
    let ttl_seconds = (end.timestamp().max(0) as u64)
        .saturating_sub(now)
        .saturating_add(COUNTER_EXPIRY_GRACE_SECONDS)
        .max(1);
    runtime_state
        .increment_daily_usage_limit(DailyUsageLimitIncrementInput {
            user_key: user_scope_key.as_deref(),
            key_key: &key_scope_key,
            bucket,
            amount_units,
            ttl_seconds,
        })
        .await?;
    Ok(())
}

fn scope_status(scope: &'static str, limit_usd: f64, used_usd: f64) -> DailyUsageScopeStatus {
    DailyUsageScopeStatus {
        scope,
        limit_usd,
        used_usd,
        remaining_usd: (limit_usd - used_usd).max(0.0),
    }
}

fn daily_usage_user_scope_key(user_id: &str, bucket: u64) -> String {
    format!("daily_usage_limit:user:{user_id}:{bucket}")
}

fn daily_usage_key_scope_key(api_key_id: &str, bucket: u64) -> String {
    format!("daily_usage_limit:key:{api_key_id}:{bucket}")
}

fn usd_to_units(value: f64) -> u64 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    (value * USD_UNITS_PER_DOLLAR)
        .round()
        .clamp(0.0, u64::MAX as f64) as u64
}

fn units_to_usd(value: u64) -> f64 {
    value as f64 / USD_UNITS_PER_DOLLAR
}

fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn positive_limit(value: f64) -> Option<f64> {
    (value.is_finite() && value > 0.0).then_some(value)
}

fn resolve_scope_limits(
    is_standalone: bool,
    principal_limit: Option<f64>,
    key_limit: Option<f64>,
) -> (Option<f64>, Option<f64>) {
    if is_standalone {
        (
            None,
            positive_limit(key_limit.or(principal_limit).unwrap_or_default()),
        )
    } else {
        (
            positive_limit(principal_limit.unwrap_or_default()),
            key_limit.and_then(positive_limit),
        )
    }
}

fn rfc3339(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}
