use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use aether_contracts::ExecutionPlan;
use aether_runtime::{
    ConcurrencyError, ConcurrencyGate, ConcurrencyPermit, MetricKind, MetricLabel, MetricSample,
};
use dashmap::DashMap;
use tokio::time::timeout;
use url::Url;

use crate::stage_metrics::observe_gateway_stage_ms;
use crate::GatewayError;

const GATE_NAME: &str = "gateway_upstream_target";
const DEFAULT_METRIC_TARGET_LIMIT: usize = 32;
const METRIC_TARGET_LIMIT_ENV: &str = "AETHER_GATEWAY_UPSTREAM_TARGET_GATE_METRIC_LIMIT";
const TARGET_QUEUE_BUDGET_MS_ENV: &str = "AETHER_GATEWAY_UPSTREAM_TARGET_GATE_QUEUE_BUDGET_MS";
const DEFAULT_TARGET_QUEUE_BUDGET_MS: u64 = 1;
const MAX_TARGET_QUEUE_BUDGET_MS: u64 = 5_000;

#[derive(Debug)]
pub(crate) struct UpstreamTargetAdmission {
    limit: Option<usize>,
    queue_budget: Duration,
    gates: DashMap<String, Arc<UpstreamTargetGate>>,
}

#[derive(Debug)]
pub(crate) struct UpstreamTargetAdmissionPermit {
    _permit: ConcurrencyPermit,
}

#[derive(Debug)]
struct UpstreamTargetGate {
    gate: ConcurrencyGate,
    raw_seen_total: AtomicU64,
    preselect_total: AtomicU64,
    selected_total: AtomicU64,
    saturated_total: AtomicU64,
}

impl UpstreamTargetGate {
    fn new(limit: usize) -> Self {
        Self {
            gate: ConcurrencyGate::new(GATE_NAME, limit),
            raw_seen_total: AtomicU64::new(0),
            preselect_total: AtomicU64::new(0),
            selected_total: AtomicU64::new(0),
            saturated_total: AtomicU64::new(0),
        }
    }

    fn raw_seen(&self) {
        self.raw_seen_total.fetch_add(1, Ordering::Relaxed);
    }

    fn preselected(&self) {
        self.preselect_total.fetch_add(1, Ordering::Relaxed);
    }

    fn selected(&self) {
        self.selected_total.fetch_add(1, Ordering::Relaxed);
    }

    fn saturated(&self) {
        self.saturated_total.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct UpstreamTargetAdmissionSnapshot {
    pub(crate) target: String,
    pub(crate) in_flight: usize,
    pub(crate) available_permits: usize,
    pub(crate) high_watermark: usize,
    pub(crate) rejected: u64,
    pub(crate) raw_seen_total: u64,
    pub(crate) preselect_total: u64,
    pub(crate) selected_total: u64,
    pub(crate) selection_pressure_total: u64,
    pub(crate) saturated_total: u64,
}

impl UpstreamTargetAdmission {
    pub(crate) fn new(limit: Option<usize>, queue_budget: Duration) -> Self {
        Self {
            limit,
            queue_budget: target_queue_budget(queue_budget),
            gates: DashMap::new(),
        }
    }

    pub(crate) async fn acquire(
        &self,
        plan: &ExecutionPlan,
        trace_id: &str,
    ) -> Result<Option<UpstreamTargetAdmissionPermit>, GatewayError> {
        let Some(limit) = self.limit else {
            return Ok(None);
        };
        let key = upstream_target_key(plan);
        let gate = self
            .gates
            .entry(key.clone())
            .or_insert_with(|| Arc::new(UpstreamTargetGate::new(limit)))
            .clone();
        gate.selected();
        let started_at = Instant::now();
        let permit = match timeout(self.queue_budget, gate.gate.acquire()).await {
            Ok(Ok(permit)) => permit,
            Ok(Err(err)) => return Err(GatewayError::Internal(err.to_string())),
            Err(_) => {
                gate.saturated();
                tracing::debug!(
                    event_name = "gateway_upstream_target_admission_timeout",
                    log_type = "ops",
                    trace_id,
                    target = key.as_str(),
                    limit,
                    queue_budget_ms = self.queue_budget.as_millis() as u64,
                    "gateway upstream target admission gate timed out"
                );
                return Err(GatewayError::AdmissionTimeout {
                    trace_id: trace_id.to_string(),
                    gate: GATE_NAME,
                    queue_budget_ms: self.queue_budget.as_millis() as u64,
                });
            }
        };
        observe_gateway_stage_ms(
            "stream_upstream_target_admission",
            started_at.elapsed().as_millis() as u64,
        );
        Ok(Some(UpstreamTargetAdmissionPermit { _permit: permit }))
    }

    pub(crate) fn try_acquire_for_plan(
        &self,
        plan: &ExecutionPlan,
    ) -> Result<Option<UpstreamTargetAdmissionPermit>, GatewayError> {
        let Some(limit) = self.limit else {
            return Ok(None);
        };
        let key = upstream_target_key(plan);
        let gate = self
            .gates
            .entry(key)
            .or_insert_with(|| Arc::new(UpstreamTargetGate::new(limit)))
            .clone();
        gate.selected();
        match gate.gate.try_acquire() {
            Ok(permit) => Ok(Some(UpstreamTargetAdmissionPermit { _permit: permit })),
            Err(ConcurrencyError::Saturated { .. }) => {
                gate.saturated();
                Ok(None)
            }
            Err(err) => Err(GatewayError::Internal(err.to_string())),
        }
    }

    pub(crate) fn snapshot_for_plan(
        &self,
        plan: &ExecutionPlan,
    ) -> Option<UpstreamTargetAdmissionSnapshot> {
        let key = upstream_target_key(plan);
        self.snapshot_for_target_key(&key)
    }

    pub(crate) fn snapshot_for_target_key(
        &self,
        target: &str,
    ) -> Option<UpstreamTargetAdmissionSnapshot> {
        let entry = self.gates.get(target)?;
        Some(snapshot_for_gate(target.to_string(), entry.value()))
    }

    pub(crate) fn record_preselect_for_target_key(&self, target: &str) {
        let Some(limit) = self.limit else {
            return;
        };
        let gate = self
            .gates
            .entry(target.to_string())
            .or_insert_with(|| Arc::new(UpstreamTargetGate::new(limit)));
        gate.preselected();
    }

    pub(crate) fn record_raw_seen_for_target_key(&self, target: &str) {
        let Some(limit) = self.limit else {
            return;
        };
        let gate = self
            .gates
            .entry(target.to_string())
            .or_insert_with(|| Arc::new(UpstreamTargetGate::new(limit)));
        gate.raw_seen();
    }

    pub(crate) fn limit(&self) -> Option<usize> {
        self.limit
    }

    pub(crate) fn metric_samples(&self) -> Vec<MetricSample> {
        let mut samples = vec![MetricSample::new(
            "upstream_target_gate_active_targets",
            "Number of upstream targets currently tracked by the gateway upstream target admission gates.",
            MetricKind::Gauge,
            self.gates.len() as u64,
        )];

        let Some(limit) = self.limit else {
            return samples;
        };

        samples.push(MetricSample::new(
            "upstream_target_gate_limit",
            "Configured per-upstream-target admission gate limit.",
            MetricKind::Gauge,
            limit as u64,
        ));

        let mut snapshots = self
            .gates
            .iter()
            .map(|entry| snapshot_for_gate(entry.key().clone(), entry.value()))
            .collect::<Vec<_>>();
        snapshots.sort_by(|left, right| {
            right
                .in_flight
                .cmp(&left.in_flight)
                .then_with(|| right.high_watermark.cmp(&left.high_watermark))
                .then_with(|| right.saturated_total.cmp(&left.saturated_total))
        });

        let metric_target_limit = upstream_target_metric_limit();
        for snapshot in snapshots.into_iter().take(metric_target_limit) {
            let labels = vec![MetricLabel::new("target", snapshot.target)];
            samples.push(
                MetricSample::new(
                    "upstream_target_gate_in_flight",
                    "Current number of in-flight operations for an upstream target admission gate.",
                    MetricKind::Gauge,
                    snapshot.in_flight as u64,
                )
                .with_labels(labels.clone()),
            );
            samples.push(
                MetricSample::new(
                    "upstream_target_gate_available_permits",
                    "Currently available permits for an upstream target admission gate.",
                    MetricKind::Gauge,
                    snapshot.available_permits as u64,
                )
                .with_labels(labels.clone()),
            );
            samples.push(
                MetricSample::new(
                    "upstream_target_gate_high_watermark",
                    "Highest observed in-flight count for an upstream target admission gate.",
                    MetricKind::Gauge,
                    snapshot.high_watermark as u64,
                )
                .with_labels(labels.clone()),
            );
            samples.push(
                MetricSample::new(
                    "upstream_target_gate_rejected_total",
                    "Number of operations rejected by an upstream target admission gate.",
                    MetricKind::Counter,
                    snapshot.rejected,
                )
                .with_labels(labels.clone()),
            );
            samples.push(
                MetricSample::new(
                    "upstream_target_selected_total",
                    "Number of selections for an upstream target.",
                    MetricKind::Counter,
                    snapshot.selected_total,
                )
                .with_labels(labels.clone()),
            );
            samples.push(
                MetricSample::new(
                    "upstream_target_raw_seen_total",
                    "Number of lightweight target-selection windows where an upstream target appeared.",
                    MetricKind::Counter,
                    snapshot.raw_seen_total,
                )
                .with_labels(labels.clone()),
            );
            samples.push(
                MetricSample::new(
                    "upstream_target_preselect_total",
                    "Number of lightweight pre-first-byte selections for an upstream target.",
                    MetricKind::Counter,
                    snapshot.preselect_total,
                )
                .with_labels(labels.clone()),
            );
            samples.push(
                MetricSample::new(
                    "upstream_target_in_flight",
                    "Current number of pre-first-byte in-flight operations for an upstream target.",
                    MetricKind::Gauge,
                    snapshot.in_flight as u64,
                )
                .with_labels(labels.clone()),
            );
            samples.push(
                MetricSample::new(
                    "upstream_target_max_in_flight",
                    "Highest observed pre-first-byte in-flight count for an upstream target.",
                    MetricKind::Gauge,
                    snapshot.high_watermark as u64,
                )
                .with_labels(labels.clone()),
            );
            samples.push(
                MetricSample::new(
                    "upstream_target_saturated_total",
                    "Number of saturated selections for an upstream target.",
                    MetricKind::Counter,
                    snapshot.saturated_total,
                )
                .with_labels(labels),
            );
        }

        samples
    }
}

fn snapshot_for_gate(target: String, gate: &UpstreamTargetGate) -> UpstreamTargetAdmissionSnapshot {
    let snapshot = gate.gate.snapshot();
    let raw_seen_total = gate.raw_seen_total.load(Ordering::Relaxed);
    let preselect_total = gate.preselect_total.load(Ordering::Relaxed);
    let selected_total = gate.selected_total.load(Ordering::Relaxed);
    UpstreamTargetAdmissionSnapshot {
        target,
        in_flight: snapshot.in_flight,
        available_permits: snapshot.available_permits,
        high_watermark: snapshot.high_watermark,
        rejected: snapshot.rejected,
        raw_seen_total,
        preselect_total,
        selected_total,
        selection_pressure_total: preselect_total.saturating_add(selected_total),
        saturated_total: gate.saturated_total.load(Ordering::Relaxed),
    }
}

pub(crate) fn upstream_target_key(plan: &ExecutionPlan) -> String {
    upstream_target_key_from_url(plan.url.as_str()).unwrap_or_else(|| fallback_target_key(plan))
}

pub(crate) fn upstream_target_key_from_url(upstream_url: &str) -> Option<String> {
    let parsed = Url::parse(upstream_url).ok();
    let Some(url) = parsed else {
        return None;
    };
    let scheme = url.scheme().to_ascii_lowercase();
    let Some(host) = url.host_str().map(|host| host.to_ascii_lowercase()) else {
        return None;
    };
    let port = url
        .port_or_known_default()
        .map(|port| port.to_string())
        .unwrap_or_else(|| "-".to_string());
    Some(format!("{scheme}://{host}:{port}"))
}

fn fallback_target_key(plan: &ExecutionPlan) -> String {
    format!(
        "unparsed|provider={}|endpoint={}|url={}",
        plan.provider_id, plan.endpoint_id, plan.url
    )
}

fn upstream_target_metric_limit() -> usize {
    std::env::var(METRIC_TARGET_LIMIT_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_METRIC_TARGET_LIMIT)
}

fn target_queue_budget(fallback: Duration) -> Duration {
    std::env::var(TARGET_QUEUE_BUDGET_MS_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|value| value.clamp(1, MAX_TARGET_QUEUE_BUDGET_MS))
        .map(Duration::from_millis)
        .unwrap_or_else(|| {
            let fallback_ms = u64::try_from(fallback.as_millis()).unwrap_or(u64::MAX);
            Duration::from_millis(fallback_ms.clamp(1, DEFAULT_TARGET_QUEUE_BUDGET_MS))
        })
}
