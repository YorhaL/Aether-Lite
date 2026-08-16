use axum::routing::get;
use axum::Router;

use crate::audit::{get_auth_api_key_snapshot, get_decision_trace, get_request_candidate_trace};
use crate::hooks::{get_request_audit_bundle, get_request_usage_audit};
use crate::router::metrics;
use crate::state::AppState;

pub(crate) fn mount_operational_routes(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/_gateway/metrics", get(metrics))
        .route(
            "/_gateway/audit/auth/users/{user_id}/api-keys/{api_key_id}",
            get(get_auth_api_key_snapshot),
        )
        .route(
            "/_gateway/audit/decision-trace/{request_id}",
            get(get_decision_trace),
        )
        .route(
            "/_gateway/audit/request-candidates/{request_id}",
            get(get_request_candidate_trace),
        )
        .route(
            "/_gateway/audit/request-audit/{request_id}",
            get(get_request_audit_bundle),
        )
        .route(
            "/_gateway/audit/request-usage/{request_id}",
            get(get_request_usage_audit),
        )
}
