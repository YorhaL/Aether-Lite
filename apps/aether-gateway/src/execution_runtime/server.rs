use std::path::Path;
use std::sync::Arc;

use aether_contracts::ExecutionPlan;
use aether_runtime::{
    maybe_hold_axum_response_permit, prometheus_response, service_up_sample, AdmissionPermit,
    ConcurrencyError, ConcurrencyGate, ConcurrencySnapshot, MetricKind, MetricLabel, MetricSample,
};
use aether_runtime_state::{RuntimeSemaphore, RuntimeSemaphoreError, RuntimeSemaphoreSnapshot};
use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use thiserror::Error;

use crate::execution_runtime::{
    build_direct_execution_frame_stream, DirectSyncExecutionRuntime, ExecutionRuntimeTransportError,
};
use crate::middleware;

const EXECUTION_RUNTIME_COMPONENT: &str = "aether-gateway-execution-runtime";
const REQUEST_GATE_NAME: &str = "execution_runtime_requests";
const DISTRIBUTED_REQUEST_GATE_NAME: &str = "execution_runtime_requests_distributed";

#[derive(Debug, Clone, Default)]
struct ExecutionRuntimeAppState {
    execution_runtime: DirectSyncExecutionRuntime,
    request_gate: Option<Arc<ConcurrencyGate>>,
    distributed_request_gate: Option<Arc<RuntimeSemaphore>>,
}

impl ExecutionRuntimeAppState {
    fn with_request_concurrency_limit(limit: Option<usize>) -> Self {
        Self {
            execution_runtime: DirectSyncExecutionRuntime::new(),
            request_gate: limit
                .filter(|limit| *limit > 0)
                .map(|limit| Arc::new(ConcurrencyGate::new(REQUEST_GATE_NAME, limit))),
            distributed_request_gate: None,
        }
    }

    fn with_distributed_request_gate(mut self, gate: RuntimeSemaphore) -> Self {
        self.distributed_request_gate = Some(Arc::new(gate));
        self
    }

    fn request_concurrency_snapshot(&self) -> Option<ConcurrencySnapshot> {
        self.request_gate.as_ref().map(|gate| gate.snapshot())
    }

    async fn distributed_request_concurrency_snapshot(
        &self,
    ) -> Result<Option<RuntimeSemaphoreSnapshot>, RuntimeSemaphoreError> {
        match self.distributed_request_gate.as_ref() {
            Some(gate) => gate.snapshot().await.map(Some),
            None => Ok(None),
        }
    }

    async fn metric_samples(&self) -> Vec<MetricSample> {
        let mut samples = vec![service_up_sample(EXECUTION_RUNTIME_COMPONENT)];
        if let Some(snapshot) = self.request_concurrency_snapshot() {
            samples.extend(snapshot.to_metric_samples(REQUEST_GATE_NAME));
        }
        if let Some(gate) = self.distributed_request_gate.as_ref() {
            match gate.snapshot().await {
                Ok(snapshot) => {
                    samples.extend(snapshot.to_metric_samples(DISTRIBUTED_REQUEST_GATE_NAME));
                }
                Err(_) => samples.push(
                    MetricSample::new(
                        "concurrency_unavailable",
                        "Whether the distributed concurrency gate is currently unavailable.",
                        MetricKind::Gauge,
                        1,
                    )
                    .with_labels(vec![MetricLabel::new(
                        "gate",
                        DISTRIBUTED_REQUEST_GATE_NAME,
                    )]),
                ),
            }
        }
        samples
    }

    async fn try_acquire_request_permit(
        &self,
    ) -> Result<Option<AdmissionPermit>, RequestAdmissionError> {
        let local = self
            .request_gate
            .as_ref()
            .map(|gate| gate.try_acquire())
            .transpose()
            .map_err(RequestAdmissionError::Local)?;
        let distributed = match self.distributed_request_gate.as_ref() {
            Some(gate) => Some(
                gate.try_acquire()
                    .await
                    .map_err(RequestAdmissionError::Distributed)?,
            ),
            None => None,
        };
        Ok(AdmissionPermit::from_parts(local, distributed))
    }
}

pub fn build_execution_runtime_router() -> Router {
    build_execution_runtime_router_with_request_concurrency_limit(None)
}

pub fn build_execution_runtime_router_with_request_concurrency_limit(
    limit: Option<usize>,
) -> Router {
    build_execution_runtime_router_with_request_gates(limit, None)
}

pub fn build_execution_runtime_router_with_request_gates(
    limit: Option<usize>,
    distributed_gate: Option<RuntimeSemaphore>,
) -> Router {
    let state = match distributed_gate {
        Some(gate) => ExecutionRuntimeAppState::with_request_concurrency_limit(limit)
            .with_distributed_request_gate(gate),
        None => ExecutionRuntimeAppState::with_request_concurrency_limit(limit),
    };
    middleware::apply_cf_header_stripping(
        Router::new()
            .route("/health", get(health))
            .route("/metrics", get(metrics))
            .route("/v1/execute/sync", post(execute_sync))
            .route("/v1/execute/stream", post(execute_stream))
            .with_state(state),
    )
}

pub async fn serve_execution_runtime_tcp(
    bind: &str,
    max_in_flight_requests: Option<usize>,
    distributed_request_gate: Option<RuntimeSemaphore>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(
        listener,
        build_execution_runtime_router_with_request_gates(
            max_in_flight_requests,
            distributed_request_gate,
        ),
    )
    .await?;
    Ok(())
}

#[cfg(unix)]
pub async fn serve_execution_runtime_unix(
    socket_path: &Path,
    max_in_flight_requests: Option<usize>,
    distributed_request_gate: Option<RuntimeSemaphore>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }

    let listener = tokio::net::UnixListener::bind(socket_path)?;
    axum::serve(
        listener,
        build_execution_runtime_router_with_request_gates(
            max_in_flight_requests,
            distributed_request_gate,
        ),
    )
    .await?;
    Ok(())
}

#[cfg(not(unix))]
pub async fn serve_execution_runtime_unix(
    _socket_path: &Path,
    _max_in_flight_requests: Option<usize>,
    _distributed_request_gate: Option<RuntimeSemaphore>,
) -> Result<(), Box<dyn std::error::Error>> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Unix sockets are not supported on this platform",
    )
    .into())
}

async fn health(State(state): State<ExecutionRuntimeAppState>) -> impl IntoResponse {
    let request_concurrency = state.request_concurrency_snapshot().map(|snapshot| {
        json!({
            "limit": snapshot.limit,
            "in_flight": snapshot.in_flight,
            "available_permits": snapshot.available_permits,
            "high_watermark": snapshot.high_watermark,
            "rejected": snapshot.rejected,
        })
    });
    let distributed_request_concurrency = state
        .distributed_request_concurrency_snapshot()
        .await
        .ok()
        .flatten()
        .map(|snapshot| {
            json!({
                "limit": snapshot.limit,
                "in_flight": snapshot.in_flight,
                "available_permits": snapshot.available_permits,
                "high_watermark": snapshot.high_watermark,
                "rejected": snapshot.rejected,
            })
        });
    Json(json!({
        "status": "ok",
        "component": EXECUTION_RUNTIME_COMPONENT,
        "request_concurrency": request_concurrency,
        "distributed_request_concurrency": distributed_request_concurrency,
    }))
}

async fn metrics(State(state): State<ExecutionRuntimeAppState>) -> Response {
    prometheus_response(&state.metric_samples().await)
}

async fn execute_sync(
    State(state): State<ExecutionRuntimeAppState>,
    request: Request,
) -> Result<Response, ExecutionRuntimeAppError> {
    let request_permit = acquire_request_permit(&state).await?;
    let plan = parse_request_json::<ExecutionPlan>(request).await?;
    let result = state
        .execution_runtime
        .execute_sync(&plan)
        .await
        .map_err(|err| ExecutionRuntimeAppError(ExecutionRuntimeServerError::Transport(err)))?;
    Ok(maybe_hold_axum_response_permit(
        Json(result).into_response(),
        request_permit,
    ))
}

async fn execute_stream(
    State(state): State<ExecutionRuntimeAppState>,
    request: Request,
) -> Result<Response, ExecutionRuntimeAppError> {
    let request_permit = acquire_request_permit(&state).await?;
    let plan = parse_request_json::<ExecutionPlan>(request).await?;
    let execution = state
        .execution_runtime
        .execute_stream(&plan)
        .await
        .map_err(|err| ExecutionRuntimeAppError(ExecutionRuntimeServerError::Transport(err)))?;

    let mut response = Response::new(Body::from_stream(build_direct_execution_frame_stream(
        execution,
    )));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/x-ndjson"),
    );
    Ok(maybe_hold_axum_response_permit(response, request_permit))
}

async fn acquire_request_permit(
    state: &ExecutionRuntimeAppState,
) -> Result<Option<AdmissionPermit>, ExecutionRuntimeAppError> {
    match state.try_acquire_request_permit().await {
        Ok(permit) => Ok(permit),
        Err(RequestAdmissionError::Local(ConcurrencyError::Saturated { gate, limit }))
        | Err(RequestAdmissionError::Distributed(RuntimeSemaphoreError::Saturated {
            gate,
            limit,
        }))
        | Err(RequestAdmissionError::Distributed(RuntimeSemaphoreError::Unavailable {
            gate,
            limit,
            ..
        })) => Err(ExecutionRuntimeAppError(
            ExecutionRuntimeServerError::Overloaded { gate, limit },
        )),
        Err(RequestAdmissionError::Local(ConcurrencyError::Closed { gate })) => Err(
            ExecutionRuntimeAppError(ExecutionRuntimeServerError::RequestRead(format!(
                "execution runtime request concurrency gate {gate} is closed"
            ))),
        ),
        Err(RequestAdmissionError::Distributed(RuntimeSemaphoreError::InvalidConfiguration(
            message,
        ))) => Err(ExecutionRuntimeAppError(
            ExecutionRuntimeServerError::RequestRead(message),
        )),
    }
}

#[derive(Debug)]
enum RequestAdmissionError {
    Local(ConcurrencyError),
    Distributed(RuntimeSemaphoreError),
}

async fn parse_request_json<T>(request: Request) -> Result<T, ExecutionRuntimeAppError>
where
    T: serde::de::DeserializeOwned,
{
    let body = to_bytes(
        request.into_body(),
        usize::try_from(crate::headers::max_request_body_bytes()).unwrap_or(usize::MAX),
    )
    .await
    .map_err(|err| {
        ExecutionRuntimeAppError(ExecutionRuntimeServerError::RequestRead(err.to_string()))
    })?;
    serde_json::from_slice(&body).map_err(|err| {
        ExecutionRuntimeAppError(ExecutionRuntimeServerError::InvalidRequestJson(err))
    })
}

fn build_overloaded_response(message: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": {
                "type": "overloaded",
                "message": message,
            }
        })),
    )
        .into_response()
}

#[derive(Debug, Error)]
enum ExecutionRuntimeServerError {
    #[error("failed to read execution runtime request body: {0}")]
    RequestRead(String),
    #[error("execution runtime request body is not valid JSON: {0}")]
    InvalidRequestJson(serde_json::Error),
    #[error("execution runtime overloaded: gate {gate} saturated at {limit}")]
    Overloaded { gate: &'static str, limit: usize },
    #[error(transparent)]
    Transport(#[from] ExecutionRuntimeTransportError),
}

#[derive(Debug)]
struct ExecutionRuntimeAppError(ExecutionRuntimeServerError);

impl IntoResponse for ExecutionRuntimeAppError {
    fn into_response(self) -> Response {
        let status_code = match self.0 {
            ExecutionRuntimeServerError::RequestRead(_)
            | ExecutionRuntimeServerError::InvalidRequestJson(_) => StatusCode::BAD_REQUEST,
            ExecutionRuntimeServerError::Overloaded { .. } => {
                return build_overloaded_response(&self.0.to_string());
            }
            ExecutionRuntimeServerError::Transport(
                ExecutionRuntimeTransportError::RequestBodyRequired
                | ExecutionRuntimeTransportError::BodyDecode(_)
                | ExecutionRuntimeTransportError::UnsupportedContentEncoding(_)
                | ExecutionRuntimeTransportError::InvalidMethod(_)
                | ExecutionRuntimeTransportError::InvalidHeaderName(_)
                | ExecutionRuntimeTransportError::InvalidHeaderValue(_)
                | ExecutionRuntimeTransportError::UnsupportedTransportProfile(_)
                | ExecutionRuntimeTransportError::BodyEncode(_),
            ) => StatusCode::BAD_REQUEST,
            ExecutionRuntimeServerError::Transport(
                ExecutionRuntimeTransportError::UpstreamHttpStatus { status_code, .. },
            ) => StatusCode::from_u16(status_code).unwrap_or(StatusCode::BAD_GATEWAY),
            ExecutionRuntimeServerError::Transport(
                ExecutionRuntimeTransportError::ClientBuild(_)
                | ExecutionRuntimeTransportError::BrowserClientBuild(_)
                | ExecutionRuntimeTransportError::BrowserBody(_)
                | ExecutionRuntimeTransportError::UpstreamRequest(_)
                | ExecutionRuntimeTransportError::UpstreamResponseTooLarge { .. }
                | ExecutionRuntimeTransportError::UpstreamResponseDecode { .. }
                | ExecutionRuntimeTransportError::RelayError(_)
                | ExecutionRuntimeTransportError::InvalidJson(_),
            ) => StatusCode::BAD_GATEWAY,
        };

        (
            status_code,
            Json(json!({
                "error": self.0.to_string(),
            })),
        )
            .into_response()
    }
}
