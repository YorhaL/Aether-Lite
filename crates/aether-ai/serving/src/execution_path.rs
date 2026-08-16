use async_trait::async_trait;

#[derive(Debug)]
pub enum AiServingExecutionOutcome<Response, Exhaustion> {
    Responded(Response),
    Deferred(Response),
    Exhausted(Exhaustion),
    NoPath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiPlanFallbackReason {
    RemoteDecisionMiss,
    SchedulerDecisionUnsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiSyncExecutionStep {
    LocalSameFormatProvider,
    RemoteDecision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiStreamExecutionStep {
    LocalSameFormatProvider,
    RemoteDecision,
}

pub const DEFAULT_STREAM_EXECUTION_STEPS: &[AiStreamExecutionStep] = &[
    AiStreamExecutionStep::LocalSameFormatProvider,
    AiStreamExecutionStep::RemoteDecision,
];

#[async_trait]
pub trait AiSyncExecutionPathPort: Send + Sync {
    type Response: Send;
    type Exhaustion: Send;
    type Error: Send;

    fn scheduler_decision_supported(&self) -> bool;

    async fn execute_sync_step(
        &self,
        step: AiSyncExecutionStep,
    ) -> Result<AiServingExecutionOutcome<Self::Response, Self::Exhaustion>, Self::Error>;

    async fn execute_sync_plan_fallback(
        &self,
        reason: AiPlanFallbackReason,
    ) -> Result<AiServingExecutionOutcome<Self::Response, Self::Exhaustion>, Self::Error>;
}

#[async_trait]
pub trait AiStreamExecutionPathPort: Send + Sync {
    type Response: Send;
    type Exhaustion: Send;
    type Error: Send;

    fn scheduler_decision_supported(&self) -> bool;

    fn stream_execution_steps(&self) -> &'static [AiStreamExecutionStep] {
        DEFAULT_STREAM_EXECUTION_STEPS
    }

    async fn execute_stream_step(
        &self,
        step: AiStreamExecutionStep,
    ) -> Result<AiServingExecutionOutcome<Self::Response, Self::Exhaustion>, Self::Error>;

    async fn execute_stream_plan_fallback(
        &self,
        reason: AiPlanFallbackReason,
    ) -> Result<AiServingExecutionOutcome<Self::Response, Self::Exhaustion>, Self::Error>;
}

pub async fn run_ai_sync_execution_path<Port>(
    port: &Port,
) -> Result<AiServingExecutionOutcome<Port::Response, Port::Exhaustion>, Port::Error>
where
    Port: AiSyncExecutionPathPort,
{
    let mut exhausted = None;
    let mut deferred = None;

    if port.scheduler_decision_supported() {
        for step in [
            AiSyncExecutionStep::LocalSameFormatProvider,
            AiSyncExecutionStep::RemoteDecision,
        ] {
            if let Some(response) =
                absorb_sync_step(port, step, &mut deferred, &mut exhausted).await?
            {
                return Ok(response);
            }
        }
    }

    finish_sync_path(port, deferred, exhausted).await
}

pub async fn run_ai_stream_execution_path<Port>(
    port: &Port,
) -> Result<AiServingExecutionOutcome<Port::Response, Port::Exhaustion>, Port::Error>
where
    Port: AiStreamExecutionPathPort,
{
    let mut exhausted = None;
    let mut deferred = None;

    if port.scheduler_decision_supported() {
        for step in port.stream_execution_steps() {
            if let Some(response) =
                absorb_stream_step(port, *step, &mut deferred, &mut exhausted).await?
            {
                return Ok(response);
            }
        }
    }

    finish_stream_path(port, deferred, exhausted).await
}

async fn finish_sync_path<Port>(
    port: &Port,
    deferred: Option<Port::Response>,
    exhausted: Option<Port::Exhaustion>,
) -> Result<AiServingExecutionOutcome<Port::Response, Port::Exhaustion>, Port::Error>
where
    Port: AiSyncExecutionPathPort,
{
    if let Some(response) = deferred {
        return Ok(AiServingExecutionOutcome::Deferred(response));
    }
    if let Some(outcome) = exhausted {
        return Ok(AiServingExecutionOutcome::Exhausted(outcome));
    }
    let reason = fallback_reason(port.scheduler_decision_supported());
    port.execute_sync_plan_fallback(reason).await
}

async fn finish_stream_path<Port>(
    port: &Port,
    deferred: Option<Port::Response>,
    exhausted: Option<Port::Exhaustion>,
) -> Result<AiServingExecutionOutcome<Port::Response, Port::Exhaustion>, Port::Error>
where
    Port: AiStreamExecutionPathPort,
{
    if let Some(response) = deferred {
        return Ok(AiServingExecutionOutcome::Deferred(response));
    }
    if let Some(outcome) = exhausted {
        return Ok(AiServingExecutionOutcome::Exhausted(outcome));
    }
    let reason = fallback_reason(port.scheduler_decision_supported());
    port.execute_stream_plan_fallback(reason).await
}

fn fallback_reason(scheduler_supported: bool) -> AiPlanFallbackReason {
    if scheduler_supported {
        AiPlanFallbackReason::RemoteDecisionMiss
    } else {
        AiPlanFallbackReason::SchedulerDecisionUnsupported
    }
}

async fn absorb_sync_step<Port>(
    port: &Port,
    step: AiSyncExecutionStep,
    deferred: &mut Option<Port::Response>,
    exhausted: &mut Option<Port::Exhaustion>,
) -> Result<Option<AiServingExecutionOutcome<Port::Response, Port::Exhaustion>>, Port::Error>
where
    Port: AiSyncExecutionPathPort,
{
    Ok(absorb_outcome(
        port.execute_sync_step(step).await?,
        deferred,
        exhausted,
    ))
}

async fn absorb_stream_step<Port>(
    port: &Port,
    step: AiStreamExecutionStep,
    deferred: &mut Option<Port::Response>,
    exhausted: &mut Option<Port::Exhaustion>,
) -> Result<Option<AiServingExecutionOutcome<Port::Response, Port::Exhaustion>>, Port::Error>
where
    Port: AiStreamExecutionPathPort,
{
    Ok(absorb_outcome(
        port.execute_stream_step(step).await?,
        deferred,
        exhausted,
    ))
}

fn absorb_outcome<Response, Exhaustion>(
    outcome: AiServingExecutionOutcome<Response, Exhaustion>,
    deferred: &mut Option<Response>,
    exhausted: &mut Option<Exhaustion>,
) -> Option<AiServingExecutionOutcome<Response, Exhaustion>> {
    match outcome {
        AiServingExecutionOutcome::Responded(response) => {
            Some(AiServingExecutionOutcome::Responded(response))
        }
        AiServingExecutionOutcome::Deferred(response) => {
            *deferred = Some(response);
            None
        }
        AiServingExecutionOutcome::Exhausted(outcome) => {
            *exhausted = Some(outcome);
            None
        }
        AiServingExecutionOutcome::NoPath => None,
    }
}
