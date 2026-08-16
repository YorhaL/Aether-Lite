use axum::routing::any;
use axum::Router;

use crate::{handlers::proxy::proxy_request, state::AppState};

pub(crate) fn mount_internal_routes(router: Router<AppState>) -> Router<AppState> {
    router.route(
        "/api/internal/gateway/{*internal_gateway_path}",
        any(proxy_request),
    )
}
