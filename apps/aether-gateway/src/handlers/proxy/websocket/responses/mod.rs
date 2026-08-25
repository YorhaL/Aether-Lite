//! Public OpenAI Responses (`/v1/responses`) WebSocket bridge.

mod audit;
mod planner;
mod protocol;
mod session;

use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, Response, Uri};

use crate::handlers::proxy::websocket::ingress::{
    prepare_authenticated_ai_websocket, AuthenticatedAiWebSocketUpgradePreparation,
    WebSocketIngressSpec,
};
use crate::handlers::proxy::websocket::session::RESPONSES_WEBSOCKET_SESSION_LIMITS;
use crate::{AppState, GatewayError};

pub(crate) async fn responses_websocket(
    State(state): State<AppState>,
    ConnectInfo(remote_addr): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    uri: Uri,
) -> Result<Response<Body>, GatewayError> {
    match prepare_authenticated_ai_websocket(
        state,
        remote_addr,
        headers,
        uri,
        RESPONSES_WEBSOCKET_INGRESS_SPEC,
    )
    .await?
    {
        AuthenticatedAiWebSocketUpgradePreparation::Rejected(response) => Ok(response),
        AuthenticatedAiWebSocketUpgradePreparation::Ready(prepared) => Ok(prepared
            .into_response_with(
                ws,
                RESPONSES_WEBSOCKET_SESSION_LIMITS,
                (),
                session::run_responses_websocket,
            )),
    }
}

const RESPONSES_WEBSOCKET_INGRESS_SPEC: WebSocketIngressSpec = WebSocketIngressSpec {
    route_unavailable_message: "OpenAI Responses WebSocket route is unavailable",
};
