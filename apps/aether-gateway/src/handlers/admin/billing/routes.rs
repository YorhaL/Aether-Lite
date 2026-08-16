use super::wallets;
use crate::handlers::admin::request::{AdminRouteRequest, AdminRouteResult};

pub(crate) async fn maybe_build_local_admin_billing_routes_response(
    request: AdminRouteRequest<'_>,
) -> AdminRouteResult {
    wallets::maybe_build_local_admin_wallets_response(
        &request.state(),
        &request.request_context(),
        request.request_body(),
    )
    .await
}
