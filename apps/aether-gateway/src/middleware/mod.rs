mod frontdoor_cors;

pub use aether_gateway_frontdoor::strip_cf_headers_middleware;
pub(crate) use aether_gateway_frontdoor::{apply_cf_header_stripping, CfConnectingIp};
pub(crate) use frontdoor_cors::frontdoor_cors_middleware;
