use super::{
    build_auth_json_response, build_auth_wallet_summary_payload, http,
    resolve_authenticated_local_user, unix_secs_to_rfc3339, AppState, Body,
    GatewayPublicRequestContext, Response,
};
use crate::handlers::shared::round_to;
use aether_data_contracts::repository::usage::UsageSettledCostSummaryQuery;
use chrono::{TimeZone, Utc};
use serde_json::json;

const WALLET_USAGE_TIMEZONE: &str = "Asia/Shanghai";

fn build_wallet_balance_payload(
    wallet: Option<&aether_data::repository::wallet::StoredWalletSnapshot>,
) -> serde_json::Value {
    let wallet_payload = build_auth_wallet_summary_payload(wallet);
    let balance = wallet_payload
        .get("balance")
        .cloned()
        .unwrap_or_else(|| json!(0.0));
    let unlimited = wallet_payload
        .get("unlimited")
        .cloned()
        .unwrap_or_else(|| json!(false));
    json!({
        "wallet": wallet_payload.clone(),
        "unlimited": unlimited,
        "limit_mode": wallet_payload
            .get("limit_mode")
            .cloned()
            .unwrap_or_else(|| json!("finite")),
        "balance": balance.clone(),
        "wallet_balance": balance.clone(),
        "total_available_balance": if unlimited.as_bool().unwrap_or(false) {
            serde_json::Value::Null
        } else {
            balance
        },
        "currency": wallet_payload
            .get("currency")
            .cloned()
            .unwrap_or_else(|| json!("USD")),
    })
}

pub(in crate::handlers::public::support) async fn build_wallet_balance_payload_for_user(
    _state: &AppState,
    _user_id: &str,
    wallet: Option<&aether_data::repository::wallet::StoredWalletSnapshot>,
) -> serde_json::Value {
    build_wallet_balance_payload(wallet)
}

pub(in crate::handlers::public::support) async fn build_wallet_balance_payload_for_auth_scope(
    _state: &AppState,
    _user_id: &str,
    _api_key_is_standalone: bool,
    wallet: Option<&aether_data::repository::wallet::StoredWalletSnapshot>,
) -> serde_json::Value {
    build_wallet_balance_payload(wallet)
}

fn wallet_today_usage_window() -> Result<(String, String, u64, u64), String> {
    let offset = chrono::FixedOffset::east_opt(8 * 3600)
        .ok_or_else(|| "wallet usage timezone is invalid".to_string())?;
    let today = Utc::now().with_timezone(&offset).date_naive();
    let local_start_naive = today
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| "wallet today start is invalid".to_string())?;
    let local_start = offset
        .from_local_datetime(&local_start_naive)
        .single()
        .ok_or_else(|| "wallet today local start is ambiguous".to_string())?;
    let local_end = local_start + chrono::Duration::days(1);
    Ok((
        today.to_string(),
        WALLET_USAGE_TIMEZONE.to_string(),
        local_start.timestamp().max(0) as u64,
        local_end.timestamp().max(0) as u64,
    ))
}

async fn build_wallet_live_today_usage_payload_for_auth_scope(
    state: &AppState,
    user_id: Option<&str>,
    api_key_id: Option<&str>,
) -> Result<Option<serde_json::Value>, String> {
    if !state.has_usage_data_reader() {
        return Ok(None);
    }
    let (date, timezone, start_unix_secs, end_unix_secs) = wallet_today_usage_window()?;
    let summary = state
        .summarize_usage_settled_cost(&UsageSettledCostSummaryQuery {
            created_from_unix_secs: start_unix_secs,
            created_until_unix_secs: end_unix_secs,
            user_id: user_id.map(ToOwned::to_owned),
            api_key_id: api_key_id.map(ToOwned::to_owned),
        })
        .await
        .map_err(|err| format!("today usage lookup failed: {err:?}"))?;
    Ok(Some(json!({
        "date": date,
        "timezone": timezone,
        "total_cost": round_to(summary.total_cost_usd, 6),
        "total_requests": summary.total_requests,
        "input_tokens": summary.input_tokens,
        "output_tokens": summary.output_tokens,
        "cache_creation_tokens": summary.cache_creation_tokens,
        "cache_read_tokens": summary.cache_read_tokens,
        "first_finalized_at": summary.first_finalized_at_unix_secs.and_then(unix_secs_to_rfc3339),
        "last_finalized_at": summary.last_finalized_at_unix_secs.and_then(unix_secs_to_rfc3339),
        "aggregated_at": Utc::now().to_rfc3339(),
        "is_today": true,
    })))
}

pub(in crate::handlers::public::support) async fn build_wallet_live_today_usage_payload_for_user(
    state: &AppState,
    user_id: &str,
) -> Result<Option<serde_json::Value>, String> {
    build_wallet_live_today_usage_payload_for_auth_scope(state, Some(user_id), None).await
}

pub(in crate::handlers::public::support) async fn build_wallet_live_today_usage_payload_for_api_key(
    state: &AppState,
    api_key_id: &str,
) -> Result<Option<serde_json::Value>, String> {
    build_wallet_live_today_usage_payload_for_auth_scope(state, None, Some(api_key_id)).await
}

pub(super) async fn handle_wallet_balance(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
) -> Response<Body> {
    let auth = match resolve_authenticated_local_user(state, request_context, headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let wallet = state
        .read_wallet_snapshot_for_auth(&auth.user.id, "", false)
        .await
        .ok()
        .flatten();
    build_auth_json_response(
        http::StatusCode::OK,
        build_wallet_balance_payload_for_user(state, &auth.user.id, wallet.as_ref()).await,
        None,
    )
}
