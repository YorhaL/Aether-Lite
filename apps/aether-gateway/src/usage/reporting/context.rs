use serde_json::Value;
use tokio::time::{sleep, Duration};

use crate::request_candidate_runtime::resolve_locally_actionable_request_candidate_report_context;
use crate::AppState;

pub(crate) use aether_usage_runtime::report_context_is_locally_actionable;

const REQUEST_CANDIDATE_REPORT_CONTEXT_RETRY_ATTEMPTS: usize = 5;
const REQUEST_CANDIDATE_REPORT_CONTEXT_RETRY_DELAY_MS: u64 = 50;

pub(crate) async fn resolve_locally_actionable_report_context(
    state: &AppState,
    report_context: Option<&Value>,
) -> Option<Value> {
    let context = report_context?.clone();
    if report_context_is_locally_actionable(Some(&context)) {
        return Some(context);
    }

    if let Some(resolved) =
        resolve_locally_actionable_request_candidate_report_context_with_retry(state, &context)
            .await
    {
        return Some(resolved);
    }

    report_context_is_locally_actionable(Some(&context)).then_some(context)
}

async fn resolve_locally_actionable_request_candidate_report_context_with_retry(
    state: &AppState,
    context: &Value,
) -> Option<Value> {
    if context
        .get("request_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return None;
    }

    for attempt in 0..=REQUEST_CANDIDATE_REPORT_CONTEXT_RETRY_ATTEMPTS {
        if let Some(resolved) =
            resolve_locally_actionable_request_candidate_report_context(state, context).await
        {
            return Some(resolved);
        }

        if attempt < REQUEST_CANDIDATE_REPORT_CONTEXT_RETRY_ATTEMPTS {
            sleep(Duration::from_millis(
                REQUEST_CANDIDATE_REPORT_CONTEXT_RETRY_DELAY_MS,
            ))
            .await;
        }
    }

    None
}
