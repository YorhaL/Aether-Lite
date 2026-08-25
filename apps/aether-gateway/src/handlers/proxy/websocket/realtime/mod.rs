//! Public OpenAI Realtime (`/v1/realtime`) WebSocket bridge.

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
use crate::handlers::proxy::websocket::session::REALTIME_WEBSOCKET_SESSION_LIMITS;
use crate::{AppState, GatewayError};

pub(crate) async fn realtime_websocket(
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
        REALTIME_WEBSOCKET_INGRESS_SPEC,
    )
    .await?
    {
        AuthenticatedAiWebSocketUpgradePreparation::Rejected(response) => Ok(response),
        AuthenticatedAiWebSocketUpgradePreparation::Ready(prepared) => {
            let realtime =
                match session::prepare_realtime_websocket(prepared.state(), prepared.context())
                    .await
                {
                    Ok(realtime) => realtime,
                    Err(rejection) => {
                        return prepared.rejection_response(rejection.status(), rejection.message())
                    }
                };
            Ok(prepared.into_response_with(
                ws,
                REALTIME_WEBSOCKET_SESSION_LIMITS,
                realtime,
                session::run_realtime_websocket,
            ))
        }
    }
}

const REALTIME_WEBSOCKET_INGRESS_SPEC: WebSocketIngressSpec = WebSocketIngressSpec {
    route_unavailable_message: "OpenAI Realtime WebSocket route is unavailable",
};
