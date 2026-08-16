use super::super::shared::{
    admin_wallet_id_from_suffix_path, build_admin_wallet_not_found_response,
    build_admin_wallet_summary_payload, build_admin_wallets_bad_request_response,
    build_admin_wallets_data_unavailable_response, normalize_admin_wallet_non_zero_amount,
    resolve_admin_wallet_owner_summary, AdminWalletAdjustRequest,
};
use crate::handlers::admin::request::{AdminAppState, AdminRequestContext};
use crate::handlers::admin::shared::attach_admin_audit_response;
use crate::GatewayError;
use axum::{
    body::Body,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub(in super::super) async fn build_admin_wallet_adjust_response(
    state: &AdminAppState<'_>,
    request_context: &AdminRequestContext<'_>,
    request_body: Option<&axum::body::Bytes>,
) -> Result<Response<Body>, GatewayError> {
    let Some(wallet_id) = admin_wallet_id_from_suffix_path(request_context.path(), "/adjust")
    else {
        return Ok(build_admin_wallets_bad_request_response("wallet_id 无效"));
    };
    let Some(request_body) = request_body else {
        return Ok(build_admin_wallets_bad_request_response("请求体不能为空"));
    };
    let payload = match serde_json::from_slice::<AdminWalletAdjustRequest>(request_body) {
        Ok(value) => value,
        Err(_) => return Ok(build_admin_wallets_bad_request_response("请求体格式无效")),
    };
    let amount_usd = match normalize_admin_wallet_non_zero_amount(payload.amount_usd, "amount_usd")
    {
        Ok(value) => value,
        Err(detail) => return Ok(build_admin_wallets_bad_request_response(detail)),
    };
    let Some(_existing_wallet) = state
        .find_wallet(aether_data::repository::wallet::WalletLookupKey::WalletId(
            &wallet_id,
        ))
        .await?
    else {
        return Ok(build_admin_wallet_not_found_response());
    };
    let has_wallet_writer = state.has_wallet_data_writer();
    let Some(wallet) = state
        .admin_adjust_wallet_balance(&wallet_id, amount_usd)
        .await?
    else {
        return if has_wallet_writer {
            Ok(build_admin_wallet_not_found_response())
        } else {
            Ok(build_admin_wallets_data_unavailable_response())
        };
    };
    let owner = resolve_admin_wallet_owner_summary(state, &wallet).await?;
    let wallet_payload = build_admin_wallet_summary_payload(&wallet, &owner);
    let response = Json(json!({
        "wallet": wallet_payload,
    }))
    .into_response();
    Ok(attach_admin_audit_response(
        response,
        "admin_wallet_balance_adjusted",
        "adjust_wallet_balance",
        "wallet",
        &wallet_id,
    ))
}
