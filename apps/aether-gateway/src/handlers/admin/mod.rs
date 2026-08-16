mod announcements;
pub(super) mod auth;
mod billing;
pub(super) mod endpoint;
pub(super) mod features;
mod model;
pub(super) mod observability;
pub(super) mod provider;
mod routing;
mod system;
mod users;

pub(super) mod request;
pub(super) mod routes;
mod shared;

pub(crate) use self::auth::maybe_build_local_admin_security_response;
pub(crate) use self::endpoint::build_admin_endpoint_health_status_payload;
#[cfg(test)]
pub(crate) use self::model::set_admin_external_models_source_url_for_tests;
pub(crate) use self::observability::{
    admin_stats_bad_request_response, maybe_build_local_admin_usage_response, parse_bounded_u32,
    round_to, AdminStatsTimeRange, AdminStatsUsageFilter,
};
pub(crate) use self::provider::maybe_build_local_admin_providers_response;
pub(crate) use self::request::{
    AdminAppState, AdminGatewayProviderTransportSnapshot, AdminRequestContext, AdminRouteRequest,
    AdminRouteResponse, AdminRouteResult,
};
pub(crate) use self::routes::maybe_build_local_admin_response;
