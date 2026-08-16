use crate::{AppState, GatewayError};
use aether_data_contracts::repository::{candidate_selection, candidates};

impl AppState {
    pub(crate) async fn list_minimal_candidate_selection_rows_for_api_format(
        &self,
        api_format: &str,
    ) -> Result<Vec<candidate_selection::StoredMinimalCandidateSelectionRow>, GatewayError> {
        self.data
            .list_minimal_candidate_selection_rows_for_api_format(api_format)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn list_minimal_candidate_selection_rows_for_api_format_and_global_model(
        &self,
        api_format: &str,
        global_model_name: &str,
    ) -> Result<Vec<candidate_selection::StoredMinimalCandidateSelectionRow>, GatewayError> {
        self.data
            .list_minimal_candidate_selection_rows(api_format, global_model_name)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn list_minimal_candidate_selection_rows_for_api_format_and_requested_model(
        &self,
        api_format: &str,
        requested_model_name: &str,
    ) -> Result<Vec<candidate_selection::StoredMinimalCandidateSelectionRow>, GatewayError> {
        self.data
            .list_minimal_candidate_selection_rows_for_requested_model(
                api_format,
                requested_model_name,
            )
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn list_minimal_candidate_selection_rows_for_api_format_and_requested_model_page(
        &self,
        query: &candidate_selection::StoredRequestedModelCandidateRowsQuery,
    ) -> Result<Vec<candidate_selection::StoredMinimalCandidateSelectionRow>, GatewayError> {
        self.data
            .list_minimal_candidate_selection_rows_for_requested_model_page(query)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn read_recent_request_candidates(
        &self,
        limit: usize,
    ) -> Result<Vec<candidates::StoredRequestCandidate>, GatewayError> {
        self.data
            .list_recent_request_candidates(limit)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn upsert_request_candidate(
        &self,
        candidate: candidates::UpsertRequestCandidateRecord,
    ) -> Result<Option<candidates::StoredRequestCandidate>, GatewayError> {
        if let Some(queue) = self.request_candidate_queue.as_ref() {
            let stored = stored_request_candidate_from_upsert(&candidate)?;
            queue
                .enqueue_or_fallback(candidate)
                .await
                .map_err(|err| GatewayError::Internal(err.to_string()))?;
            return Ok(Some(stored));
        }

        self.data
            .upsert_request_candidate(candidate)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    /// Persist a candidate status when the caller does not need the materialized row.
    ///
    /// Lifecycle updates are emitted on the hot path (in particular the first-byte
    /// `pending -> streaming` transition). Rebuilding `StoredRequestCandidate` here
    /// only to discard it adds validation and clones for every update, especially when
    /// the async queue is enabled.
    pub(crate) async fn enqueue_request_candidate_status(
        &self,
        candidate: candidates::UpsertRequestCandidateRecord,
    ) -> Result<Option<()>, GatewayError> {
        if let Some(queue) = self.request_candidate_queue.as_ref() {
            queue
                .enqueue_or_fallback(candidate)
                .await
                .map_err(|err| GatewayError::Internal(err.to_string()))?;
            return Ok(Some(()));
        }

        self.data
            .upsert_request_candidate(candidate)
            .await
            .map(|stored| stored.map(|_| ()))
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    /// Try the in-memory lifecycle lane without awaiting or touching the repository.
    /// The returned record must be persisted through `enqueue_request_candidate_status`
    /// when the queue is disabled or closed.
    pub(crate) fn try_enqueue_request_candidate_status(
        &self,
        candidate: candidates::UpsertRequestCandidateRecord,
    ) -> Result<(), candidates::UpsertRequestCandidateRecord> {
        let Some(queue) = self.request_candidate_queue.as_ref() else {
            return Err(candidate);
        };
        queue.try_enqueue_priority_status(candidate)
    }
}

fn stored_request_candidate_from_upsert(
    candidate: &candidates::UpsertRequestCandidateRecord,
) -> Result<candidates::StoredRequestCandidate, GatewayError> {
    candidate
        .validate()
        .map_err(|err| GatewayError::Internal(err.to_string()))?;
    candidates::StoredRequestCandidate::new(
        candidate.id.clone(),
        candidate.request_id.clone(),
        candidate.user_id.clone(),
        candidate.api_key_id.clone(),
        candidate.username.clone(),
        candidate.api_key_name.clone(),
        candidate.candidate_index.try_into().unwrap_or(i32::MAX),
        candidate.retry_index.try_into().unwrap_or(i32::MAX),
        candidate.provider_id.clone(),
        candidate.endpoint_id.clone(),
        candidate.key_id.clone(),
        candidate.status,
        candidate.skip_reason.clone(),
        candidate.is_cached.unwrap_or(false),
        candidate.status_code.map(i32::from),
        candidate.error_type.clone(),
        candidate.error_message.clone(),
        candidate
            .latency_ms
            .map(|value| i32::try_from(value).unwrap_or(i32::MAX)),
        candidate
            .concurrent_requests
            .map(|value| i32::try_from(value).unwrap_or(i32::MAX)),
        candidate.extra_data.clone(),
        candidate.required_capabilities.clone(),
        candidate
            .created_at_unix_ms
            .or(candidate.started_at_unix_ms)
            .or(candidate.finished_at_unix_ms)
            .unwrap_or_else(crate::clock::current_unix_ms)
            .try_into()
            .unwrap_or(i64::MAX),
        candidate
            .started_at_unix_ms
            .map(|value| value.try_into().unwrap_or(i64::MAX)),
        candidate
            .finished_at_unix_ms
            .map(|value| value.try_into().unwrap_or(i64::MAX)),
    )
    .map_err(|err| GatewayError::Internal(err.to_string()))
}
