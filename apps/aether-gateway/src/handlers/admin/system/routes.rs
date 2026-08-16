use super::{adaptive, core};
use crate::handlers::admin::request::{AdminRouteRequest, AdminRouteResult};

pub(crate) async fn maybe_build_local_admin_system_response(
    request: AdminRouteRequest<'_>,
) -> AdminRouteResult {
    if let Some(response) = core::maybe_build_local_admin_core_response(
        &request.state(),
        &request.request_context(),
        request.request_body(),
    )
    .await?
    {
        return Ok(Some(response));
    }

    if let Some(response) = adaptive::maybe_build_local_admin_adaptive_response(
        &request.state(),
        &request.request_context(),
        request.request_body(),
    )
    .await?
    {
        return Ok(Some(response));
    }

    Ok(None)
}
