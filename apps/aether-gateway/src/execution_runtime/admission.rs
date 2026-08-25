//! Shared gateway-wide admission for long-lived upstream executions.

use std::time::Duration;

use aether_runtime::{ConcurrencyGate, ConcurrencyPermit};
use tokio::time::timeout;

use crate::stage_metrics::observe_gateway_stage_ms;
use crate::{AppState, GatewayError};

const UPSTREAM_EXECUTION_GATE_NAME: &str = "gateway_upstream_execution";

trait UpstreamExecutionGateProvider {
    fn upstream_execution_gate(&self) -> Option<&ConcurrencyGate>;
    fn upstream_execution_gate_queue_budget(&self) -> Duration;
}

impl UpstreamExecutionGateProvider for AppState {
    fn upstream_execution_gate(&self) -> Option<&ConcurrencyGate> {
        self.upstream_execution_gate.as_deref()
    }

    fn upstream_execution_gate_queue_budget(&self) -> Duration {
        self.frontdoor_runtime_guards.internal_gate_queue_budget
    }
}

pub(crate) async fn acquire_upstream_execution_gate(
    state: &AppState,
    trace_id: &str,
) -> Result<Option<ConcurrencyPermit>, GatewayError> {
    let Some(gate) = state.upstream_execution_gate() else {
        return Ok(None);
    };
    let budget = state.upstream_execution_gate_queue_budget();
    let started_at = std::time::Instant::now();
    match timeout(budget, gate.acquire()).await {
        Ok(Ok(permit)) => {
            observe_gateway_stage_ms(
                "upstream_execution_gate_wait",
                started_at.elapsed().as_millis() as u64,
            );
            Ok(Some(permit))
        }
        Ok(Err(error)) => Err(GatewayError::Internal(error.to_string())),
        Err(_) => Err(GatewayError::AdmissionTimeout {
            trace_id: trace_id.to_string(),
            gate: UPSTREAM_EXECUTION_GATE_NAME,
            queue_budget_ms: budget.as_millis() as u64,
        }),
    }
}
