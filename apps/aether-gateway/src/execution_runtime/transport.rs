use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::error::Error as _;
use std::future::Future;
use std::io::Read;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex as StdMutex, OnceLock, RwLock as StdRwLock};
use std::time::{Duration, Instant};

use aether_contracts::{
    ExecutionPlan, ExecutionResponseBodyMode, ExecutionResponseObservation, ExecutionResult,
    ExecutionTelemetry, ResolvedTransportProfile, ResponseBody,
    EXECUTION_REQUEST_ACCEPT_INVALID_CERTS_HEADER, EXECUTION_REQUEST_FOLLOW_REDIRECTS_HEADER,
    EXECUTION_REQUEST_HTTP1_ONLY_HEADER, EXECUTION_RESPONSE_BODY_MODE_HEADER,
    TRANSPORT_BACKEND_BROWSER_WREQ, TRANSPORT_BACKEND_REQWEST_RUSTLS,
    TRANSPORT_HTTP_MODE_H2C_PRIOR_KNOWLEDGE, TRANSPORT_HTTP_MODE_HTTP1_ONLY,
};
use aether_http::{apply_http_client_config, HttpClientConfig};
use aether_runtime::{MetricKind, MetricSample};
use axum::body::Bytes;
use base64::Engine as _;
use flate2::read::{DeflateDecoder, GzDecoder};
use flate2::write::GzEncoder;
use flate2::Compression;
use futures_util::stream::FuturesUnordered;
use futures_util::StreamExt;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming as HyperIncomingBody;
use hyper::client::conn::http2::SendRequest as HyperH2cSendRequest;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client as HyperLegacyClient;
use hyper_util::rt::{TokioExecutor, TokioIo};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::redirect::Policy;
use serde_json::json;
use serde_json::Value;
use sha2::Digest as _;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio::sync::OnceCell as TokioOnceCell;

#[cfg(test)]
use crate::execution_runtime::remote_test_support::execute_sync_plan_via_remote_execution_runtime;
use crate::frontdoor_loop_guard::{
    configured_gateway_frontdoor_base_url, gateway_frontdoor_self_loop_guard_error,
};
use crate::stage_metrics::observe_gateway_stage_ms;
use crate::upstream_admission::UpstreamTargetAdmissionPermit;
use crate::{AppState, GatewayError};

const DEFAULT_STREAM_FIRST_BYTE_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_NON_STREAM_TOTAL_TIMEOUT_MS: u64 = 300_000;
const DEFAULT_CODEX_COMPACT_TOTAL_TIMEOUT_MS: u64 = 1_200_000;
const DIRECT_REQWEST_H2_CLIENT_SHARDS_ENV: &str = "AETHER_GATEWAY_DIRECT_REQWEST_H2_CLIENT_SHARDS";
const DIRECT_REQWEST_CLIENT_SHARDS_ENV: &str = "AETHER_GATEWAY_DIRECT_REQWEST_CLIENT_SHARDS";
const DIRECT_REQWEST_H2_TARGET_STREAMS_PER_CLIENT_ENV: &str =
    "AETHER_GATEWAY_DIRECT_REQWEST_H2_TARGET_STREAMS_PER_CLIENT";
const DIRECT_REQWEST_HTTP1_TARGET_STREAMS_PER_CLIENT_ENV: &str =
    "AETHER_GATEWAY_DIRECT_REQWEST_HTTP1_TARGET_STREAMS_PER_CLIENT";
const DIRECT_REQWEST_STREAM_HTTP_MODE_ENV: &str = "AETHER_GATEWAY_DIRECT_REQWEST_STREAM_HTTP_MODE";
const DIRECT_REQWEST_CACHE_PER_ORIGIN_ENV: &str = "AETHER_GATEWAY_DIRECT_REQWEST_CACHE_PER_ORIGIN";
const DIRECT_H2C_FAST_PATH_ENV: &str = "AETHER_GATEWAY_DIRECT_H2C_FAST_PATH";
const DIRECT_H2C_CLIENT_SHARDS_ENV: &str = "AETHER_GATEWAY_DIRECT_H2C_CLIENT_SHARDS";
const DIRECT_H2C_POOL_MAX_IDLE_PER_HOST_ENV: &str =
    "AETHER_GATEWAY_DIRECT_H2C_POOL_MAX_IDLE_PER_HOST";
const DIRECT_H2C_TARGET_STREAMS_PER_CLIENT_ENV: &str =
    "AETHER_GATEWAY_DIRECT_H2C_TARGET_STREAMS_PER_CLIENT";
const DIRECT_H2C_SENDER_SELECT_WINDOW_ENV: &str = "AETHER_GATEWAY_DIRECT_H2C_SENDER_SELECT_WINDOW";
const DIRECT_H2C_ADAPTIVE_WINDOW_ENV: &str = "AETHER_GATEWAY_DIRECT_H2C_ADAPTIVE_WINDOW";
const DIRECT_H2C_DRIVER_RUNTIME_THREADS_ENV: &str =
    "AETHER_GATEWAY_DIRECT_H2C_DRIVER_RUNTIME_THREADS";
const DIRECT_H2C_PREWARM_URLS_ENV: &str = "AETHER_GATEWAY_DIRECT_H2C_PREWARM_URLS";
const DIRECT_H2C_PREWARM_READY_ENV: &str = "AETHER_GATEWAY_DIRECT_H2C_PREWARM_READY";
const DIRECT_H2C_PREWARM_CONNECT_TIMEOUT_MS_ENV: &str =
    "AETHER_GATEWAY_DIRECT_H2C_PREWARM_CONNECT_TIMEOUT_MS";
const DIRECT_REQWEST_SYNC_WARM_CLIENTS_ENV: &str =
    "AETHER_GATEWAY_DIRECT_REQWEST_SYNC_WARM_CLIENTS";
const DIRECT_REQWEST_PREWARM_SYNC_CLIENTS_ENV: &str =
    "AETHER_GATEWAY_DIRECT_REQWEST_PREWARM_SYNC_CLIENTS";
const DEFAULT_H2_TARGET_STREAMS_PER_CLIENT: usize = 8;
const DEFAULT_HTTP1_TARGET_STREAMS_PER_CLIENT: usize = 512;
const DEFAULT_DIRECT_H2C_POOL_MAX_IDLE_PER_HOST: usize = 512;
const DEFAULT_DIRECT_H2C_TARGET_STREAMS_PER_CLIENT: usize = 128;
const DEFAULT_DIRECT_H2C_SENDER_SELECT_WINDOW: usize = 4;
const MAX_DIRECT_H2C_DRIVER_RUNTIME_THREADS: usize = 16;
const DIRECT_H2C_DRIVER_RUNTIME_MAX_BLOCKING_THREADS: usize = 16;
const DIRECT_H2C_DRIVER_RUNTIME_STACK_BYTES: usize = 2 * 1024 * 1024;
const DIRECT_H2C_DRIVER_RUNTIME_THREAD_NAME: &str = "aether-h2c-driver";
const DEFAULT_DIRECT_REQWEST_SYNC_WARM_CLIENTS: usize = 4;
const MAX_DIRECT_REQWEST_SYNC_WARM_CLIENTS: usize = 16;
const MAX_DIRECT_H2C_CLIENT_SHARDS: usize = 512;
const MAX_DIRECT_REQWEST_H2_CLIENT_SHARDS: usize = 2048;

type DirectHyperH2cRequestBody = Full<Bytes>;
type DirectHyperH2cClient = HyperLegacyClient<HttpConnector, DirectHyperH2cRequestBody>;
type DirectHyperH2cSender = HyperH2cSendRequest<DirectHyperH2cRequestBody>;
type DirectHyperH2cSenderCacheCell = TokioOnceCell<Arc<DirectHyperH2cSenderCacheEntry>>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DirectReqwestClientCacheKey {
    upstream_origin: Option<String>,
    pool_partition: Option<String>,
    connect_timeout_ms: Option<u64>,
    follow_redirects: bool,
    http1_only: bool,
    accept_invalid_certs: bool,
    transport_profile: Option<DirectReqwestTransportProfileCacheKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DirectReqwestTransportProfileCacheKey {
    profile_id: String,
    backend: String,
    http_mode: String,
    pool_scope: String,
    header_fingerprint: Option<String>,
    extra: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DirectHyperH2cClientCacheKey {
    upstream_origin: String,
    connect_timeout_ms: Option<u64>,
    pool_max_idle_per_host: usize,
}

struct DirectReqwestClientCacheEntry {
    clients: Vec<reqwest::Client>,
    next: AtomicU64,
    target_len: usize,
    warming: bool,
}

impl DirectReqwestClientCacheEntry {
    fn new(clients: Vec<reqwest::Client>, target_len: usize, warming: bool) -> Self {
        Self {
            clients,
            next: AtomicU64::new(0),
            target_len: target_len.max(1),
            warming,
        }
    }

    fn select(&self) -> reqwest::Client {
        if self.clients.len() <= 1 {
            return self
                .clients
                .first()
                .expect("direct reqwest client cache entry should contain a client")
                .clone();
        }
        let index = self.next.fetch_add(1, Ordering::Relaxed) as usize % self.clients.len();
        self.clients[index].clone()
    }

    fn len(&self) -> usize {
        self.clients.len()
    }

    fn should_warm(&self) -> bool {
        self.clients.len() < self.target_len && !self.warming
    }
}

struct DirectHyperH2cClientCacheEntry {
    clients: Vec<DirectHyperH2cClient>,
    next: AtomicU64,
    target_len: usize,
}

struct DirectHyperH2cSenderCacheEntry {
    senders: Vec<Arc<DirectHyperH2cSenderSlot>>,
    next: AtomicU64,
    target_len: usize,
}

impl DirectHyperH2cSenderCacheEntry {
    fn new(senders: Vec<DirectHyperH2cSender>, target_len: usize) -> Self {
        Self {
            senders: senders
                .into_iter()
                .map(DirectHyperH2cSenderSlot::new)
                .collect(),
            next: AtomicU64::new(0),
            target_len: target_len.max(1),
        }
    }

    fn select(&self) -> DirectHyperH2cSenderLease {
        if self.senders.len() <= 1 {
            let slot = self
                .senders
                .first()
                .expect("direct h2c sender cache entry should contain a sender")
                .clone();
            return DirectHyperH2cSenderLease::new(slot);
        }
        let start = self.next.fetch_add(1, Ordering::Relaxed) as usize;
        let window = direct_h2c_sender_select_window()
            .min(self.senders.len())
            .max(1);
        let mut selected_index = start % self.senders.len();
        let mut selected_load = self.senders[selected_index].in_flight();
        for offset in 1..window {
            let index = start.wrapping_add(offset) % self.senders.len();
            let load = self.senders[index].in_flight();
            if load < selected_load {
                selected_index = index;
                selected_load = load;
                if load == 0 {
                    break;
                }
            }
        }
        DirectHyperH2cSenderLease::new(Arc::clone(&self.senders[selected_index]))
    }

    fn len(&self) -> usize {
        self.senders.len()
    }

    fn in_flight(&self) -> u64 {
        self.senders.iter().map(|sender| sender.in_flight()).sum()
    }

    fn max_in_flight(&self) -> u64 {
        self.senders
            .iter()
            .map(|sender| sender.max_in_flight())
            .max()
            .unwrap_or(0)
    }
}

struct DirectHyperH2cSenderSlot {
    sender: DirectHyperH2cSender,
    in_flight: AtomicU64,
    max_in_flight: AtomicU64,
}

impl DirectHyperH2cSenderSlot {
    fn new(sender: DirectHyperH2cSender) -> Arc<Self> {
        Arc::new(Self {
            sender,
            in_flight: AtomicU64::new(0),
            max_in_flight: AtomicU64::new(0),
        })
    }

    fn acquire(self: &Arc<Self>) -> DirectHyperH2cSenderLease {
        let in_flight = self.in_flight.fetch_add(1, Ordering::AcqRel) + 1;
        self.max_in_flight.fetch_max(in_flight, Ordering::AcqRel);
        DirectHyperH2cSenderLease {
            sender: self.sender.clone(),
            slot: Some(Arc::clone(self)),
        }
    }

    fn in_flight(&self) -> u64 {
        self.in_flight.load(Ordering::Acquire)
    }

    fn max_in_flight(&self) -> u64 {
        self.max_in_flight.load(Ordering::Acquire)
    }
}

struct DirectHyperH2cSenderLease {
    sender: DirectHyperH2cSender,
    slot: Option<Arc<DirectHyperH2cSenderSlot>>,
}

impl DirectHyperH2cSenderLease {
    fn new(slot: Arc<DirectHyperH2cSenderSlot>) -> Self {
        slot.acquire()
    }

    fn sender(&mut self) -> &mut DirectHyperH2cSender {
        &mut self.sender
    }

    fn release(&mut self) {
        if let Some(slot) = self.slot.take() {
            slot.in_flight.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

impl Drop for DirectHyperH2cSenderLease {
    fn drop(&mut self) {
        self.release();
    }
}

impl DirectHyperH2cClientCacheEntry {
    fn new(clients: Vec<DirectHyperH2cClient>, target_len: usize) -> Self {
        Self {
            clients,
            next: AtomicU64::new(0),
            target_len: target_len.max(1),
        }
    }

    fn select(&self) -> DirectHyperH2cClient {
        if self.clients.len() <= 1 {
            return self
                .clients
                .first()
                .expect("direct h2c client cache entry should contain a client")
                .clone();
        }
        let index = self.next.fetch_add(1, Ordering::Relaxed) as usize % self.clients.len();
        self.clients[index].clone()
    }

    fn len(&self) -> usize {
        self.clients.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectReqwestStreamHttpMode {
    Http1,
    Auto,
}

static DIRECT_REQWEST_CLIENT_CACHE: LazyLock<
    StdMutex<HashMap<DirectReqwestClientCacheKey, DirectReqwestClientCacheEntry>>,
> = LazyLock::new(|| StdMutex::new(HashMap::new()));

static DIRECT_H2C_CLIENT_CACHE: LazyLock<
    StdMutex<HashMap<DirectHyperH2cClientCacheKey, DirectHyperH2cClientCacheEntry>>,
> = LazyLock::new(|| StdMutex::new(HashMap::new()));

static DIRECT_H2C_SENDER_CACHE: LazyLock<
    StdRwLock<HashMap<DirectHyperH2cClientCacheKey, Arc<DirectHyperH2cSenderCacheCell>>>,
> = LazyLock::new(|| StdRwLock::new(HashMap::new()));

static DIRECT_H2C_POOL_MAX_IDLE_PER_HOST: LazyLock<usize> = LazyLock::new(|| {
    env_positive_usize(DIRECT_H2C_POOL_MAX_IDLE_PER_HOST_ENV)
        .unwrap_or(DEFAULT_DIRECT_H2C_POOL_MAX_IDLE_PER_HOST)
});

static DIRECT_H2C_SENDER_SELECT_WINDOW: LazyLock<usize> = LazyLock::new(|| {
    env_positive_usize(DIRECT_H2C_SENDER_SELECT_WINDOW_ENV)
        .unwrap_or(DEFAULT_DIRECT_H2C_SENDER_SELECT_WINDOW)
        .clamp(1, MAX_DIRECT_H2C_CLIENT_SHARDS)
});

static DIRECT_REQWEST_STREAM_HTTP_MODE: LazyLock<DirectReqwestStreamHttpMode> =
    LazyLock::new(|| {
        std::env::var(DIRECT_REQWEST_STREAM_HTTP_MODE_ENV)
            .ok()
            .map(|value| parse_direct_reqwest_stream_http_mode(&value))
            .unwrap_or(DirectReqwestStreamHttpMode::Http1)
    });

#[derive(Debug, Default)]
struct DirectReqwestClientCacheMetrics {
    hits: AtomicU64,
    misses: AtomicU64,
    builds: AtomicU64,
    warm_enqueues: AtomicU64,
    warm_skipped_total: AtomicU64,
    http1_selections: AtomicU64,
    h2c_selections: AtomicU64,
    auto_selections: AtomicU64,
}

static DIRECT_REQWEST_CLIENT_CACHE_METRICS: LazyLock<DirectReqwestClientCacheMetrics> =
    LazyLock::new(DirectReqwestClientCacheMetrics::default);

#[derive(Debug, Default)]
struct DirectHyperH2cClientCacheMetrics {
    hits: AtomicU64,
    misses: AtomicU64,
    builds: AtomicU64,
}

static DIRECT_H2C_CLIENT_CACHE_METRICS: LazyLock<DirectHyperH2cClientCacheMetrics> =
    LazyLock::new(DirectHyperH2cClientCacheMetrics::default);

#[derive(Debug, Default)]
struct DirectHyperH2cSenderCacheMetrics {
    hits: AtomicU64,
    misses: AtomicU64,
    builds: AtomicU64,
    prewarm_requested: AtomicU64,
    prewarm_success: AtomicU64,
    prewarm_failed: AtomicU64,
}

static DIRECT_H2C_SENDER_CACHE_METRICS: LazyLock<DirectHyperH2cSenderCacheMetrics> =
    LazyLock::new(DirectHyperH2cSenderCacheMetrics::default);

#[derive(Debug, Clone, Default)]
pub struct DirectH2cSenderPrewarmReport {
    pub requested_urls: u64,
    pub unique_targets: u64,
    pub warmed_targets: u64,
    pub failed_targets: u64,
    pub ready_required: bool,
    pub first_error: Option<String>,
}

pub(crate) fn format_upstream_request_error(err: &reqwest::Error) -> String {
    let mut kinds = Vec::new();
    if err.is_connect() {
        kinds.push("connect");
    }
    if err.is_timeout() {
        kinds.push("timeout");
    }
    if err.is_redirect() {
        kinds.push("redirect");
    }
    if err.is_body() {
        kinds.push("body");
    }
    if err.is_decode() {
        kinds.push("decode");
    }
    if err.is_request() {
        kinds.push("request");
    }

    let mut detail = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !cause_text.is_empty() && !detail.contains(&cause_text) {
            detail.push_str(": ");
            detail.push_str(&cause_text);
        }
        source = cause.source();
    }

    if let Some(url) = err.url() {
        let (sanitized_detail, sanitized_url) =
            sanitize_upstream_request_error_detail(&detail, url.as_str());
        detail = sanitized_detail;
        detail.push_str(" [url=");
        detail.push_str(&sanitized_url);
        detail.push(']');
    }
    if !kinds.is_empty() {
        detail.push_str(" [kind=");
        detail.push_str(&kinds.join(","));
        detail.push(']');
    }

    detail
}

fn sanitize_upstream_request_error_detail(detail: &str, upstream_url: &str) -> (String, String) {
    let sanitized_url = sanitize_upstream_url_text(upstream_url);
    (detail.replace(upstream_url, &sanitized_url), sanitized_url)
}

fn sanitize_upstream_url_text(upstream_url: &str) -> String {
    if let Ok(mut parsed_url) = reqwest::Url::parse(upstream_url) {
        parsed_url.set_query(None);
        parsed_url.set_fragment(None);
        return parsed_url.to_string();
    }

    let suffix_offset = upstream_url
        .char_indices()
        .find_map(|(offset, character)| matches!(character, '?' | '#').then_some(offset))
        .unwrap_or(upstream_url.len());
    upstream_url[..suffix_offset].to_string()
}

pub(crate) fn format_wreq_upstream_request_error(err: &wreq::Error) -> String {
    let mut kinds = Vec::new();
    if err.is_connect() {
        kinds.push("connect");
    }
    if err.is_timeout() {
        kinds.push("timeout");
    }
    if err.is_redirect() {
        kinds.push("redirect");
    }
    if err.is_body() {
        kinds.push("body");
    }
    if err.is_decode() {
        kinds.push("decode");
    }
    if err.is_request() {
        kinds.push("request");
    }

    let mut detail = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !cause_text.is_empty() && !detail.contains(&cause_text) {
            detail.push_str(": ");
            detail.push_str(&cause_text);
        }
        source = cause.source();
    }

    if let Some(uri) = err.uri() {
        let uri = uri.to_string();
        let (sanitized_detail, sanitized_uri) =
            sanitize_upstream_request_error_detail(&detail, &uri);
        detail = sanitized_detail;
        detail.push_str(" [uri=");
        detail.push_str(&sanitized_uri);
        detail.push(']');
    }
    if !kinds.is_empty() {
        detail.push_str(" [kind=");
        detail.push_str(&kinds.join(","));
        detail.push(']');
    }

    detail
}

pub(crate) fn format_hyper_error_chain(err: &dyn std::error::Error) -> String {
    let mut detail = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !cause_text.is_empty() && !detail.contains(&cause_text) {
            detail.push_str(": ");
            detail.push_str(&cause_text);
        }
        source = cause.source();
    }
    detail
}

#[derive(Debug, Error)]
pub(crate) enum ExecutionRuntimeTransportError {
    #[error("request body must contain json_body or body_bytes_b64")]
    RequestBodyRequired,
    #[error("request body base64 is invalid: {0}")]
    BodyDecode(base64::DecodeError),
    #[error("request content-encoding is not supported: {0}")]
    UnsupportedContentEncoding(String),
    #[error("invalid method: {0}")]
    InvalidMethod(#[from] http::method::InvalidMethod),
    #[error("invalid upstream header name: {0}")]
    InvalidHeaderName(String),
    #[error("invalid upstream header value for {0}")]
    InvalidHeaderValue(String),
    #[error("unsupported transport profile backend: {0}")]
    UnsupportedTransportProfile(String),
    #[error("failed to encode request body: {0}")]
    BodyEncode(serde_json::Error),
    #[error("failed to build HTTP client: {0}")]
    ClientBuild(reqwest::Error),
    #[error("failed to build browser impersonation HTTP client: {0}")]
    BrowserClientBuild(wreq::Error),
    #[error("browser impersonation response body failed: {0}")]
    BrowserBody(String),
    #[error("{message}")]
    UpstreamHttpStatus { status_code: u16, message: String },
    #[error("failed to execute upstream request: {0}")]
    UpstreamRequest(String),
    #[error("upstream response {phase} body exceeds {limit_bytes} bytes")]
    UpstreamResponseTooLarge {
        phase: UpstreamResponseBodyPhase,
        limit_bytes: usize,
    },
    #[error("failed to decode upstream response body with content-encoding {encoding}: {message}")]
    UpstreamResponseDecode { encoding: String, message: String },
    #[error("hub relay request failed: {0}")]
    RelayError(String),
    #[error("upstream response is not valid JSON: {0}")]
    InvalidJson(serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpstreamResponseBodyPhase {
    Wire,
    Decoded,
}

impl std::fmt::Display for UpstreamResponseBodyPhase {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Wire => "wire",
            Self::Decoded => "decoded",
        })
    }
}

pub(crate) fn append_upstream_response_body_chunk(
    body: &mut Vec<u8>,
    chunk: &[u8],
) -> Result<(), ExecutionRuntimeTransportError> {
    append_upstream_response_body_chunk_with_limit(
        body,
        chunk,
        crate::headers::max_internal_buffered_body_bytes(),
    )
}

fn append_upstream_response_body_chunk_with_limit(
    body: &mut Vec<u8>,
    chunk: &[u8],
    limit_bytes: usize,
) -> Result<(), ExecutionRuntimeTransportError> {
    if body.len() > limit_bytes || chunk.len() > limit_bytes.saturating_sub(body.len()) {
        return Err(ExecutionRuntimeTransportError::UpstreamResponseTooLarge {
            phase: UpstreamResponseBodyPhase::Wire,
            limit_bytes,
        });
    }
    body.extend_from_slice(chunk);
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DirectSyncExecutionRuntime;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ExecutionTransportControls {
    follow_redirects: Option<bool>,
    http1_only: bool,
    accept_invalid_certs: bool,
}

pub(crate) enum DirectUpstreamResponse {
    Reqwest(reqwest::Response),
    HyperH2c(hyper::Response<HyperIncomingBody>),
    BrowserWreq(wreq::Response),
}

pub(crate) struct DirectUpstreamStreamExecution {
    pub(crate) request_id: String,
    pub(crate) candidate_id: Option<String>,
    pub(crate) status_code: u16,
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) provider_api_format: String,
    pub(crate) stream_summary_report_context: Value,
    pub(crate) prefetched_body: VecDeque<Result<Bytes, String>>,
    pub(crate) stream_precommit_committed: bool,
    pub(crate) response: DirectUpstreamResponse,
    pub(crate) started_at: Instant,
    pub(crate) response_observation: ExecutionResponseObservation,
    pub(crate) stream_first_byte_timeout: Option<Duration>,
    pub(crate) upstream_target_permit: Option<UpstreamTargetAdmissionPermit>,
}

#[derive(Debug, Clone)]
pub(crate) struct DirectSyncResponseStarted {
    pub(crate) status_code: u16,
    pub(crate) ttfb_ms: u64,
    pub(crate) response_observation: ExecutionResponseObservation,
}

impl DirectSyncExecutionRuntime {
    pub(crate) const fn new() -> Self {
        Self
    }

    pub(crate) async fn execute_sync(
        &self,
        plan: &ExecutionPlan,
    ) -> Result<ExecutionResult, ExecutionRuntimeTransportError> {
        self.execute_sync_with_response_started(plan, |_| {}).await
    }

    pub(crate) async fn execute_sync_with_response_started<F>(
        &self,
        plan: &ExecutionPlan,
        on_response_started: F,
    ) -> Result<ExecutionResult, ExecutionRuntimeTransportError>
    where
        F: FnOnce(DirectSyncResponseStarted),
    {
        let body_bytes = build_request_body(plan)?;

        let started_at = Instant::now();
        let request_started_at_unix_ms = crate::clock::current_unix_ms();
        let request_order_id = uuid::Uuid::now_v7().to_string();
        with_non_stream_total_timeout(plan, async move {
            let response = send_request_inner(plan, body_bytes, false).await?;
            let ttfb_ms = started_at.elapsed().as_millis() as u64;
            let response_headers_observed_at_unix_ms = crate::clock::current_unix_ms();
            let status_code = response.status_code();
            let headers = response.headers();
            let response_observation = ExecutionResponseObservation {
                request_started_at_unix_ms,
                response_headers_observed_at_unix_ms,
                request_order_id,
            };
            on_response_started(DirectSyncResponseStarted {
                status_code,
                ttfb_ms,
                response_observation: response_observation.clone(),
            });
            let (body_bytes, stream_ttfb_ms) =
                response.bytes_with_stream_timeout(plan, started_at).await?;
            let decoded_body_bytes = decode_response_body_bytes(&headers, &body_bytes)?;
            let elapsed_ms = started_at.elapsed().as_millis() as u64;
            let upstream_bytes = body_bytes.len() as u64;

            let body = build_execution_response_body(
                &headers,
                &body_bytes,
                decoded_body_bytes.as_ref(),
                plan.stream,
                execution_response_body_mode(plan),
            )?;

            Ok(ExecutionResult {
                request_id: plan.request_id.clone(),
                candidate_id: plan.candidate_id.clone(),
                status_code,
                headers,
                response_observation: Some(response_observation),
                body,
                telemetry: Some(ExecutionTelemetry {
                    ttfb_ms: stream_ttfb_ms.or(Some(ttfb_ms)),
                    elapsed_ms: Some(elapsed_ms),
                    upstream_bytes: Some(upstream_bytes),
                }),
                error: None,
            })
        })
        .await
    }

    pub(crate) async fn execute_stream(
        &self,
        plan: &ExecutionPlan,
    ) -> Result<DirectUpstreamStreamExecution, ExecutionRuntimeTransportError> {
        let build_body_started_at = Instant::now();
        let body_bytes = build_request_body(plan)?;
        observe_gateway_stage_ms(
            "direct_build_body",
            build_body_started_at.elapsed().as_millis() as u64,
        );

        let started_at = Instant::now();
        let request_started_at_unix_ms = crate::clock::current_unix_ms();
        let request_order_id = uuid::Uuid::now_v7().to_string();
        let response = send_request(plan, body_bytes).await?;
        observe_gateway_stage_ms(
            "direct_send_headers",
            started_at.elapsed().as_millis() as u64,
        );
        let status_code = response.status_code();
        let headers = response.headers();
        let response_headers_observed_at_unix_ms = crate::clock::current_unix_ms();

        let stream_summary_report_context = build_stream_summary_report_context(plan);

        Ok(DirectUpstreamStreamExecution {
            request_id: plan.request_id.clone(),
            candidate_id: plan.candidate_id.clone(),
            status_code,
            headers,
            provider_api_format: plan.provider_api_format.clone(),
            stream_summary_report_context,
            prefetched_body: VecDeque::new(),
            stream_precommit_committed: false,
            response: response.into_direct_upstream_response(),
            started_at,
            response_observation: ExecutionResponseObservation {
                request_started_at_unix_ms,
                response_headers_observed_at_unix_ms,
                request_order_id,
            },
            stream_first_byte_timeout: resolve_stream_first_byte_timeout(plan),
            upstream_target_permit: None,
        })
    }
}

pub(crate) async fn execute_sync_plan(
    state: &AppState,
    trace_id: Option<&str>,
    plan: &ExecutionPlan,
) -> Result<ExecutionResult, GatewayError> {
    execute_sync_plan_with_report_context(state, trace_id, plan, None).await
}

pub(crate) async fn execute_sync_plan_with_report_context(
    state: &AppState,
    trace_id: Option<&str>,
    plan: &ExecutionPlan,
    report_context: Option<&serde_json::Value>,
) -> Result<ExecutionResult, GatewayError> {
    #[cfg(test)]
    {
        let remote_execution_runtime_base_url = state
            .execution_runtime_override_base_url()
            .unwrap_or_default();
        if !remote_execution_runtime_base_url.trim().is_empty() {
            return execute_sync_plan_via_remote_execution_runtime(
                state,
                remote_execution_runtime_base_url,
                trace_id,
                plan,
            )
            .await;
        }
    }

    let _ = trace_id;
    DirectSyncExecutionRuntime::new()
        .execute_sync(plan)
        .await
        .map_err(|err| GatewayError::Internal(err.to_string()))
}

fn build_stream_summary_report_context(plan: &ExecutionPlan) -> Value {
    json!({
        "provider_api_format": plan.provider_api_format,
        "client_api_format": plan.client_api_format,
        "model": plan.model_name,
        "upstream_is_stream": plan.stream,
    })
}

pub(crate) async fn send_request(
    plan: &ExecutionPlan,
    body_bytes: Vec<u8>,
) -> Result<DirectHttpResponse, ExecutionRuntimeTransportError> {
    send_request_inner(plan, body_bytes, true).await
}

async fn send_request_inner(
    plan: &ExecutionPlan,
    body_bytes: Vec<u8>,
    apply_request_total_timeout: bool,
) -> Result<DirectHttpResponse, ExecutionRuntimeTransportError> {
    if let Some(detail) = gateway_frontdoor_self_loop_guard_error(plan.url.as_str()) {
        return Err(ExecutionRuntimeTransportError::UpstreamRequest(detail));
    }

    let prepare_started_at = Instant::now();
    let method = plan.method.parse::<reqwest::Method>()?;
    let transport_controls = resolve_execution_transport_controls(&plan.headers);
    let headers = build_request_headers(
        &plan.headers,
        plan.content_encoding.as_deref(),
        plan.body.body_bytes_b64.is_some(),
    )?;
    let total_timeout = if apply_request_total_timeout {
        resolve_non_stream_total_timeout(plan)
    } else {
        None
    };
    let stream_first_byte_timeout = resolve_stream_first_byte_timeout(plan);
    observe_gateway_stage_ms(
        "direct_request_prepare",
        prepare_started_at.elapsed().as_millis() as u64,
    );

    if transport_profile_uses_browser_wreq(plan.transport_profile.as_ref()) {
        return send_via_browser_wreq_transport(
            plan,
            method,
            headers,
            body_bytes,
            total_timeout,
            stream_first_byte_timeout,
            transport_controls,
            apply_request_total_timeout,
        )
        .await;
    }

    let direct_transport_controls =
        direct_reqwest_effective_transport_controls(plan, transport_controls);
    if direct_h2c_fast_path_applies(plan, direct_transport_controls) {
        return send_via_direct_h2c_fast_path(
            plan,
            method,
            headers,
            body_bytes,
            stream_first_byte_timeout,
        )
        .await
        .map(DirectHttpResponse::HyperH2c);
    }

    let client_select_started_at = Instant::now();
    let client = build_client(
        &plan.url,
        &plan.key_id,
        plan.timeouts.as_ref(),
        plan.transport_profile.as_ref(),
        direct_transport_controls,
    )?;
    observe_gateway_stage_ms(
        "direct_reqwest_client_select",
        client_select_started_at.elapsed().as_millis() as u64,
    );
    let request_build_started_at = Instant::now();
    let mut request = client.request(method, &plan.url);
    request = request.headers(headers).body(body_bytes);
    if let Some(timeout) = total_timeout {
        request = request.timeout(timeout);
    }
    observe_gateway_stage_ms(
        "direct_reqwest_request_build",
        request_build_started_at.elapsed().as_millis() as u64,
    );
    send_reqwest_request(request, stream_first_byte_timeout)
        .await
        .map(DirectHttpResponse::Reqwest)
}

pub(crate) enum DirectHttpResponse {
    Reqwest(reqwest::Response),
    HyperH2c(hyper::Response<HyperIncomingBody>),
    BrowserWreq(wreq::Response),
}

impl DirectHttpResponse {
    pub(crate) fn status_code(&self) -> u16 {
        match self {
            DirectHttpResponse::Reqwest(response) => response.status().as_u16(),
            DirectHttpResponse::HyperH2c(response) => response.status().as_u16(),
            DirectHttpResponse::BrowserWreq(response) => response.status().as_u16(),
        }
    }

    pub(crate) fn headers(&self) -> BTreeMap<String, String> {
        match self {
            DirectHttpResponse::Reqwest(response) => collect_response_headers(response.headers()),
            DirectHttpResponse::HyperH2c(response) => collect_response_headers(response.headers()),
            DirectHttpResponse::BrowserWreq(response) => {
                collect_response_headers(response.headers())
            }
        }
    }

    pub(crate) async fn bytes(self) -> Result<Bytes, ExecutionRuntimeTransportError> {
        let started_at = Instant::now();
        match self {
            DirectHttpResponse::Reqwest(response) => {
                collect_reqwest_stream_body(response, started_at, None)
                    .await
                    .map(|(body, _)| body)
            }
            DirectHttpResponse::HyperH2c(response) => {
                collect_hyper_stream_body(response, started_at, None)
                    .await
                    .map(|(body, _)| body)
            }
            DirectHttpResponse::BrowserWreq(response) => {
                collect_wreq_stream_body(response, started_at, None)
                    .await
                    .map(|(body, _)| body)
            }
        }
    }

    async fn bytes_with_stream_timeout(
        self,
        plan: &ExecutionPlan,
        started_at: Instant,
    ) -> Result<(Bytes, Option<u64>), ExecutionRuntimeTransportError> {
        if !plan.stream {
            return self.bytes().await.map(|bytes| (bytes, None));
        }

        let first_byte_timeout = resolve_stream_first_byte_timeout(plan);
        match self {
            DirectHttpResponse::Reqwest(response) => {
                collect_reqwest_stream_body(response, started_at, first_byte_timeout).await
            }
            DirectHttpResponse::HyperH2c(response) => {
                collect_hyper_stream_body(response, started_at, first_byte_timeout).await
            }
            DirectHttpResponse::BrowserWreq(response) => {
                collect_wreq_stream_body(response, started_at, first_byte_timeout).await
            }
        }
    }

    fn into_direct_upstream_response(self) -> DirectUpstreamResponse {
        match self {
            DirectHttpResponse::Reqwest(response) => DirectUpstreamResponse::Reqwest(response),
            DirectHttpResponse::HyperH2c(response) => DirectUpstreamResponse::HyperH2c(response),
            DirectHttpResponse::BrowserWreq(response) => {
                DirectUpstreamResponse::BrowserWreq(response)
            }
        }
    }
}

async fn await_stream_body_first_item<T, F>(
    future: F,
    started_at: Instant,
    timeout: Option<Duration>,
) -> Result<T, ExecutionRuntimeTransportError>
where
    F: Future<Output = T>,
{
    let Some(timeout) = timeout else {
        return Ok(future.await);
    };
    let Some(remaining) = timeout.checked_sub(started_at.elapsed()) else {
        return Err(ExecutionRuntimeTransportError::UpstreamRequest(
            stream_first_byte_timeout_message(timeout),
        ));
    };
    if remaining.is_zero() {
        return Err(ExecutionRuntimeTransportError::UpstreamRequest(
            stream_first_byte_timeout_message(timeout),
        ));
    }
    tokio::time::timeout(remaining, future).await.map_err(|_| {
        ExecutionRuntimeTransportError::UpstreamRequest(stream_first_byte_timeout_message(timeout))
    })
}

async fn collect_reqwest_stream_body(
    response: reqwest::Response,
    started_at: Instant,
    first_byte_timeout: Option<Duration>,
) -> Result<(Bytes, Option<u64>), ExecutionRuntimeTransportError> {
    let mut stream = response.bytes_stream();
    let mut body_bytes = Vec::new();
    let mut first_byte_ms = None;

    loop {
        let item = if first_byte_ms.is_none() {
            await_stream_body_first_item(stream.next(), started_at, first_byte_timeout).await?
        } else {
            stream.next().await
        };
        let Some(item) = item else {
            break;
        };
        let chunk = item.map_err(|err| {
            ExecutionRuntimeTransportError::UpstreamRequest(format_upstream_request_error(&err))
        })?;
        if first_byte_ms.is_none() && !chunk.is_empty() {
            first_byte_ms = Some(started_at.elapsed().as_millis() as u64);
        }
        append_upstream_response_body_chunk(&mut body_bytes, &chunk)?;
    }

    Ok((Bytes::from(body_bytes), first_byte_ms))
}

async fn collect_hyper_stream_body(
    response: hyper::Response<HyperIncomingBody>,
    started_at: Instant,
    first_byte_timeout: Option<Duration>,
) -> Result<(Bytes, Option<u64>), ExecutionRuntimeTransportError> {
    let mut stream = response.into_body().into_data_stream();
    let mut body_bytes = Vec::new();
    let mut first_byte_ms = None;

    loop {
        let item = if first_byte_ms.is_none() {
            await_stream_body_first_item(stream.next(), started_at, first_byte_timeout).await?
        } else {
            stream.next().await
        };
        let Some(item) = item else {
            break;
        };
        let chunk = item.map_err(|err| {
            ExecutionRuntimeTransportError::UpstreamRequest(format_hyper_error_chain(&err))
        })?;
        if first_byte_ms.is_none() && !chunk.is_empty() {
            first_byte_ms = Some(started_at.elapsed().as_millis() as u64);
        }
        append_upstream_response_body_chunk(&mut body_bytes, &chunk)?;
    }

    Ok((Bytes::from(body_bytes), first_byte_ms))
}

async fn collect_wreq_stream_body(
    response: wreq::Response,
    started_at: Instant,
    first_byte_timeout: Option<Duration>,
) -> Result<(Bytes, Option<u64>), ExecutionRuntimeTransportError> {
    let mut stream = response.bytes_stream();
    let mut body_bytes = Vec::new();
    let mut first_byte_ms = None;

    loop {
        let item = if first_byte_ms.is_none() {
            await_stream_body_first_item(stream.next(), started_at, first_byte_timeout).await?
        } else {
            stream.next().await
        };
        let Some(item) = item else {
            break;
        };
        let chunk = item.map_err(|err| {
            ExecutionRuntimeTransportError::BrowserBody(format_wreq_upstream_request_error(&err))
        })?;
        if first_byte_ms.is_none() && !chunk.is_empty() {
            first_byte_ms = Some(started_at.elapsed().as_millis() as u64);
        }
        append_upstream_response_body_chunk(&mut body_bytes, &chunk)?;
    }

    Ok((Bytes::from(body_bytes), first_byte_ms))
}

fn direct_h2c_fast_path_applies(
    plan: &ExecutionPlan,
    transport_controls: ExecutionTransportControls,
) -> bool {
    if !direct_h2c_fast_path_enabled()
        || !plan.stream
        || transport_controls.http1_only
        || transport_controls.accept_invalid_certs
        || !transport_profile_h2c_prior_knowledge(plan.transport_profile.as_ref())
    {
        return false;
    }

    reqwest::Url::parse(plan.url.as_str())
        .ok()
        .is_some_and(|url| url.scheme() == "http")
}

fn direct_h2c_fast_path_enabled() -> bool {
    std::env::var(DIRECT_H2C_FAST_PATH_ENV)
        .ok()
        .is_some_and(|value| matches_truthy_env_value(value.trim()))
}

pub(crate) async fn prewarm_direct_h2c_sender_cache_from_env(
) -> Result<Option<DirectH2cSenderPrewarmReport>, ExecutionRuntimeTransportError> {
    let urls = direct_h2c_prewarm_urls_from_env();
    if urls.is_empty() {
        return Ok(None);
    }

    let ready_required = direct_h2c_prewarm_ready_required();
    let report = prewarm_direct_h2c_sender_cache_urls(urls, ready_required).await;
    if ready_required && report.failed_targets > 0 {
        return Err(ExecutionRuntimeTransportError::UpstreamRequest(format!(
            "direct h2c sender prewarm failed for {}/{} targets{}",
            report.failed_targets,
            report.unique_targets,
            report
                .first_error
                .as_deref()
                .map(|err| format!(": {err}"))
                .unwrap_or_default()
        )));
    }
    Ok(Some(report))
}

async fn prewarm_direct_h2c_sender_cache_urls(
    urls: Vec<String>,
    ready_required: bool,
) -> DirectH2cSenderPrewarmReport {
    let started_at = Instant::now();
    let requested_urls = urls.len() as u64;
    DIRECT_H2C_SENDER_CACHE_METRICS
        .prewarm_requested
        .fetch_add(requested_urls, Ordering::Relaxed);

    let connect_timeout_ms =
        env_positive_usize(DIRECT_H2C_PREWARM_CONNECT_TIMEOUT_MS_ENV).map(|value| value as u64);
    let timeouts = connect_timeout_ms.map(|connect_ms| aether_contracts::ExecutionTimeouts {
        connect_ms: Some(connect_ms),
        ..Default::default()
    });
    let (keys, parse_failures, mut first_error) =
        direct_h2c_sender_prewarm_cache_keys(&urls, timeouts.as_ref());
    let unique_targets = keys.len() as u64;
    if parse_failures > 0 {
        DIRECT_H2C_SENDER_CACHE_METRICS
            .prewarm_failed
            .fetch_add(parse_failures, Ordering::Relaxed);
    }

    let mut warmed_targets = 0;
    let mut failed_targets = parse_failures;
    let mut pending = FuturesUnordered::new();
    for key in keys {
        pending.push(prewarm_direct_h2c_sender_cache_key(key));
    }

    while let Some(result) = pending.next().await {
        match result {
            Ok(()) => {
                warmed_targets += 1;
                DIRECT_H2C_SENDER_CACHE_METRICS
                    .prewarm_success
                    .fetch_add(1, Ordering::Relaxed);
            }
            Err(err) => {
                failed_targets += 1;
                DIRECT_H2C_SENDER_CACHE_METRICS
                    .prewarm_failed
                    .fetch_add(1, Ordering::Relaxed);
                if first_error.is_none() {
                    first_error = Some(err.to_string());
                }
            }
        }
    }

    observe_gateway_stage_ms(
        "direct_h2c_sender_cache_prewarm",
        started_at.elapsed().as_millis() as u64,
    );
    DirectH2cSenderPrewarmReport {
        requested_urls,
        unique_targets,
        warmed_targets,
        failed_targets,
        ready_required,
        first_error,
    }
}

async fn prewarm_direct_h2c_sender_cache_key(
    cache_key: DirectHyperH2cClientCacheKey,
) -> Result<(), ExecutionRuntimeTransportError> {
    let cell = direct_h2c_sender_cache_cell(&cache_key);
    cell.get_or_try_init(|| async {
        let target_len = direct_h2c_client_shard_count();
        build_direct_h2c_sender_cache_entry_from_cache_key(&cache_key, target_len)
            .await
            .map(Arc::new)
    })
    .await?;
    Ok(())
}

fn direct_h2c_sender_prewarm_cache_keys(
    urls: &[String],
    timeouts: Option<&aether_contracts::ExecutionTimeouts>,
) -> (Vec<DirectHyperH2cClientCacheKey>, u64, Option<String>) {
    let mut seen = HashSet::new();
    let mut keys = Vec::new();
    let mut failed = 0;
    let mut first_error = None;
    for url in urls {
        match direct_h2c_client_cache_key(url, timeouts) {
            Ok(key) => {
                if seen.insert(key.clone()) {
                    keys.push(key);
                }
            }
            Err(err) => {
                failed += 1;
                if first_error.is_none() {
                    first_error = Some(err.to_string());
                }
            }
        }
    }
    (keys, failed, first_error)
}

fn direct_h2c_prewarm_urls_from_env() -> Vec<String> {
    std::env::var(DIRECT_H2C_PREWARM_URLS_ENV)
        .ok()
        .map(|value| {
            value
                .split([',', ';', '\n', '\t', ' '])
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn direct_h2c_prewarm_ready_required() -> bool {
    std::env::var(DIRECT_H2C_PREWARM_READY_ENV)
        .ok()
        .is_some_and(|value| matches_truthy_env_value(value.trim()))
}

async fn cached_direct_h2c_sender(
    request_url: &str,
    timeouts: Option<&aether_contracts::ExecutionTimeouts>,
) -> Result<DirectHyperH2cSenderLease, ExecutionRuntimeTransportError> {
    let cache_key = direct_h2c_client_cache_key(request_url, timeouts)?;
    let cell = direct_h2c_sender_cache_cell(&cache_key);
    let entry = cell
        .get_or_try_init(|| async {
            let target_len = direct_h2c_client_shard_count();
            build_direct_h2c_sender_cache_entry_from_cache_key(&cache_key, target_len)
                .await
                .map(Arc::new)
        })
        .await?;
    Ok(entry.select())
}

fn direct_h2c_sender_cache_cell(
    cache_key: &DirectHyperH2cClientCacheKey,
) -> Arc<DirectHyperH2cSenderCacheCell> {
    let cache_lock_started_at = Instant::now();
    if let Ok(cache) = DIRECT_H2C_SENDER_CACHE.read() {
        if let Some(cell) = cache.get(cache_key) {
            let cell = Arc::clone(cell);
            drop(cache);
            observe_gateway_stage_ms(
                "direct_reqwest_client_cache_lock",
                cache_lock_started_at.elapsed().as_millis() as u64,
            );
            DIRECT_H2C_SENDER_CACHE_METRICS
                .hits
                .fetch_add(1, Ordering::Relaxed);
            return cell;
        }
    }

    // Recheck after acquiring the write lock so simultaneous first requests
    // still share one OnceCell and one connection warmup.
    if let Ok(mut cache) = DIRECT_H2C_SENDER_CACHE.write() {
        let (cell, hit) = match cache.get(cache_key) {
            Some(cell) => (Arc::clone(cell), true),
            None => {
                let cell = Arc::new(TokioOnceCell::new());
                cache.insert(cache_key.clone(), Arc::clone(&cell));
                (cell, false)
            }
        };
        drop(cache);
        observe_gateway_stage_ms(
            "direct_reqwest_client_cache_lock",
            cache_lock_started_at.elapsed().as_millis() as u64,
        );
        if hit {
            DIRECT_H2C_SENDER_CACHE_METRICS
                .hits
                .fetch_add(1, Ordering::Relaxed);
        } else {
            DIRECT_H2C_SENDER_CACHE_METRICS
                .misses
                .fetch_add(1, Ordering::Relaxed);
        }
        return cell;
    } else {
        observe_gateway_stage_ms(
            "direct_reqwest_client_cache_lock",
            cache_lock_started_at.elapsed().as_millis() as u64,
        );
        DIRECT_H2C_SENDER_CACHE_METRICS
            .misses
            .fetch_add(1, Ordering::Relaxed);
    }
    Arc::new(TokioOnceCell::new())
}

fn direct_h2c_client_cache_key(
    request_url: &str,
    timeouts: Option<&aether_contracts::ExecutionTimeouts>,
) -> Result<DirectHyperH2cClientCacheKey, ExecutionRuntimeTransportError> {
    let upstream_origin = direct_reqwest_upstream_origin(request_url).ok_or_else(|| {
        ExecutionRuntimeTransportError::UpstreamRequest(format!(
            "invalid h2c upstream origin: {request_url}"
        ))
    })?;
    Ok(DirectHyperH2cClientCacheKey {
        upstream_origin,
        connect_timeout_ms: timeouts.and_then(|timeouts| timeouts.connect_ms),
        pool_max_idle_per_host: direct_h2c_pool_max_idle_per_host(),
    })
}

async fn build_direct_h2c_sender_cache_entry_from_cache_key(
    cache_key: &DirectHyperH2cClientCacheKey,
    target_len: usize,
) -> Result<DirectHyperH2cSenderCacheEntry, ExecutionRuntimeTransportError> {
    let mut pending = FuturesUnordered::new();
    for _ in 0..target_len {
        pending.push(connect_direct_h2c_sender(cache_key));
    }

    let mut senders = Vec::with_capacity(target_len);
    while let Some(sender) = pending.next().await {
        senders.push(sender?);
        DIRECT_H2C_SENDER_CACHE_METRICS
            .builds
            .fetch_add(1, Ordering::Relaxed);
    }
    Ok(DirectHyperH2cSenderCacheEntry::new(senders, target_len))
}

async fn connect_direct_h2c_sender(
    cache_key: &DirectHyperH2cClientCacheKey,
) -> Result<DirectHyperH2cSender, ExecutionRuntimeTransportError> {
    let driver_runtime = configured_direct_h2c_driver_runtime()?;
    connect_direct_h2c_sender_on_runtime(cache_key, driver_runtime).await
}

async fn connect_direct_h2c_sender_on_runtime(
    cache_key: &DirectHyperH2cClientCacheKey,
    driver_runtime: Option<&'static tokio::runtime::Runtime>,
) -> Result<DirectHyperH2cSender, ExecutionRuntimeTransportError> {
    let Some(driver_runtime) = driver_runtime else {
        return connect_direct_h2c_sender_on_current_runtime(cache_key).await;
    };

    let cache_key = cache_key.clone();
    driver_runtime
        .handle()
        .spawn(async move { connect_direct_h2c_sender_on_current_runtime(&cache_key).await })
        .await
        .map_err(|err| {
            ExecutionRuntimeTransportError::UpstreamRequest(format!(
                "direct H2C connect task failed: {err}"
            ))
        })?
}

async fn connect_direct_h2c_sender_on_current_runtime(
    cache_key: &DirectHyperH2cClientCacheKey,
) -> Result<DirectHyperH2cSender, ExecutionRuntimeTransportError> {
    let upstream = reqwest::Url::parse(&cache_key.upstream_origin).map_err(|err| {
        ExecutionRuntimeTransportError::UpstreamRequest(format!(
            "invalid h2c upstream origin {}: {err}",
            cache_key.upstream_origin
        ))
    })?;
    let host = upstream.host_str().ok_or_else(|| {
        ExecutionRuntimeTransportError::UpstreamRequest(format!(
            "missing h2c upstream host: {}",
            cache_key.upstream_origin
        ))
    })?;
    let port = upstream.port_or_known_default().ok_or_else(|| {
        ExecutionRuntimeTransportError::UpstreamRequest(format!(
            "missing h2c upstream port: {}",
            cache_key.upstream_origin
        ))
    })?;
    let connect = TcpStream::connect((host, port));
    let stream = if let Some(timeout_ms) = cache_key.connect_timeout_ms {
        let timeout = Duration::from_millis(timeout_ms);
        tokio::time::timeout(timeout, connect)
            .await
            .map_err(|_| {
                ExecutionRuntimeTransportError::UpstreamRequest(stream_first_byte_timeout_message(
                    timeout,
                ))
            })?
            .map_err(|err| {
                ExecutionRuntimeTransportError::UpstreamRequest(format!(
                    "failed to connect h2c upstream {}: {err}",
                    cache_key.upstream_origin
                ))
            })?
    } else {
        connect.await.map_err(|err| {
            ExecutionRuntimeTransportError::UpstreamRequest(format!(
                "failed to connect h2c upstream {}: {err}",
                cache_key.upstream_origin
            ))
        })?
    };
    stream.set_nodelay(true).map_err(|err| {
        ExecutionRuntimeTransportError::UpstreamRequest(format!(
            "failed to configure h2c upstream socket {}: {err}",
            cache_key.upstream_origin
        ))
    })?;
    let io = TokioIo::new(stream);
    let mut builder = hyper::client::conn::http2::Builder::new(TokioExecutor::new());
    builder.adaptive_window(direct_h2c_adaptive_window_enabled());
    let (sender, connection) = builder.handshake(io).await.map_err(|err| {
        ExecutionRuntimeTransportError::UpstreamRequest(format_hyper_error_chain(&err))
    })?;
    // Connect, handshake, and drive the connection on the same runtime so the
    // socket remains registered with the reactor polling the H2 connection.
    spawn_direct_h2c_driver_task(None, async move {
        if let Err(err) = connection.await {
            tracing::debug!(
                error = %format_hyper_error_chain(&err),
                "direct h2c sender connection closed"
            );
        }
    });
    Ok(sender)
}

fn cached_direct_h2c_client(
    request_url: &str,
    timeouts: Option<&aether_contracts::ExecutionTimeouts>,
) -> Result<DirectHyperH2cClient, ExecutionRuntimeTransportError> {
    let cache_key = direct_h2c_client_cache_key(request_url, timeouts)?;

    let cache_lock_started_at = Instant::now();
    if let Ok(mut cache) = DIRECT_H2C_CLIENT_CACHE.lock() {
        observe_gateway_stage_ms(
            "direct_reqwest_client_cache_lock",
            cache_lock_started_at.elapsed().as_millis() as u64,
        );
        if let Some(entry) = cache.get(&cache_key) {
            DIRECT_H2C_CLIENT_CACHE_METRICS
                .hits
                .fetch_add(1, Ordering::Relaxed);
            return Ok(entry.select());
        }

        DIRECT_H2C_CLIENT_CACHE_METRICS
            .misses
            .fetch_add(1, Ordering::Relaxed);
        let target_len = direct_h2c_client_shard_count();
        let mut clients = Vec::with_capacity(target_len);
        for _ in 0..target_len {
            clients.push(build_direct_h2c_client_from_cache_key(&cache_key));
            DIRECT_H2C_CLIENT_CACHE_METRICS
                .builds
                .fetch_add(1, Ordering::Relaxed);
        }
        let entry = DirectHyperH2cClientCacheEntry::new(clients, target_len);
        let client = entry.select();
        cache.insert(cache_key, entry);
        return Ok(client);
    }

    observe_gateway_stage_ms(
        "direct_reqwest_client_cache_lock",
        cache_lock_started_at.elapsed().as_millis() as u64,
    );
    DIRECT_H2C_CLIENT_CACHE_METRICS
        .misses
        .fetch_add(1, Ordering::Relaxed);
    DIRECT_H2C_CLIENT_CACHE_METRICS
        .builds
        .fetch_add(1, Ordering::Relaxed);
    Ok(build_direct_h2c_client_from_cache_key(&cache_key))
}

fn build_direct_h2c_client_from_cache_key(
    cache_key: &DirectHyperH2cClientCacheKey,
) -> DirectHyperH2cClient {
    let mut connector = HttpConnector::new();
    connector.enforce_http(true);
    connector.set_nodelay(true);
    connector.set_connect_timeout(cache_key.connect_timeout_ms.map(Duration::from_millis));

    let mut builder = HyperLegacyClient::builder(TokioExecutor::new());
    builder.http2_only(true);
    builder.http2_adaptive_window(true);
    builder.pool_max_idle_per_host(cache_key.pool_max_idle_per_host);
    builder.build(connector)
}

fn direct_h2c_pool_max_idle_per_host() -> usize {
    *DIRECT_H2C_POOL_MAX_IDLE_PER_HOST
}

fn direct_h2c_client_shard_count() -> usize {
    if let Some(shards) = env_positive_usize(DIRECT_H2C_CLIENT_SHARDS_ENV) {
        return shards.clamp(1, MAX_DIRECT_H2C_CLIENT_SHARDS);
    }
    let target_gate_limit = crate::state::upstream_target_gate_limit_from_env()
        .unwrap_or_else(crate::state::upstream_target_gate_auto_limit);
    let streams_per_client = env_positive_usize(DIRECT_H2C_TARGET_STREAMS_PER_CLIENT_ENV)
        .unwrap_or(DEFAULT_DIRECT_H2C_TARGET_STREAMS_PER_CLIENT)
        .max(1);
    target_gate_limit
        .max(1)
        .div_ceil(streams_per_client)
        .clamp(1, MAX_DIRECT_H2C_CLIENT_SHARDS)
}

fn direct_h2c_sender_select_window() -> usize {
    *DIRECT_H2C_SENDER_SELECT_WINDOW
}

fn direct_h2c_adaptive_window_enabled() -> bool {
    std::env::var(DIRECT_H2C_ADAPTIVE_WINDOW_ENV)
        .ok()
        .map(|value| matches_truthy_env_value(value.trim()))
        .unwrap_or(true)
}

fn direct_h2c_driver_runtime_threads() -> Option<usize> {
    parse_direct_h2c_driver_runtime_threads(
        std::env::var(DIRECT_H2C_DRIVER_RUNTIME_THREADS_ENV)
            .ok()
            .as_deref(),
    )
}

fn parse_direct_h2c_driver_runtime_threads(value: Option<&str>) -> Option<usize> {
    value
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|threads| *threads > 0)
        .map(|threads| threads.clamp(1, MAX_DIRECT_H2C_DRIVER_RUNTIME_THREADS))
}

fn configured_direct_h2c_driver_runtime(
) -> Result<Option<&'static tokio::runtime::Runtime>, ExecutionRuntimeTransportError> {
    direct_h2c_driver_runtime_threads()
        .map(direct_h2c_driver_runtime)
        .transpose()
}

fn direct_h2c_driver_runtime(
    worker_threads: usize,
) -> Result<&'static tokio::runtime::Runtime, ExecutionRuntimeTransportError> {
    struct RuntimeEntry {
        runtime: &'static tokio::runtime::Runtime,
        worker_threads: usize,
    }

    static RUNTIME: OnceLock<Result<RuntimeEntry, String>> = OnceLock::new();
    let entry = RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(worker_threads)
            .max_blocking_threads(DIRECT_H2C_DRIVER_RUNTIME_MAX_BLOCKING_THREADS)
            .thread_name(DIRECT_H2C_DRIVER_RUNTIME_THREAD_NAME)
            .thread_stack_size(DIRECT_H2C_DRIVER_RUNTIME_STACK_BYTES)
            .build()
            .map(|runtime| RuntimeEntry {
                runtime: Box::leak(Box::new(runtime)),
                worker_threads,
            })
            .map_err(|err| format!("failed to build direct H2C driver runtime: {err}"))
    });
    match entry {
        Ok(entry) if entry.worker_threads == worker_threads => Ok(entry.runtime),
        Ok(entry) => Err(ExecutionRuntimeTransportError::UpstreamRequest(format!(
            "direct H2C driver runtime was initialized with {} worker threads, not {worker_threads}",
            entry.worker_threads
        ))),
        Err(err) => Err(ExecutionRuntimeTransportError::UpstreamRequest(err.clone())),
    }
}

fn spawn_direct_h2c_driver_task<F>(
    driver_runtime: Option<&'static tokio::runtime::Runtime>,
    task: F,
) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    match driver_runtime {
        Some(runtime) => runtime.handle().spawn(task),
        None => tokio::spawn(task),
    }
}

async fn send_via_direct_h2c_fast_path(
    plan: &ExecutionPlan,
    method: reqwest::Method,
    headers: HeaderMap,
    body_bytes: Vec<u8>,
    stream_first_byte_timeout: Option<Duration>,
) -> Result<hyper::Response<HyperIncomingBody>, ExecutionRuntimeTransportError> {
    let client_select_started_at = Instant::now();
    let sender = cached_direct_h2c_sender(&plan.url, plan.timeouts.as_ref()).await?;
    observe_gateway_stage_ms(
        "direct_h2c_client_select",
        client_select_started_at.elapsed().as_millis() as u64,
    );

    let request_build_started_at = Instant::now();
    let uri = plan.url.parse::<hyper::Uri>().map_err(|err| {
        ExecutionRuntimeTransportError::UpstreamRequest(format!("invalid h2c upstream uri: {err}"))
    })?;
    let authority = uri
        .authority()
        .map(|authority| authority.as_str().to_string());
    let mut builder = hyper::Request::builder().method(method.as_str()).uri(uri);
    {
        let target_headers = builder.headers_mut().ok_or_else(|| {
            ExecutionRuntimeTransportError::UpstreamRequest(
                "failed to prepare h2c request headers".to_string(),
            )
        })?;
        *target_headers = headers;
        if !target_headers.contains_key(reqwest::header::HOST) {
            if let Some(authority) = authority.as_deref() {
                let value = HeaderValue::from_str(authority).map_err(|_| {
                    ExecutionRuntimeTransportError::InvalidHeaderValue("host".to_string())
                })?;
                target_headers.insert(reqwest::header::HOST, value);
            }
        }
    }
    let request = builder
        .body(Full::new(Bytes::from(body_bytes)))
        .map_err(|err| {
            ExecutionRuntimeTransportError::UpstreamRequest(format!(
                "failed to build h2c request: {err}"
            ))
        })?;
    observe_gateway_stage_ms(
        "direct_h2c_request_build",
        request_build_started_at.elapsed().as_millis() as u64,
    );

    send_hyper_h2c_request(sender, request, stream_first_byte_timeout).await
}

async fn send_hyper_h2c_request(
    mut sender: DirectHyperH2cSenderLease,
    request: hyper::Request<DirectHyperH2cRequestBody>,
    stream_first_byte_timeout: Option<Duration>,
) -> Result<hyper::Response<HyperIncomingBody>, ExecutionRuntimeTransportError> {
    let started_at = Instant::now();
    let deadline = stream_first_byte_timeout.map(|timeout| (timeout, Instant::now() + timeout));

    let ready_started_at = Instant::now();
    let ready_result = if let Some((timeout, deadline)) = deadline {
        match direct_h2c_remaining_timeout(deadline) {
            Some(remaining) => match tokio::time::timeout(remaining, sender.sender().ready()).await
            {
                Ok(Ok(())) => Ok(()),
                Ok(Err(err)) => Err(ExecutionRuntimeTransportError::UpstreamRequest(
                    format_hyper_error_chain(&err),
                )),
                Err(_) => Err(ExecutionRuntimeTransportError::UpstreamRequest(
                    stream_first_byte_timeout_message(timeout),
                )),
            },
            None => Err(ExecutionRuntimeTransportError::UpstreamRequest(
                stream_first_byte_timeout_message(timeout),
            )),
        }
    } else {
        sender.sender().ready().await.map_err(|err| {
            ExecutionRuntimeTransportError::UpstreamRequest(format_hyper_error_chain(&err))
        })
    };
    observe_gateway_stage_ms(
        "direct_h2c_sender_ready_wait",
        ready_started_at.elapsed().as_millis() as u64,
    );
    ready_result?;

    let headers_started_at = Instant::now();
    let dispatch_started_at = Instant::now();
    let response_future = sender.sender().send_request(request);
    observe_gateway_stage_ms(
        "direct_h2c_request_dispatch",
        dispatch_started_at.elapsed().as_millis() as u64,
    );

    let response_headers_started_at = Instant::now();
    let response_result = if let Some((timeout, deadline)) = deadline {
        match direct_h2c_remaining_timeout(deadline) {
            Some(remaining) => match tokio::time::timeout(remaining, response_future).await {
                Ok(Ok(response)) => Ok(response),
                Ok(Err(err)) => Err(ExecutionRuntimeTransportError::UpstreamRequest(
                    format_hyper_error_chain(&err),
                )),
                Err(_) => Err(ExecutionRuntimeTransportError::UpstreamRequest(
                    stream_first_byte_timeout_message(timeout),
                )),
            },
            None => Err(ExecutionRuntimeTransportError::UpstreamRequest(
                stream_first_byte_timeout_message(timeout),
            )),
        }
    } else {
        response_future.await.map_err(|err| {
            ExecutionRuntimeTransportError::UpstreamRequest(format_hyper_error_chain(&err))
        })
    };
    observe_gateway_stage_ms(
        "direct_h2c_response_headers_wait",
        response_headers_started_at.elapsed().as_millis() as u64,
    );
    observe_gateway_stage_ms(
        "direct_h2c_request_headers_wait",
        headers_started_at.elapsed().as_millis() as u64,
    );
    let response = response_result?;
    sender.release();
    observe_gateway_stage_ms(
        "direct_h2c_request_send",
        started_at.elapsed().as_millis() as u64,
    );
    Ok(response)
}

fn direct_h2c_remaining_timeout(deadline: Instant) -> Option<Duration> {
    deadline.checked_duration_since(Instant::now())
}

async fn send_via_browser_wreq_transport(
    plan: &ExecutionPlan,
    method: reqwest::Method,
    headers: HeaderMap,
    body_bytes: Vec<u8>,
    total_timeout: Option<Duration>,
    stream_first_byte_timeout: Option<Duration>,
    transport_controls: ExecutionTransportControls,
    apply_request_total_timeout: bool,
) -> Result<DirectHttpResponse, ExecutionRuntimeTransportError> {
    let profile = plan.transport_profile.as_ref().ok_or_else(|| {
        ExecutionRuntimeTransportError::UnsupportedTransportProfile(String::new())
    })?;
    let client = build_browser_wreq_client(
        plan.timeouts.as_ref(),
        profile,
        transport_controls,
        apply_request_total_timeout && !plan.stream,
    )?;
    let method = wreq::Method::from_bytes(method.as_str().as_bytes())
        .map_err(ExecutionRuntimeTransportError::InvalidMethod)?;
    let mut request = client
        .request(method, plan.url.as_str())
        .headers(headers)
        .body(body_bytes);
    if let Some(timeout) = total_timeout {
        request = request.timeout(timeout);
    }
    send_wreq_request(request, stream_first_byte_timeout)
        .await
        .map(DirectHttpResponse::BrowserWreq)
}

pub(crate) fn build_request_body(
    plan: &ExecutionPlan,
) -> Result<Vec<u8>, ExecutionRuntimeTransportError> {
    let mut body_bytes = if let Some(json_body) = plan.body.json_body.clone() {
        serde_json::to_vec(&json_body).map_err(ExecutionRuntimeTransportError::BodyEncode)?
    } else if let Some(body_b64) = plan.body.body_bytes_b64.as_deref() {
        base64::engine::general_purpose::STANDARD
            .decode(body_b64)
            .map_err(ExecutionRuntimeTransportError::BodyDecode)?
    } else {
        Vec::new()
    };

    if plan.body.json_body.is_some() {
        body_bytes = match normalize_content_encoding(plan.content_encoding.as_deref()).as_deref() {
            Some("gzip") => gzip_bytes(&body_bytes)?,
            Some("zstd") => zstd_bytes(&body_bytes)?,
            _ => body_bytes,
        };
    }

    Ok(body_bytes)
}

fn normalize_content_encoding(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

fn gzip_bytes(body_bytes: &[u8]) -> Result<Vec<u8>, ExecutionRuntimeTransportError> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(body_bytes)
        .map_err(|err| ExecutionRuntimeTransportError::RelayError(err.to_string()))?;
    encoder
        .finish()
        .map_err(|err| ExecutionRuntimeTransportError::RelayError(err.to_string()))
}

fn zstd_bytes(body_bytes: &[u8]) -> Result<Vec<u8>, ExecutionRuntimeTransportError> {
    zstd::stream::encode_all(std::io::Cursor::new(body_bytes), 3)
        .map_err(|err| ExecutionRuntimeTransportError::RelayError(err.to_string()))
}

pub(crate) fn resolve_non_stream_total_timeout_for_request(
    is_stream: bool,
    provider_api_format: &str,
    timeouts: Option<&aether_contracts::ExecutionTimeouts>,
) -> Option<Duration> {
    if is_stream {
        return None;
    }
    let default_timeout_ms =
        if crate::ai_serving::is_openai_responses_compact_format(provider_api_format) {
            DEFAULT_CODEX_COMPACT_TOTAL_TIMEOUT_MS
        } else {
            DEFAULT_NON_STREAM_TOTAL_TIMEOUT_MS
        };
    let timeout_ms = timeouts
        .and_then(|timeouts| timeouts.total_ms)
        .unwrap_or(default_timeout_ms);
    Some(Duration::from_millis(timeout_ms.max(1)))
}

fn resolve_non_stream_total_timeout(plan: &ExecutionPlan) -> Option<Duration> {
    resolve_non_stream_total_timeout_for_request(
        plan.stream,
        &plan.provider_api_format,
        plan.timeouts.as_ref(),
    )
}

pub(crate) fn resolve_stream_first_byte_timeout_for_request(
    is_stream: bool,
    timeouts: Option<&aether_contracts::ExecutionTimeouts>,
) -> Option<Duration> {
    if !is_stream {
        return None;
    }
    let timeout_ms = timeouts
        .and_then(|timeouts| timeouts.first_byte_ms)
        .unwrap_or(DEFAULT_STREAM_FIRST_BYTE_TIMEOUT_MS);
    Some(Duration::from_millis(timeout_ms.max(1)))
}

pub(crate) fn resolve_stream_first_byte_timeout(plan: &ExecutionPlan) -> Option<Duration> {
    resolve_stream_first_byte_timeout_for_request(plan.stream, plan.timeouts.as_ref())
}

pub(crate) async fn with_non_stream_total_timeout<T, F>(
    plan: &ExecutionPlan,
    future: F,
) -> Result<T, ExecutionRuntimeTransportError>
where
    F: Future<Output = Result<T, ExecutionRuntimeTransportError>>,
{
    let Some(timeout) = resolve_non_stream_total_timeout(plan) else {
        return future.await;
    };

    match tokio::time::timeout(timeout, future).await {
        Ok(result) => result,
        Err(_) => Err(ExecutionRuntimeTransportError::UpstreamRequest(
            non_stream_total_timeout_message(timeout),
        )),
    }
}

async fn send_reqwest_request(
    request: reqwest::RequestBuilder,
    stream_first_byte_timeout: Option<Duration>,
) -> Result<reqwest::Response, ExecutionRuntimeTransportError> {
    let started_at = Instant::now();
    if let Some(timeout) = stream_first_byte_timeout {
        return match tokio::time::timeout(timeout, request.send()).await {
            Ok(Ok(response)) => {
                observe_gateway_stage_ms(
                    "direct_reqwest_request_send",
                    started_at.elapsed().as_millis() as u64,
                );
                Ok(response)
            }
            Ok(Err(error)) => Err(ExecutionRuntimeTransportError::UpstreamRequest(
                format_upstream_request_error(&error),
            )),
            Err(_) => Err(ExecutionRuntimeTransportError::UpstreamRequest(
                stream_first_byte_timeout_message(timeout),
            )),
        };
    }

    let response = request.send().await.map_err(|err| {
        ExecutionRuntimeTransportError::UpstreamRequest(format_upstream_request_error(&err))
    })?;
    observe_gateway_stage_ms(
        "direct_reqwest_request_send",
        started_at.elapsed().as_millis() as u64,
    );
    Ok(response)
}

async fn send_wreq_request(
    request: wreq::RequestBuilder,
    stream_first_byte_timeout: Option<Duration>,
) -> Result<wreq::Response, ExecutionRuntimeTransportError> {
    if let Some(timeout) = stream_first_byte_timeout {
        return match tokio::time::timeout(timeout, request.send()).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(error)) => Err(ExecutionRuntimeTransportError::UpstreamRequest(
                format_wreq_upstream_request_error(&error),
            )),
            Err(_) => Err(ExecutionRuntimeTransportError::UpstreamRequest(
                stream_first_byte_timeout_message(timeout),
            )),
        };
    }

    request.send().await.map_err(|err| {
        ExecutionRuntimeTransportError::UpstreamRequest(format_wreq_upstream_request_error(&err))
    })
}

fn non_stream_total_timeout_message(timeout: Duration) -> String {
    format!(
        "provider non-stream request total timeout after {} ms",
        timeout.as_millis()
    )
}

pub(crate) fn stream_first_byte_timeout_message(timeout: Duration) -> String {
    format!(
        "provider stream first byte timeout after {} ms",
        timeout.as_millis()
    )
}

fn build_client(
    request_url: &str,
    key_id: &str,
    timeouts: Option<&aether_contracts::ExecutionTimeouts>,
    transport_profile: Option<&ResolvedTransportProfile>,
    transport_controls: ExecutionTransportControls,
) -> Result<reqwest::Client, ExecutionRuntimeTransportError> {
    validate_reqwest_transport_profile(transport_profile)?;
    let cache_key = direct_reqwest_client_cache_key(
        request_url,
        key_id,
        timeouts,
        transport_profile,
        transport_controls,
    );
    cached_direct_reqwest_client(cache_key)
}

fn direct_reqwest_effective_transport_controls(
    plan: &ExecutionPlan,
    mut transport_controls: ExecutionTransportControls,
) -> ExecutionTransportControls {
    if transport_controls.http1_only || !plan.stream {
        return transport_controls;
    }
    if transport_profile_h2c_prior_knowledge(plan.transport_profile.as_ref()) {
        return transport_controls;
    }
    if direct_reqwest_stream_http_mode() == DirectReqwestStreamHttpMode::Http1 {
        transport_controls.http1_only = true;
    }
    transport_controls
}

fn direct_reqwest_stream_http_mode() -> DirectReqwestStreamHttpMode {
    *DIRECT_REQWEST_STREAM_HTTP_MODE
}

fn parse_direct_reqwest_stream_http_mode(value: &str) -> DirectReqwestStreamHttpMode {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" | "profile" | "provider" => DirectReqwestStreamHttpMode::Auto,
        _ => DirectReqwestStreamHttpMode::Http1,
    }
}

pub(crate) fn prewarm_direct_reqwest_client_cache_for_plan(plan: &ExecutionPlan) {
    match try_prewarm_direct_reqwest_client_cache_for_plan(plan) {
        Ok(true) => {}
        Ok(false) => {}
        Err(err) => {
            tracing::debug!(
                error = ?err,
                request_id = %plan.request_id,
                candidate_id = ?plan.candidate_id,
                provider_id = %plan.provider_id,
                endpoint_id = %plan.endpoint_id,
                key_partition = ?direct_reqwest_pool_partition(
                    plan.transport_profile.as_ref(),
                    &plan.key_id,
                ),
                "gateway direct reqwest client prewarm skipped"
            );
        }
    }
}

fn try_prewarm_direct_reqwest_client_cache_for_plan(
    plan: &ExecutionPlan,
) -> Result<bool, ExecutionRuntimeTransportError> {
    if transport_profile_uses_browser_wreq(plan.transport_profile.as_ref()) {
        return Ok(false);
    }
    let transport_controls = direct_reqwest_effective_transport_controls(
        plan,
        resolve_execution_transport_controls(&plan.headers),
    );
    if direct_h2c_fast_path_applies(plan, transport_controls) {
        return Ok(false);
    }
    validate_reqwest_transport_profile(plan.transport_profile.as_ref())?;
    let cache_key = direct_reqwest_client_cache_key(
        &plan.url,
        &plan.key_id,
        plan.timeouts.as_ref(),
        plan.transport_profile.as_ref(),
        transport_controls,
    );
    prewarm_direct_reqwest_client_cache(cache_key)?;
    Ok(true)
}

fn prewarm_direct_reqwest_client_cache(
    cache_key: DirectReqwestClientCacheKey,
) -> Result<(), ExecutionRuntimeTransportError> {
    let mut warm_after_unlock = None;
    let cache_lock_started_at = Instant::now();
    if let Ok(mut cache) = DIRECT_REQWEST_CLIENT_CACHE.lock() {
        observe_gateway_stage_ms(
            "direct_reqwest_client_cache_lock",
            cache_lock_started_at.elapsed().as_millis() as u64,
        );
        if let Some(entry) = cache.get_mut(&cache_key) {
            if entry.should_warm() {
                entry.warming = true;
                warm_after_unlock = Some((cache_key.clone(), entry.len(), entry.target_len));
            }
            drop(cache);
            if let Some((cache_key, existing_len, target_len)) = warm_after_unlock {
                let spawned = spawn_direct_reqwest_client_cache_warm(
                    cache_key.clone(),
                    existing_len,
                    target_len,
                );
                if !spawned {
                    mark_direct_reqwest_client_cache_not_warming(&cache_key);
                }
            }
            return Ok(());
        }

        let target_len = direct_reqwest_client_shard_count(&cache_key);
        let initial_len = direct_reqwest_prewarm_client_shard_count(target_len);
        let mut clients = Vec::with_capacity(initial_len);
        for _ in 0..initial_len {
            clients.push(build_direct_reqwest_client_from_cache_key(&cache_key)?);
            DIRECT_REQWEST_CLIENT_CACHE_METRICS
                .builds
                .fetch_add(1, Ordering::Relaxed);
        }
        let entry =
            DirectReqwestClientCacheEntry::new(clients, target_len, target_len > initial_len);
        let warm_key = (target_len > initial_len).then(|| cache_key.clone());
        cache.insert(cache_key, entry);
        if let Some(warm_key) = warm_key {
            warm_after_unlock = Some((warm_key, initial_len, target_len));
        }
        drop(cache);
        if let Some((cache_key, existing_len, target_len)) = warm_after_unlock {
            let spawned =
                spawn_direct_reqwest_client_cache_warm(cache_key.clone(), existing_len, target_len);
            if !spawned {
                mark_direct_reqwest_client_cache_not_warming(&cache_key);
            }
        }
    } else {
        observe_gateway_stage_ms(
            "direct_reqwest_client_cache_lock",
            cache_lock_started_at.elapsed().as_millis() as u64,
        );
    }
    Ok(())
}

fn cached_direct_reqwest_client(
    cache_key: DirectReqwestClientCacheKey,
) -> Result<reqwest::Client, ExecutionRuntimeTransportError> {
    let mut warm_after_unlock = None;
    let cache_lock_started_at = Instant::now();
    if let Ok(mut cache) = DIRECT_REQWEST_CLIENT_CACHE.lock() {
        observe_gateway_stage_ms(
            "direct_reqwest_client_cache_lock",
            cache_lock_started_at.elapsed().as_millis() as u64,
        );
        if let Some(entry) = cache.get_mut(&cache_key) {
            DIRECT_REQWEST_CLIENT_CACHE_METRICS
                .hits
                .fetch_add(1, Ordering::Relaxed);
            record_direct_reqwest_client_protocol_selection(&cache_key);
            let client = entry.select();
            if entry.should_warm() {
                entry.warming = true;
                warm_after_unlock = Some((cache_key.clone(), entry.len(), entry.target_len));
            }
            drop(cache);
            if let Some((cache_key, existing_len, target_len)) = warm_after_unlock {
                let spawned = spawn_direct_reqwest_client_cache_warm(
                    cache_key.clone(),
                    existing_len,
                    target_len,
                );
                if !spawned {
                    mark_direct_reqwest_client_cache_not_warming(&cache_key);
                }
            }
            return Ok(client);
        }
        DIRECT_REQWEST_CLIENT_CACHE_METRICS
            .misses
            .fetch_add(1, Ordering::Relaxed);
        let target_len = direct_reqwest_client_shard_count(&cache_key);
        let initial_len = direct_reqwest_initial_client_shard_count(target_len);
        let mut clients = Vec::with_capacity(initial_len);
        for _ in 0..initial_len {
            clients.push(build_direct_reqwest_client_from_cache_key(&cache_key)?);
            DIRECT_REQWEST_CLIENT_CACHE_METRICS
                .builds
                .fetch_add(1, Ordering::Relaxed);
        }
        let entry =
            DirectReqwestClientCacheEntry::new(clients, target_len, target_len > initial_len);
        record_direct_reqwest_client_protocol_selection(&cache_key);
        let client = entry.select();
        let warm_key = (target_len > initial_len).then(|| cache_key.clone());
        cache.insert(cache_key, entry);
        if let Some(warm_key) = warm_key {
            warm_after_unlock = Some((warm_key, initial_len, target_len));
        }
        drop(cache);
        if let Some((cache_key, existing_len, target_len)) = warm_after_unlock {
            let spawned =
                spawn_direct_reqwest_client_cache_warm(cache_key.clone(), existing_len, target_len);
            if !spawned {
                mark_direct_reqwest_client_cache_not_warming(&cache_key);
            }
        }
        return Ok(client);
    }

    observe_gateway_stage_ms(
        "direct_reqwest_client_cache_lock",
        cache_lock_started_at.elapsed().as_millis() as u64,
    );
    DIRECT_REQWEST_CLIENT_CACHE_METRICS
        .misses
        .fetch_add(1, Ordering::Relaxed);
    record_direct_reqwest_client_protocol_selection(&cache_key);
    let client = build_direct_reqwest_client_from_cache_key(&cache_key)?;
    DIRECT_REQWEST_CLIENT_CACHE_METRICS
        .builds
        .fetch_add(1, Ordering::Relaxed);
    Ok(client)
}

fn spawn_direct_reqwest_client_cache_warm(
    cache_key: DirectReqwestClientCacheKey,
    existing_len: usize,
    target_len: usize,
) -> bool {
    if target_len <= existing_len {
        DIRECT_REQWEST_CLIENT_CACHE_METRICS
            .warm_skipped_total
            .fetch_add(1, Ordering::Relaxed);
        return false;
    }
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        DIRECT_REQWEST_CLIENT_CACHE_METRICS
            .warm_skipped_total
            .fetch_add(1, Ordering::Relaxed);
        return false;
    };
    DIRECT_REQWEST_CLIENT_CACHE_METRICS
        .warm_enqueues
        .fetch_add(1, Ordering::Relaxed);
    let enqueue_started_at = Instant::now();
    handle.spawn_blocking(move || {
        for _ in existing_len..target_len {
            match build_direct_reqwest_client_from_cache_key(&cache_key) {
                Ok(client) => {
                    DIRECT_REQWEST_CLIENT_CACHE_METRICS
                        .builds
                        .fetch_add(1, Ordering::Relaxed);
                    let Ok(mut cache) = DIRECT_REQWEST_CLIENT_CACHE.lock() else {
                        return;
                    };
                    let Some(entry) = cache.get_mut(&cache_key) else {
                        return;
                    };
                    if entry.clients.len() >= entry.target_len {
                        entry.warming = false;
                        return;
                    }
                    entry.clients.push(client);
                    if entry.clients.len() >= entry.target_len {
                        entry.warming = false;
                        return;
                    }
                }
                Err(err) => {
                    tracing::debug!(
                        error = ?err,
                        "gateway direct reqwest client cache warm failed"
                    );
                    mark_direct_reqwest_client_cache_not_warming(&cache_key);
                    break;
                }
            }
        }

        let Ok(mut cache) = DIRECT_REQWEST_CLIENT_CACHE.lock() else {
            return;
        };
        let Some(entry) = cache.get_mut(&cache_key) else {
            return;
        };
        entry.warming = false;
    });
    observe_gateway_stage_ms(
        "direct_reqwest_client_cache_warm_enqueue",
        enqueue_started_at.elapsed().as_millis() as u64,
    );
    true
}

fn mark_direct_reqwest_client_cache_warming(cache_key: &DirectReqwestClientCacheKey) {
    if let Ok(mut cache) = DIRECT_REQWEST_CLIENT_CACHE.lock() {
        if let Some(entry) = cache.get_mut(cache_key) {
            entry.warming = true;
        }
    }
}

fn mark_direct_reqwest_client_cache_not_warming(cache_key: &DirectReqwestClientCacheKey) {
    if let Ok(mut cache) = DIRECT_REQWEST_CLIENT_CACHE.lock() {
        if let Some(entry) = cache.get_mut(cache_key) {
            entry.warming = false;
        }
    }
}

fn direct_reqwest_client_cache_key(
    request_url: &str,
    key_id: &str,
    timeouts: Option<&aether_contracts::ExecutionTimeouts>,
    transport_profile: Option<&ResolvedTransportProfile>,
    transport_controls: ExecutionTransportControls,
) -> DirectReqwestClientCacheKey {
    DirectReqwestClientCacheKey {
        upstream_origin: direct_reqwest_cache_per_origin()
            .then(|| direct_reqwest_upstream_origin(request_url))
            .flatten(),
        pool_partition: direct_reqwest_pool_partition(transport_profile, key_id),
        connect_timeout_ms: timeouts.and_then(|timeouts| timeouts.connect_ms),
        follow_redirects: transport_controls.follow_redirects == Some(true),
        http1_only: transport_controls.http1_only,
        accept_invalid_certs: transport_controls.accept_invalid_certs,
        transport_profile: transport_profile.map(direct_reqwest_transport_profile_cache_key),
    }
}

fn direct_reqwest_pool_partition(
    transport_profile: Option<&ResolvedTransportProfile>,
    key_id: &str,
) -> Option<String> {
    let key_id = key_id.trim();
    transport_profile
        .filter(|profile| profile.pool_scope.trim().eq_ignore_ascii_case("key"))
        .filter(|_| !key_id.is_empty())
        .map(|_| format!("{:x}", sha2::Sha256::digest(key_id.as_bytes())))
}

fn direct_reqwest_cache_per_origin() -> bool {
    std::env::var(DIRECT_REQWEST_CACHE_PER_ORIGIN_ENV)
        .ok()
        .is_some_and(|value| matches_truthy_env_value(value.trim()))
}

fn matches_truthy_env_value(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn direct_reqwest_upstream_origin(request_url: &str) -> Option<String> {
    let url = reqwest::Url::parse(request_url).ok()?;
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = url.host_str()?;
    let port = url.port_or_known_default()?;
    Some(format!("{scheme}://{host}:{port}"))
}

fn direct_reqwest_transport_profile_cache_key(
    profile: &ResolvedTransportProfile,
) -> DirectReqwestTransportProfileCacheKey {
    DirectReqwestTransportProfileCacheKey {
        profile_id: profile.profile_id.trim().to_string(),
        backend: profile.backend.trim().to_ascii_lowercase(),
        http_mode: profile.http_mode.trim().to_ascii_lowercase(),
        pool_scope: profile.pool_scope.trim().to_ascii_lowercase(),
        header_fingerprint: stable_json_cache_key(profile.header_fingerprint.as_ref()),
        extra: stable_json_cache_key(profile.extra.as_ref()),
    }
}

fn stable_json_cache_key(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| serde_json::to_string(value).ok())
}

fn build_direct_reqwest_client_cache_entry_from_cache_key(
    cache_key: &DirectReqwestClientCacheKey,
) -> Result<DirectReqwestClientCacheEntry, ExecutionRuntimeTransportError> {
    let shard_count = direct_reqwest_client_shard_count(cache_key);
    let mut clients = Vec::with_capacity(shard_count);
    for _ in 0..shard_count {
        clients.push(build_direct_reqwest_client_from_cache_key(cache_key)?);
    }
    Ok(DirectReqwestClientCacheEntry::new(
        clients,
        shard_count,
        false,
    ))
}

fn direct_reqwest_client_shard_count(cache_key: &DirectReqwestClientCacheKey) -> usize {
    if let Some(shards) = env_positive_usize(DIRECT_REQWEST_CLIENT_SHARDS_ENV) {
        return shards.clamp(1, MAX_DIRECT_REQWEST_H2_CLIENT_SHARDS);
    }
    let target_gate_limit = crate::state::upstream_target_gate_limit_from_env()
        .unwrap_or_else(crate::state::upstream_target_gate_auto_limit);
    if !direct_reqwest_client_cache_key_uses_http2(cache_key) {
        return direct_reqwest_client_shards_from_config(
            None,
            target_gate_limit,
            env_positive_usize(DIRECT_REQWEST_HTTP1_TARGET_STREAMS_PER_CLIENT_ENV)
                .unwrap_or(DEFAULT_HTTP1_TARGET_STREAMS_PER_CLIENT),
        );
    }
    direct_reqwest_h2_client_shards_from_config(
        env_positive_usize(DIRECT_REQWEST_H2_CLIENT_SHARDS_ENV),
        target_gate_limit,
        env_positive_usize(DIRECT_REQWEST_H2_TARGET_STREAMS_PER_CLIENT_ENV)
            .unwrap_or(DEFAULT_H2_TARGET_STREAMS_PER_CLIENT),
    )
}

fn direct_reqwest_client_cache_key_uses_http2(cache_key: &DirectReqwestClientCacheKey) -> bool {
    if cache_key.http1_only {
        return false;
    }
    direct_reqwest_client_cache_key_uses_h2c_prior_knowledge(cache_key)
}

fn direct_reqwest_client_cache_key_uses_h2c_prior_knowledge(
    cache_key: &DirectReqwestClientCacheKey,
) -> bool {
    cache_key
        .transport_profile
        .as_ref()
        .is_some_and(|profile| profile.http_mode == TRANSPORT_HTTP_MODE_H2C_PRIOR_KNOWLEDGE)
}

fn record_direct_reqwest_client_protocol_selection(cache_key: &DirectReqwestClientCacheKey) {
    if cache_key.http1_only {
        DIRECT_REQWEST_CLIENT_CACHE_METRICS
            .http1_selections
            .fetch_add(1, Ordering::Relaxed);
        return;
    }
    if direct_reqwest_client_cache_key_uses_h2c_prior_knowledge(cache_key) {
        DIRECT_REQWEST_CLIENT_CACHE_METRICS
            .h2c_selections
            .fetch_add(1, Ordering::Relaxed);
        return;
    }
    DIRECT_REQWEST_CLIENT_CACHE_METRICS
        .auto_selections
        .fetch_add(1, Ordering::Relaxed);
}

fn direct_reqwest_h2_client_shards_from_config(
    explicit_shards: Option<usize>,
    target_gate_limit: usize,
    target_streams_per_client: usize,
) -> usize {
    direct_reqwest_client_shards_from_config(
        explicit_shards,
        target_gate_limit,
        target_streams_per_client,
    )
}

fn direct_reqwest_client_shards_from_config(
    explicit_shards: Option<usize>,
    target_gate_limit: usize,
    target_streams_per_client: usize,
) -> usize {
    if let Some(shards) = explicit_shards {
        return shards.clamp(1, MAX_DIRECT_REQWEST_H2_CLIENT_SHARDS);
    }
    let streams_per_client = target_streams_per_client.max(1);
    target_gate_limit
        .max(1)
        .div_ceil(streams_per_client)
        .clamp(1, MAX_DIRECT_REQWEST_H2_CLIENT_SHARDS)
}

fn direct_reqwest_initial_client_shard_count(target_len: usize) -> usize {
    env_positive_usize(DIRECT_REQWEST_SYNC_WARM_CLIENTS_ENV)
        .unwrap_or(DEFAULT_DIRECT_REQWEST_SYNC_WARM_CLIENTS)
        .clamp(1, target_len.clamp(1, MAX_DIRECT_REQWEST_SYNC_WARM_CLIENTS))
}

fn direct_reqwest_prewarm_client_shard_count(target_len: usize) -> usize {
    let request_path_cap = direct_reqwest_initial_client_shard_count(target_len);
    env_positive_usize(DIRECT_REQWEST_PREWARM_SYNC_CLIENTS_ENV)
        .unwrap_or(request_path_cap)
        .clamp(1, target_len.max(1).min(request_path_cap))
}

fn env_positive_usize(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
}

fn build_direct_reqwest_client_from_cache_key(
    cache_key: &DirectReqwestClientCacheKey,
) -> Result<reqwest::Client, ExecutionRuntimeTransportError> {
    let mut builder = reqwest::Client::builder();
    if !cache_key.follow_redirects {
        builder = builder.redirect(Policy::none());
    }
    if cache_key.http1_only
        || cache_key
            .transport_profile
            .as_ref()
            .is_some_and(|profile| profile.http_mode == TRANSPORT_HTTP_MODE_HTTP1_ONLY)
    {
        builder = builder.http1_only();
    } else if cache_key
        .transport_profile
        .as_ref()
        .is_some_and(|profile| profile.http_mode == TRANSPORT_HTTP_MODE_H2C_PRIOR_KNOWLEDGE)
    {
        builder = builder.http2_prior_knowledge();
    }
    let mut builder = apply_http_client_config(
        builder,
        &HttpClientConfig {
            connect_timeout_ms: cache_key.connect_timeout_ms,
            pool_max_idle_per_host: Some(direct_reqwest_pool_max_idle_per_host()),
            ..HttpClientConfig::default()
        },
    );
    builder = apply_transport_profile_cache_key(
        builder,
        cache_key.transport_profile.as_ref(),
        cache_key.http1_only,
    );
    if cache_key.accept_invalid_certs {
        builder = builder.danger_accept_invalid_certs(true);
    }
    builder
        .build()
        .map_err(ExecutionRuntimeTransportError::ClientBuild)
}

fn direct_reqwest_pool_max_idle_per_host() -> usize {
    const DEFAULT_MAX_IDLE_PER_HOST: usize = 1024;
    std::env::var("AETHER_GATEWAY_UPSTREAM_POOL_MAX_IDLE_PER_HOST")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_IDLE_PER_HOST)
}

pub(crate) fn direct_reqwest_client_cache_metric_samples() -> Vec<MetricSample> {
    let (entries, clients, target_clients, ready_entries, warming_entries, pending_clients) =
        DIRECT_REQWEST_CLIENT_CACHE
            .lock()
            .map(|cache| {
                let entries = cache.len() as u64;
                let clients = cache.values().map(|entry| entry.len() as u64).sum();
                let target_clients = cache.values().map(|entry| entry.target_len as u64).sum();
                let ready_entries = cache
                    .values()
                    .filter(|entry| entry.len() >= entry.target_len)
                    .count() as u64;
                let warming_entries = cache.values().filter(|entry| entry.warming).count() as u64;
                let pending_clients = cache
                    .values()
                    .map(|entry| entry.target_len.saturating_sub(entry.len()) as u64)
                    .sum();
                (
                    entries,
                    clients,
                    target_clients,
                    ready_entries,
                    warming_entries,
                    pending_clients,
                )
            })
            .unwrap_or((0, 0, 0, 0, 0, 0));
    let (h2c_entries, h2c_clients, h2c_target_clients) = DIRECT_H2C_CLIENT_CACHE
        .lock()
        .map(|cache| {
            let entries = cache.len() as u64;
            let clients = cache.values().map(|entry| entry.len() as u64).sum();
            let target_clients = cache.values().map(|entry| entry.target_len as u64).sum();
            (entries, clients, target_clients)
        })
        .unwrap_or((0, 0, 0));
    let (
        h2c_sender_entries,
        h2c_sender_ready_entries,
        h2c_senders,
        h2c_target_senders,
        h2c_pending_senders,
        h2c_sender_in_flight,
        h2c_sender_max_in_flight,
    ) = DIRECT_H2C_SENDER_CACHE
        .read()
        .map_or((0, 0, 0, 0, 0, 0, 0), |cache| {
            let entries = cache.len() as u64;
            let ready_entries = cache
                .values()
                .filter_map(|cell| cell.get())
                .filter(|entry| entry.len() >= entry.target_len)
                .count() as u64;
            let senders = cache
                .values()
                .filter_map(|cell| cell.get())
                .map(|entry| entry.len() as u64)
                .sum();
            let target_senders = cache
                .values()
                .filter_map(|cell| cell.get())
                .map(|entry| entry.target_len as u64)
                .sum();
            let pending_senders = cache
                .values()
                .map(|cell| {
                    cell.get()
                        .map(|entry| entry.target_len.saturating_sub(entry.len()) as u64)
                        .unwrap_or_else(|| direct_h2c_client_shard_count() as u64)
                })
                .sum();
            let in_flight = cache
                .values()
                .filter_map(|cell| cell.get())
                .map(|entry| entry.in_flight())
                .sum();
            let max_in_flight = cache
                .values()
                .filter_map(|cell| cell.get())
                .map(|entry| entry.max_in_flight())
                .max()
                .unwrap_or(0);
            (
                entries,
                ready_entries,
                senders,
                target_senders,
                pending_senders,
                in_flight,
                max_in_flight,
            )
        });
    let mut samples = vec![
        MetricSample::new(
            "direct_reqwest_client_cache_entries",
            "Number of cached direct reqwest clients.",
            MetricKind::Gauge,
            entries,
        ),
        MetricSample::new(
            "direct_reqwest_client_cache_clients",
            "Number of direct reqwest clients across all cache entries.",
            MetricKind::Gauge,
            clients,
        ),
        MetricSample::new(
            "direct_reqwest_client_cache_target_clients",
            "Target number of direct reqwest clients across all cache entries.",
            MetricKind::Gauge,
            target_clients,
        ),
        MetricSample::new(
            "direct_reqwest_client_cache_ready_entries",
            "Number of direct reqwest client cache entries at target shard count.",
            MetricKind::Gauge,
            ready_entries,
        ),
        MetricSample::new(
            "direct_reqwest_client_cache_warming_entries",
            "Number of direct reqwest client cache entries currently warming in the background.",
            MetricKind::Gauge,
            warming_entries,
        ),
        MetricSample::new(
            "direct_reqwest_client_cache_pending_clients",
            "Number of direct reqwest client shards still missing from target cache size.",
            MetricKind::Gauge,
            pending_clients,
        ),
        MetricSample::new(
            "direct_reqwest_client_cache_hits_total",
            "Number of direct reqwest client cache hits.",
            MetricKind::Counter,
            DIRECT_REQWEST_CLIENT_CACHE_METRICS
                .hits
                .load(Ordering::Relaxed),
        ),
        MetricSample::new(
            "direct_reqwest_client_cache_misses_total",
            "Number of direct reqwest client cache misses.",
            MetricKind::Counter,
            DIRECT_REQWEST_CLIENT_CACHE_METRICS
                .misses
                .load(Ordering::Relaxed),
        ),
        MetricSample::new(
            "direct_reqwest_client_cache_builds_total",
            "Number of direct reqwest clients built after cache misses.",
            MetricKind::Counter,
            DIRECT_REQWEST_CLIENT_CACHE_METRICS
                .builds
                .load(Ordering::Relaxed),
        ),
        MetricSample::new(
            "direct_reqwest_client_cache_warm_enqueue_total",
            "Number of background direct reqwest client cache warm jobs enqueued.",
            MetricKind::Counter,
            DIRECT_REQWEST_CLIENT_CACHE_METRICS
                .warm_enqueues
                .load(Ordering::Relaxed),
        ),
        MetricSample::new(
            "direct_reqwest_client_cache_warm_skipped_total",
            "Number of direct reqwest client cache warm attempts skipped before enqueue.",
            MetricKind::Counter,
            DIRECT_REQWEST_CLIENT_CACHE_METRICS
                .warm_skipped_total
                .load(Ordering::Relaxed),
        ),
        MetricSample::new(
            "direct_reqwest_client_http1_select_total",
            "Number of direct reqwest client selections using forced HTTP/1.",
            MetricKind::Counter,
            DIRECT_REQWEST_CLIENT_CACHE_METRICS
                .http1_selections
                .load(Ordering::Relaxed),
        ),
        MetricSample::new(
            "direct_reqwest_client_h2c_select_total",
            "Number of direct reqwest client selections using h2c prior knowledge.",
            MetricKind::Counter,
            DIRECT_REQWEST_CLIENT_CACHE_METRICS
                .h2c_selections
                .load(Ordering::Relaxed),
        ),
        MetricSample::new(
            "direct_reqwest_client_auto_select_total",
            "Number of direct reqwest client selections using reqwest automatic protocol negotiation.",
            MetricKind::Counter,
            DIRECT_REQWEST_CLIENT_CACHE_METRICS
                .auto_selections
                .load(Ordering::Relaxed),
        ),
    ];
    samples.extend([
        MetricSample::new(
            "direct_h2c_client_cache_entries",
            "Number of cached direct H2C client entries.",
            MetricKind::Gauge,
            h2c_entries,
        ),
        MetricSample::new(
            "direct_h2c_client_cache_clients",
            "Number of direct H2C clients across all cache entries.",
            MetricKind::Gauge,
            h2c_clients,
        ),
        MetricSample::new(
            "direct_h2c_client_cache_target_clients",
            "Target number of direct H2C clients across all cache entries.",
            MetricKind::Gauge,
            h2c_target_clients,
        ),
        MetricSample::new(
            "direct_h2c_client_cache_hits_total",
            "Number of direct H2C client cache hits.",
            MetricKind::Counter,
            DIRECT_H2C_CLIENT_CACHE_METRICS.hits.load(Ordering::Relaxed),
        ),
        MetricSample::new(
            "direct_h2c_client_cache_misses_total",
            "Number of direct H2C client cache misses.",
            MetricKind::Counter,
            DIRECT_H2C_CLIENT_CACHE_METRICS
                .misses
                .load(Ordering::Relaxed),
        ),
        MetricSample::new(
            "direct_h2c_client_cache_builds_total",
            "Number of direct H2C clients built after cache misses.",
            MetricKind::Counter,
            DIRECT_H2C_CLIENT_CACHE_METRICS
                .builds
                .load(Ordering::Relaxed),
        ),
        MetricSample::new(
            "direct_h2c_sender_cache_entries",
            "Number of cached direct H2C sender entries.",
            MetricKind::Gauge,
            h2c_sender_entries,
        ),
        MetricSample::new(
            "direct_h2c_sender_cache_senders",
            "Number of direct H2C senders across all cache entries.",
            MetricKind::Gauge,
            h2c_senders,
        ),
        MetricSample::new(
            "direct_h2c_sender_cache_ready_entries",
            "Number of direct H2C sender cache entries at target sender count.",
            MetricKind::Gauge,
            h2c_sender_ready_entries,
        ),
        MetricSample::new(
            "direct_h2c_sender_cache_target_senders",
            "Target number of direct H2C senders across all cache entries.",
            MetricKind::Gauge,
            h2c_target_senders,
        ),
        MetricSample::new(
            "direct_h2c_sender_cache_pending_senders",
            "Number of direct H2C sender connections still missing from target cache size.",
            MetricKind::Gauge,
            h2c_pending_senders,
        ),
        MetricSample::new(
            "direct_h2c_sender_cache_in_flight",
            "Current number of direct H2C requests waiting for upstream headers across sender slots.",
            MetricKind::Gauge,
            h2c_sender_in_flight,
        ),
        MetricSample::new(
            "direct_h2c_sender_cache_max_slot_in_flight",
            "Highest observed in-flight request count on a single direct H2C sender slot.",
            MetricKind::Gauge,
            h2c_sender_max_in_flight,
        ),
        MetricSample::new(
            "direct_h2c_sender_cache_hits_total",
            "Number of direct H2C sender cache hits.",
            MetricKind::Counter,
            DIRECT_H2C_SENDER_CACHE_METRICS.hits.load(Ordering::Relaxed),
        ),
        MetricSample::new(
            "direct_h2c_sender_cache_misses_total",
            "Number of direct H2C sender cache misses.",
            MetricKind::Counter,
            DIRECT_H2C_SENDER_CACHE_METRICS
                .misses
                .load(Ordering::Relaxed),
        ),
        MetricSample::new(
            "direct_h2c_sender_cache_builds_total",
            "Number of direct H2C senders built after cache misses.",
            MetricKind::Counter,
            DIRECT_H2C_SENDER_CACHE_METRICS
                .builds
                .load(Ordering::Relaxed),
        ),
        MetricSample::new(
            "direct_h2c_sender_cache_prewarm_requested_total",
            "Number of direct H2C sender prewarm URLs requested.",
            MetricKind::Counter,
            DIRECT_H2C_SENDER_CACHE_METRICS
                .prewarm_requested
                .load(Ordering::Relaxed),
        ),
        MetricSample::new(
            "direct_h2c_sender_cache_prewarm_success_total",
            "Number of direct H2C sender cache targets successfully prewarmed.",
            MetricKind::Counter,
            DIRECT_H2C_SENDER_CACHE_METRICS
                .prewarm_success
                .load(Ordering::Relaxed),
        ),
        MetricSample::new(
            "direct_h2c_sender_cache_prewarm_failed_total",
            "Number of direct H2C sender cache prewarm targets or URLs that failed.",
            MetricKind::Counter,
            DIRECT_H2C_SENDER_CACHE_METRICS
                .prewarm_failed
                .load(Ordering::Relaxed),
        ),
    ]);
    samples
}

pub(crate) fn build_browser_wreq_client(
    timeouts: Option<&aether_contracts::ExecutionTimeouts>,
    transport_profile: &ResolvedTransportProfile,
    transport_controls: ExecutionTransportControls,
    apply_total_timeout: bool,
) -> Result<wreq::Client, ExecutionRuntimeTransportError> {
    let emulation = browser_wreq_emulation_from_profile(transport_profile)?;
    let mut builder = wreq::Client::builder().emulation(emulation);
    if transport_controls.follow_redirects == Some(true) {
        builder = builder.redirect(wreq::redirect::Policy::limited(10));
    }
    if transport_controls.http1_only || transport_profile_http1_only(Some(transport_profile)) {
        builder = builder.http1_only();
    }
    if transport_controls.accept_invalid_certs {
        builder = builder.cert_verification(false).verify_hostname(false);
    }
    if let Some(connect_ms) = timeouts.and_then(|timeouts| timeouts.connect_ms) {
        builder = builder.connect_timeout(Duration::from_millis(connect_ms));
    }
    if apply_total_timeout {
        if let Some(total_ms) = timeouts.and_then(|timeouts| timeouts.total_ms) {
            builder = builder.timeout(Duration::from_millis(total_ms));
        }
    }
    if let Some(read_ms) = timeouts.and_then(|timeouts| timeouts.read_ms) {
        builder = builder.read_timeout(Duration::from_millis(read_ms));
    }
    builder
        .build()
        .map_err(ExecutionRuntimeTransportError::BrowserClientBuild)
}

fn browser_wreq_emulation_from_profile(
    profile: &ResolvedTransportProfile,
) -> Result<wreq_util::Emulation, ExecutionRuntimeTransportError> {
    match normalize_browser_profile_name(browser_transport_profile_name(profile)).as_str() {
        "chrome100" => Ok(wreq_util::Emulation::Chrome100),
        "chrome101" => Ok(wreq_util::Emulation::Chrome101),
        "chrome104" => Ok(wreq_util::Emulation::Chrome104),
        "chrome105" => Ok(wreq_util::Emulation::Chrome105),
        "chrome106" => Ok(wreq_util::Emulation::Chrome106),
        "chrome107" => Ok(wreq_util::Emulation::Chrome107),
        "chrome108" => Ok(wreq_util::Emulation::Chrome108),
        "chrome109" => Ok(wreq_util::Emulation::Chrome109),
        "chrome110" => Ok(wreq_util::Emulation::Chrome110),
        "chrome114" => Ok(wreq_util::Emulation::Chrome114),
        "chrome116" => Ok(wreq_util::Emulation::Chrome116),
        "chrome117" => Ok(wreq_util::Emulation::Chrome117),
        "chrome118" => Ok(wreq_util::Emulation::Chrome118),
        "chrome119" => Ok(wreq_util::Emulation::Chrome119),
        "chrome120" => Ok(wreq_util::Emulation::Chrome120),
        "chrome123" => Ok(wreq_util::Emulation::Chrome123),
        "chrome124" => Ok(wreq_util::Emulation::Chrome124),
        "chrome126" => Ok(wreq_util::Emulation::Chrome126),
        "chrome127" => Ok(wreq_util::Emulation::Chrome127),
        "chrome128" => Ok(wreq_util::Emulation::Chrome128),
        "chrome129" => Ok(wreq_util::Emulation::Chrome129),
        "chrome130" => Ok(wreq_util::Emulation::Chrome130),
        "chrome131" => Ok(wreq_util::Emulation::Chrome131),
        "chrome132" => Ok(wreq_util::Emulation::Chrome132),
        "chrome133" => Ok(wreq_util::Emulation::Chrome133),
        "chrome134" => Ok(wreq_util::Emulation::Chrome134),
        "chrome135" => Ok(wreq_util::Emulation::Chrome135),
        "chrome136" => Ok(wreq_util::Emulation::Chrome136),
        "chrome137" => Ok(wreq_util::Emulation::Chrome137),
        "chrome138" => Ok(wreq_util::Emulation::Chrome138),
        "chrome139" => Ok(wreq_util::Emulation::Chrome139),
        "chrome140" => Ok(wreq_util::Emulation::Chrome140),
        "chrome141" => Ok(wreq_util::Emulation::Chrome141),
        "chrome142" => Ok(wreq_util::Emulation::Chrome142),
        "chrome143" => Ok(wreq_util::Emulation::Chrome143),
        "chrome144" => Ok(wreq_util::Emulation::Chrome144),
        "chrome145" => Ok(wreq_util::Emulation::Chrome145),
        other => Err(ExecutionRuntimeTransportError::UnsupportedTransportProfile(
            format!("browser_wreq:{other}"),
        )),
    }
}

fn normalize_browser_profile_name(value: String) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace(['_', '-', ' '], "")
}

fn validate_reqwest_transport_profile(
    transport_profile: Option<&ResolvedTransportProfile>,
) -> Result<(), ExecutionRuntimeTransportError> {
    let Some(profile) = transport_profile else {
        return Ok(());
    };
    if profile
        .backend
        .trim()
        .eq_ignore_ascii_case(TRANSPORT_BACKEND_REQWEST_RUSTLS)
    {
        return Ok(());
    }
    Err(ExecutionRuntimeTransportError::UnsupportedTransportProfile(
        profile.backend.clone(),
    ))
}

fn transport_profile_uses_browser_wreq(
    transport_profile: Option<&ResolvedTransportProfile>,
) -> bool {
    transport_profile
        .map(|profile| {
            profile
                .backend
                .trim()
                .eq_ignore_ascii_case(TRANSPORT_BACKEND_BROWSER_WREQ)
        })
        .unwrap_or(false)
}

fn browser_transport_profile_name(profile: &ResolvedTransportProfile) -> String {
    profile
        .extra
        .as_ref()
        .and_then(|value| {
            value
                .get("browser_profile")
                .or_else(|| value.get("impersonate"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            profile
                .profile_id
                .trim()
                .is_empty()
                .then_some("chrome136".to_string())
                .or_else(|| Some(profile.profile_id.trim().to_string()))
        })
        .unwrap_or_else(|| "chrome136".to_string())
}

fn insert_browser_control_header(
    headers: &mut HeaderMap,
    name: &'static str,
    value: &str,
) -> Result<(), ExecutionRuntimeTransportError> {
    headers.insert(
        HeaderName::from_static(name),
        HeaderValue::from_str(value)
            .map_err(|_| ExecutionRuntimeTransportError::InvalidHeaderValue(name.to_string()))?,
    );
    Ok(())
}

fn transport_profile_http1_only(transport_profile: Option<&ResolvedTransportProfile>) -> bool {
    transport_profile
        .map(|profile| {
            profile
                .http_mode
                .trim()
                .eq_ignore_ascii_case(TRANSPORT_HTTP_MODE_HTTP1_ONLY)
        })
        .unwrap_or(false)
}

fn transport_profile_h2c_prior_knowledge(
    transport_profile: Option<&ResolvedTransportProfile>,
) -> bool {
    transport_profile
        .map(|profile| {
            profile
                .http_mode
                .trim()
                .eq_ignore_ascii_case(TRANSPORT_HTTP_MODE_H2C_PRIOR_KNOWLEDGE)
        })
        .unwrap_or(false)
}

fn apply_transport_profile(
    builder: reqwest::ClientBuilder,
    transport_profile: Option<&ResolvedTransportProfile>,
) -> reqwest::ClientBuilder {
    let Some(profile) = transport_profile else {
        return builder;
    };
    let profile_id = profile.profile_id.trim();
    if profile_id.is_empty() || transport_profile_h2c_prior_knowledge(Some(profile)) {
        return builder;
    }

    let _ = rustls::crypto::ring::default_provider().install_default();

    builder.use_preconfigured_tls(build_best_effort_transport_tls_config(
        transport_profile_http1_only(transport_profile),
    ))
}

fn apply_transport_profile_cache_key(
    builder: reqwest::ClientBuilder,
    transport_profile: Option<&DirectReqwestTransportProfileCacheKey>,
    http1_only: bool,
) -> reqwest::ClientBuilder {
    let Some(profile) = transport_profile else {
        return builder;
    };
    if profile.profile_id.is_empty() || profile.http_mode == TRANSPORT_HTTP_MODE_H2C_PRIOR_KNOWLEDGE
    {
        return builder;
    }

    let _ = rustls::crypto::ring::default_provider().install_default();

    builder.use_preconfigured_tls(build_best_effort_transport_tls_config(http1_only))
}

fn build_best_effort_transport_tls_config(http1_only: bool) -> rustls::ClientConfig {
    let root_store =
        rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut config = rustls::ClientConfig::builder_with_protocol_versions(&[
        &rustls::version::TLS13,
        &rustls::version::TLS12,
    ])
    .with_root_certificates(root_store)
    .with_no_client_auth();
    config.alpn_protocols = if http1_only {
        vec![b"http/1.1".to_vec()]
    } else {
        vec![b"h2".to_vec(), b"http/1.1".to_vec()]
    };
    config
}

pub(crate) fn build_request_headers(
    headers: &BTreeMap<String, String>,
    content_encoding: Option<&str>,
    allow_passthrough_content_encoding: bool,
) -> Result<HeaderMap, ExecutionRuntimeTransportError> {
    let mut out = HeaderMap::new();
    let normalized_content_encoding = normalize_content_encoding(content_encoding);
    if let Some(encoding) = normalized_content_encoding.as_deref() {
        if !matches!(encoding, "gzip" | "zstd") && !allow_passthrough_content_encoding {
            return Err(ExecutionRuntimeTransportError::UnsupportedContentEncoding(
                encoding.to_string(),
            ));
        }
    }
    for (key, value) in headers {
        let normalized_key = key.trim().to_ascii_lowercase();
        if crate::headers::should_skip_request_header(&normalized_key)
            || is_hop_by_hop_header(&normalized_key)
            || normalized_key == "content-encoding"
            || normalized_key == EXECUTION_REQUEST_FOLLOW_REDIRECTS_HEADER
            || normalized_key == EXECUTION_REQUEST_HTTP1_ONLY_HEADER
            || normalized_key == EXECUTION_REQUEST_ACCEPT_INVALID_CERTS_HEADER
            || normalized_key == EXECUTION_RESPONSE_BODY_MODE_HEADER
        {
            continue;
        }

        let header_name = HeaderName::from_bytes(key.as_bytes())
            .map_err(|_| ExecutionRuntimeTransportError::InvalidHeaderName(key.clone()))?;
        let header_value = HeaderValue::from_str(value)
            .map_err(|_| ExecutionRuntimeTransportError::InvalidHeaderValue(key.clone()))?;
        out.insert(header_name, header_value);
    }
    if let Some(encoding) = normalized_content_encoding {
        out.insert(
            reqwest::header::CONTENT_ENCODING,
            HeaderValue::from_str(&encoding).map_err(|_| {
                ExecutionRuntimeTransportError::InvalidHeaderValue("content-encoding".into())
            })?,
        );
    }
    Ok(out)
}

fn resolve_execution_transport_controls(
    headers: &BTreeMap<String, String>,
) -> ExecutionTransportControls {
    ExecutionTransportControls {
        follow_redirects: execution_transport_header_value(
            headers,
            EXECUTION_REQUEST_FOLLOW_REDIRECTS_HEADER,
        )
        .and_then(|value| parse_execution_transport_bool(value)),
        http1_only: execution_transport_header_value(headers, EXECUTION_REQUEST_HTTP1_ONLY_HEADER)
            .and_then(|value| parse_execution_transport_bool(value))
            .unwrap_or(false),
        accept_invalid_certs: execution_transport_header_value(
            headers,
            EXECUTION_REQUEST_ACCEPT_INVALID_CERTS_HEADER,
        )
        .and_then(|value| parse_execution_transport_bool(value))
        .unwrap_or(false),
    }
}

pub(crate) fn execution_response_body_mode(plan: &ExecutionPlan) -> ExecutionResponseBodyMode {
    if plan.stream
        || plan.body.body_bytes_b64.is_none()
        || !plan
            .client_api_format
            .trim()
            .eq_ignore_ascii_case(plan.provider_api_format.trim())
    {
        return ExecutionResponseBodyMode::StructuredJson;
    }

    ExecutionResponseBodyMode::from_header_value(execution_transport_header_value(
        &plan.headers,
        EXECUTION_RESPONSE_BODY_MODE_HEADER,
    ))
}

fn execution_transport_header_value<'a>(
    headers: &'a BTreeMap<String, String>,
    target: &str,
) -> Option<&'a str> {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(target))
        .map(|(_, value)| value.as_str())
}

fn parse_execution_transport_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn header_map_to_string_map(headers: &HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

fn is_hop_by_hop_header(name: &str) -> bool {
    matches!(
        name,
        "host"
            | "content-length"
            | "connection"
            | "upgrade"
            | "keep-alive"
            | "proxy-authorization"
            | "proxy-connection"
            | "te"
            | "trailer"
            | "transfer-encoding"
    )
}

pub(crate) fn collect_response_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    header_map_to_string_map(headers)
}

pub(crate) fn decode_response_body_bytes<'a>(
    headers: &BTreeMap<String, String>,
    body_bytes: &'a [u8],
) -> Result<Cow<'a, [u8]>, ExecutionRuntimeTransportError> {
    decode_response_body_bytes_with_limit(
        headers,
        body_bytes,
        crate::headers::max_internal_buffered_body_bytes(),
    )
}

fn decode_response_body_bytes_with_limit<'a>(
    headers: &BTreeMap<String, String>,
    body_bytes: &'a [u8],
    limit_bytes: usize,
) -> Result<Cow<'a, [u8]>, ExecutionRuntimeTransportError> {
    let encoding = headers
        .get("content-encoding")
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    match encoding.as_deref() {
        Some("gzip") => {
            let mut decoder = GzDecoder::new(body_bytes);
            read_upstream_response_decoder_with_limit("gzip", &mut decoder, limit_bytes)
                .map(Cow::Owned)
        }
        Some("deflate") => {
            let mut decoder = DeflateDecoder::new(body_bytes);
            read_upstream_response_decoder_with_limit("deflate", &mut decoder, limit_bytes)
                .map(Cow::Owned)
        }
        _ => Ok(Cow::Borrowed(body_bytes)),
    }
}

fn read_upstream_response_decoder_with_limit(
    encoding: &str,
    decoder: &mut impl Read,
    limit_bytes: usize,
) -> Result<Vec<u8>, ExecutionRuntimeTransportError> {
    let read_limit = u64::try_from(limit_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut limited = decoder.take(read_limit);
    let mut out = Vec::new();
    limited.read_to_end(&mut out).map_err(|error| {
        ExecutionRuntimeTransportError::UpstreamResponseDecode {
            encoding: encoding.to_string(),
            message: error.to_string(),
        }
    })?;
    if out.len() > limit_bytes {
        return Err(ExecutionRuntimeTransportError::UpstreamResponseTooLarge {
            phase: UpstreamResponseBodyPhase::Decoded,
            limit_bytes,
        });
    }
    Ok(out)
}

pub(crate) fn response_body_is_json(headers: &BTreeMap<String, String>, body_bytes: &[u8]) -> bool {
    let content_type = headers
        .get("content-type")
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    if content_type.contains("application/connect+json")
        || content_type.contains("application/connect+proto")
    {
        return false;
    }
    if content_type.contains("json") {
        return true;
    }

    serde_json::from_slice::<Value>(body_bytes).is_ok()
}

pub(crate) fn build_execution_response_body(
    headers: &BTreeMap<String, String>,
    body_bytes: &[u8],
    decoded_body_bytes: &[u8],
    stream: bool,
    response_body_mode: ExecutionResponseBodyMode,
) -> Result<Option<ResponseBody>, ExecutionRuntimeTransportError> {
    if body_bytes.is_empty() {
        return Ok(None);
    }

    if !stream && response_body_is_json(headers, decoded_body_bytes) {
        let body_json: Value = serde_json::from_slice(decoded_body_bytes)
            .map_err(ExecutionRuntimeTransportError::InvalidJson)?;
        return Ok(Some(ResponseBody {
            json_body: Some(body_json),
            body_bytes_b64: (response_body_mode == ExecutionResponseBodyMode::PreserveBytes)
                .then(|| base64::engine::general_purpose::STANDARD.encode(body_bytes)),
        }));
    }

    if stream {
        return Ok(Some(ResponseBody {
            json_body: None,
            body_bytes_b64: Some(base64::engine::general_purpose::STANDARD.encode(body_bytes)),
        }));
    }

    Ok(Some(ResponseBody {
        json_body: None,
        body_bytes_b64: Some(base64::engine::general_purpose::STANDARD.encode(body_bytes)),
    }))
}
