use super::{AdminAppState, AdminRequestContext};
use crate::AppState;
use axum::body::Bytes;
use axum::http::HeaderMap;

#[derive(Clone, Copy)]
pub(crate) struct AdminRouteRequest<'a> {
    state: AdminAppState<'a>,
    request_context: AdminRequestContext<'a>,
    request_headers: &'a HeaderMap,
    request_body: Option<&'a Bytes>,
}

impl<'a> AdminRouteRequest<'a> {
    pub(crate) fn new(
        state: &'a AppState,
        request_context: &'a crate::control::GatewayPublicRequestContext,
        request_headers: &'a HeaderMap,
        request_body: Option<&'a Bytes>,
    ) -> Self {
        Self {
            state: AdminAppState::new(state),
            request_context: AdminRequestContext::new(request_context),
            request_headers,
            request_body,
        }
    }

    pub(crate) fn state(self) -> AdminAppState<'a> {
        self.state
    }

    pub(crate) fn request_context(self) -> AdminRequestContext<'a> {
        self.request_context
    }

    pub(crate) fn request_headers(self) -> &'a HeaderMap {
        self.request_headers
    }

    pub(crate) fn request_body(self) -> Option<&'a Bytes> {
        self.request_body
    }
}
