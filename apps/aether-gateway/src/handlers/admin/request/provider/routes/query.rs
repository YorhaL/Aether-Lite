use crate::handlers::admin::provider::query::{
    models::build_admin_provider_query_models_response, payload::parse_admin_provider_query_body,
};
use crate::handlers::admin::request::AdminAppState;
use crate::handlers::admin::AdminRequestContext;
use crate::GatewayError;
use axum::{
    body::{Body, Bytes},
    http,
    response::Response,
};

impl<'a> AdminAppState<'a> {
    pub(crate) async fn maybe_build_admin_provider_query_route_response(
        &self,
        request_context: &AdminRequestContext<'_>,
        request_body: Option<&Bytes>,
    ) -> Result<Option<Response<Body>>, GatewayError> {
        let Some(decision) = request_context.decision() else {
            return Ok(None);
        };

        if decision.route_family.as_deref() != Some("provider_query_manage") {
            return Ok(None);
        }

        if request_context.method() != http::Method::POST {
            return Ok(None);
        }

        let payload = match parse_admin_provider_query_body(request_body) {
            Ok(value) => value,
            Err(response) => return Ok(Some(response)),
        };

        Ok(Some(
            build_admin_provider_query_models_response(self, &payload).await?,
        ))
    }
}
