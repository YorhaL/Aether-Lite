pub(super) use super::{
    build_auth_json_response, build_auth_wallet_summary_payload, resolve_authenticated_local_user,
    unix_secs_to_rfc3339, AppState, GatewayPublicRequestContext,
};
pub(super) use axum::{body::Body, http, response::Response};

#[path = "wallet/reads.rs"]
mod reads;

use self::reads::handle_wallet_balance;
pub(in crate::handlers::public::support) use self::reads::{
    build_wallet_balance_payload_for_auth_scope, build_wallet_balance_payload_for_user,
    build_wallet_live_today_usage_payload_for_api_key,
    build_wallet_live_today_usage_payload_for_user,
};

pub(super) async fn maybe_build_local_wallet_response(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
) -> Option<Response<Body>> {
    let decision = request_context.control_decision.as_ref()?;
    if decision.route_family.as_deref() != Some("wallet")
        || decision.route_kind.as_deref() != Some("balance")
        || request_context.request_path != "/api/wallet/balance"
    {
        return None;
    }

    Some(handle_wallet_balance(state, request_context, headers).await)
}
