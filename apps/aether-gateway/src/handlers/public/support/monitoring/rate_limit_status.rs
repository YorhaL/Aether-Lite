use axum::{
    body::Body,
    http,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use serde_json::json;

use super::{
    build_auth_error_response, resolve_authenticated_local_user, AppState,
    GatewayPublicRequestContext,
};

fn rule_status(available: bool, limit: f64) -> &'static str {
    if !available {
        "unavailable"
    } else if limit > 0.0 {
        "available"
    } else {
        "unlimited"
    }
}

fn request_count_rule(
    available: bool,
    limit: u32,
    used: Option<u32>,
    window_seconds: u64,
    reset_at: &str,
) -> serde_json::Value {
    json!({
        "kind": "request_count",
        "available": available,
        "status": rule_status(available, f64::from(limit)),
        "limit": limit,
        "used": used,
        "remaining": used
            .filter(|_| limit > 0)
            .map(|used| limit.saturating_sub(used)),
        "window_seconds": window_seconds,
        "reset_at": reset_at,
    })
}

fn concurrent_requests_rule(available: bool, limit: u32, used: Option<u64>) -> serde_json::Value {
    json!({
        "kind": "concurrent_requests",
        "available": available,
        "status": rule_status(available, f64::from(limit)),
        "limit": limit,
        "used": used,
        "remaining": used
            .filter(|_| limit > 0)
            .map(|used| u64::from(limit).saturating_sub(used)),
    })
}

fn daily_usage_rule(
    status: &crate::daily_usage_limit::FrontdoorPrincipalDailyUsageStatus,
    limit_usd: f64,
) -> serde_json::Value {
    json!({
        "kind": "usage_cost_usd",
        "available": status.available,
        "status": rule_status(status.available, limit_usd),
        "limit": limit_usd,
        "used": status.used_usd,
        "remaining": status
            .used_usd
            .filter(|_| limit_usd > 0.0)
            .map(|used| (limit_usd - used).max(0.0)),
        "period": "calendar_day",
        "timezone": status.timezone,
        "window_start": status.window_start,
        "window_end": status.window_end,
        "reset_at": status.window_end,
    })
}

pub(super) async fn handle_user_rate_limit_status(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
) -> Response<Body> {
    let auth = match resolve_authenticated_local_user(state, request_context, headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };

    let groups = match state.list_user_groups_for_user(&auth.user.id).await {
        Ok(value) => value,
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("user admission groups read failed: {err:?}"),
                false,
            )
        }
    };
    let group_ids = groups
        .iter()
        .map(|group| group.id.clone())
        .collect::<Vec<_>>();
    let principal = match state
        .data
        .resolve_principal_admission_policy(&auth.user.id, &group_ids)
        .await
    {
        Ok(value) => value,
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("user admission policy read failed: {err:?}"),
                false,
            )
        }
    };

    let now = Utc::now();
    let now_unix_secs = u64::try_from(now.timestamp()).unwrap_or_default();
    let rpm_limiter = state.frontdoor_user_rpm();
    let rpm_limit = principal.requests_per_minute().unwrap_or_default();
    let rpm_reset_at = (now
        + chrono::Duration::seconds(
            i64::try_from(rpm_limiter.retry_after(now_unix_secs)).unwrap_or_default(),
        ))
    .to_rfc3339();
    let (rpm_available, rpm_used) = if rpm_limit == 0 {
        (true, None)
    } else {
        let bucket = rpm_limiter.current_bucket(now_unix_secs);
        let scope_key = rpm_limiter.user_scope_key(&auth.user.id, bucket);
        match rpm_limiter.get_scope_count(state, &scope_key, bucket).await {
            Ok(value) => (true, Some(value)),
            Err(err) => {
                tracing::warn!(
                    error = ?err,
                    user_id = %auth.user.id,
                    "account rpm status unavailable"
                );
                (false, None)
            }
        }
    };

    let concurrent_limit = principal.concurrent_requests().unwrap_or_default();
    let active_since =
        now_unix_secs.saturating_sub(aether_scheduler_core::ACTIVE_REQUEST_WINDOW_SECS);
    let (concurrent_available, concurrent_used) = match state
        .count_active_request_candidates_for_user_since(&auth.user.id, active_since)
        .await
    {
        Ok(value) => (true, Some(value)),
        Err(err) => {
            tracing::warn!(
                error = ?err,
                user_id = %auth.user.id,
                "account concurrency status unavailable"
            );
            (false, None)
        }
    };

    let daily_limit_usd = principal.daily_usage_limit_usd().unwrap_or_default();
    let daily_status = match state
        .frontdoor_daily_usage()
        .current_principal_status(state, &auth.user.id)
        .await
    {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!(
                error = ?err,
                user_id = %auth.user.id,
                "account daily usage status unavailable"
            );
            let timezone = crate::app_timezone::app_timezone();
            let (_, start, end) = crate::app_timezone::local_day_window(now, timezone);
            crate::daily_usage_limit::FrontdoorPrincipalDailyUsageStatus {
                available: false,
                timezone: timezone.name().to_string(),
                window_start: start.to_rfc3339(),
                window_end: end.to_rfc3339(),
                reset_at_unix_secs: end.timestamp().max(0) as u64,
                used_usd: None,
            }
        }
    };

    Json(json!({
        "user_id": auth.user.id,
        "rules": [
            request_count_rule(
                rpm_available,
                rpm_limit,
                rpm_used,
                rpm_limiter.config().bucket_seconds(),
                &rpm_reset_at,
            ),
            concurrent_requests_rule(
                concurrent_available,
                concurrent_limit,
                concurrent_used,
            ),
            daily_usage_rule(&daily_status, daily_limit_usd),
        ],
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::{concurrent_requests_rule, daily_usage_rule, request_count_rule};
    use crate::daily_usage_limit::FrontdoorPrincipalDailyUsageStatus;

    #[test]
    fn account_rules_expose_usage_and_remaining_values() {
        let rpm = request_count_rule(true, 100, Some(37), 60, "rpm-reset");
        let concurrent = concurrent_requests_rule(true, 8, Some(2));
        let daily = daily_usage_rule(
            &FrontdoorPrincipalDailyUsageStatus {
                available: true,
                timezone: "Asia/Hong_Kong".to_string(),
                window_start: "day-start".to_string(),
                window_end: "day-end".to_string(),
                reset_at_unix_secs: 123,
                used_usd: Some(4.25),
            },
            20.0,
        );

        assert_eq!(rpm["used"], 37);
        assert_eq!(rpm["remaining"], 63);
        assert_eq!(concurrent["used"], 2);
        assert_eq!(concurrent["remaining"], 6);
        assert_eq!(daily["used"], 4.25);
        assert_eq!(daily["remaining"], 15.75);
        assert_eq!(daily["timezone"], "Asia/Hong_Kong");
    }

    #[test]
    fn account_rules_distinguish_unlimited_and_unavailable() {
        let unlimited = concurrent_requests_rule(true, 0, Some(2));
        let unavailable = request_count_rule(false, 100, None, 60, "rpm-reset");

        assert_eq!(unlimited["status"], "unlimited");
        assert!(unlimited["remaining"].is_null());
        assert_eq!(unavailable["status"], "unavailable");
        assert!(unavailable["used"].is_null());
    }
}
