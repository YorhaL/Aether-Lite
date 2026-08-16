use aether_billing::enrich_usage_event_with_billing;
use aether_billing::BillingModelContextLookup;
use aether_data::repository::audit::RequestAuditReader;
use aether_data::repository::auth::{
    AuthApiKeyLookupKey, ResolvedAuthApiKeySnapshotReader, StoredAuthApiKeySnapshot,
};
use aether_data::DataLayerError;
use aether_data_contracts::repository::billing::StoredBillingModelContext;
use aether_data_contracts::repository::candidate_selection::StoredMinimalCandidateSelectionRow;
use aether_data_contracts::repository::candidates::DecisionTrace;
use aether_data_contracts::repository::provider_catalog::{
    StoredProviderCatalogEndpoint, StoredProviderCatalogKey, StoredProviderCatalogProvider,
};
use aether_data_contracts::repository::settlement::{StoredUsageSettlement, UsageSettlementInput};
use aether_data_contracts::repository::usage::{
    StoredRequestUsageAudit, UpsertUsageRecord, UsageWriteRepository,
};
use aether_data_contracts::repository::video_tasks::{StoredVideoTask, VideoTaskLookupKey};
use aether_runtime_state::RuntimeQueueStore;
use aether_usage_runtime::{
    UsageBillingEventEnricher, UsageBodyCapturePolicy, UsageEvent, UsageRecordWriter,
    UsageRequestRecordLevel, UsageRuntimeAccess, UsageSettlementWriter,
};
use aether_video_tasks_core::StoredVideoTaskReadSide;
use async_trait::async_trait;
use serde_json::Value;
use tracing::warn;

use super::GatewayDataState;
use crate::data::candidate_selection::MinimalCandidateSelectionRowSource;
use crate::provider_transport::ProviderTransportSnapshotSource;

const REQUEST_RECORD_LEVEL_KEY: &str = "request_record_level";
const LEGACY_REQUEST_LOG_LEVEL_KEY: &str = "request_log_level";

fn usage_request_record_level_from_value(value: Option<&Value>) -> UsageRequestRecordLevel {
    let Some(value) = value.and_then(Value::as_str).map(str::trim) else {
        return UsageRequestRecordLevel::Full;
    };

    if value.eq_ignore_ascii_case("basic")
        || value.eq_ignore_ascii_case("base")
        || value.eq_ignore_ascii_case("headers")
        || value.eq_ignore_ascii_case("minimal")
        || value.eq_ignore_ascii_case("none")
    {
        UsageRequestRecordLevel::Basic
    } else {
        UsageRequestRecordLevel::Full
    }
}

#[async_trait]
impl RequestAuditReader for GatewayDataState {
    async fn find_request_usage_audit_by_request_id(
        &self,
        request_id: &str,
    ) -> Result<Option<StoredRequestUsageAudit>, DataLayerError> {
        GatewayDataState::find_request_usage_by_request_id(self, request_id).await
    }

    async fn read_request_decision_trace(
        &self,
        request_id: &str,
        attempted_only: bool,
    ) -> Result<Option<DecisionTrace>, DataLayerError> {
        GatewayDataState::read_decision_trace(self, request_id, attempted_only).await
    }

    async fn read_resolved_auth_api_key_snapshot(
        &self,
        user_id: &str,
        api_key_id: &str,
        now_unix_secs: u64,
    ) -> Result<Option<aether_data::repository::auth::ResolvedAuthApiKeySnapshot>, DataLayerError>
    {
        GatewayDataState::read_auth_api_key_snapshot(self, user_id, api_key_id, now_unix_secs).await
    }
}

#[async_trait]
impl ResolvedAuthApiKeySnapshotReader for GatewayDataState {
    async fn find_stored_auth_api_key_snapshot(
        &self,
        key: AuthApiKeyLookupKey<'_>,
    ) -> Result<Option<StoredAuthApiKeySnapshot>, DataLayerError> {
        GatewayDataState::find_auth_api_key_snapshot(self, key).await
    }
}

#[async_trait]
impl StoredVideoTaskReadSide for GatewayDataState {
    async fn find_stored_video_task(
        &self,
        key: VideoTaskLookupKey<'_>,
    ) -> Result<Option<StoredVideoTask>, DataLayerError> {
        GatewayDataState::find_video_task(self, key).await
    }
}

#[async_trait]
impl ProviderTransportSnapshotSource for GatewayDataState {
    fn encryption_key(&self) -> Option<&str> {
        GatewayDataState::encryption_key(self)
    }

    async fn list_provider_catalog_providers_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogProvider>, DataLayerError> {
        GatewayDataState::list_provider_catalog_providers_by_ids(self, ids).await
    }

    async fn list_provider_catalog_endpoints_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogEndpoint>, DataLayerError> {
        GatewayDataState::list_provider_catalog_endpoints_by_ids(self, ids).await
    }

    async fn list_provider_catalog_keys_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogKey>, DataLayerError> {
        GatewayDataState::list_provider_catalog_keys_by_ids(self, ids).await
    }
}

#[async_trait]
impl MinimalCandidateSelectionRowSource for GatewayDataState {
    async fn read_minimal_candidate_selection_rows_for_api_format_and_global_model(
        &self,
        api_format: &str,
        global_model_name: &str,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        self.list_minimal_candidate_selection_rows(api_format, global_model_name)
            .await
    }

    async fn read_minimal_candidate_selection_rows_for_api_format_and_requested_model(
        &self,
        api_format: &str,
        requested_model_name: &str,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        self.list_minimal_candidate_selection_rows_for_requested_model(
            api_format,
            requested_model_name,
        )
        .await
    }

    async fn read_minimal_candidate_selection_rows_for_api_format_and_requested_model_page(
        &self,
        query: &aether_data_contracts::repository::candidate_selection::StoredRequestedModelCandidateRowsQuery,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        self.list_minimal_candidate_selection_rows_for_requested_model_page(query)
            .await
    }

    async fn read_minimal_candidate_selection_rows_for_api_format(
        &self,
        api_format: &str,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        self.list_minimal_candidate_selection_rows_for_api_format(api_format)
            .await
    }

    async fn read_minimal_candidate_selection_rows_for_api_format_page(
        &self,
        query: &aether_data_contracts::repository::candidate_selection::StoredApiFormatCandidateRowsQuery,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        self.list_minimal_candidate_selection_rows_for_api_format_page(query)
            .await
    }
}

#[async_trait]
impl BillingModelContextLookup for GatewayDataState {
    async fn find_billing_model_context_by_model_id(
        &self,
        provider_id: &str,
        provider_api_key_id: Option<&str>,
        model_id: &str,
    ) -> Result<Option<StoredBillingModelContext>, DataLayerError> {
        GatewayDataState::find_billing_model_context_by_model_id(
            self,
            provider_id,
            provider_api_key_id,
            model_id,
        )
        .await
    }

    async fn find_billing_model_context(
        &self,
        provider_id: &str,
        provider_api_key_id: Option<&str>,
        global_model_name: &str,
    ) -> Result<Option<StoredBillingModelContext>, DataLayerError> {
        GatewayDataState::find_billing_model_context(
            self,
            provider_id,
            provider_api_key_id,
            global_model_name,
        )
        .await
    }
}

#[async_trait]
impl UsageSettlementWriter for GatewayDataState {
    fn has_usage_settlement_writer(&self) -> bool {
        GatewayDataState::has_settlement_writer(self)
    }

    async fn settle_usage(
        &self,
        input: UsageSettlementInput,
    ) -> Result<Option<StoredUsageSettlement>, DataLayerError> {
        GatewayDataState::settle_usage(self, input).await
    }
}

#[async_trait]
impl UsageBillingEventEnricher for GatewayDataState {
    async fn enrich_usage_event(&self, event: &mut UsageEvent) -> Result<(), DataLayerError> {
        enrich_usage_event_with_billing(self, event).await
    }
}

#[async_trait]
impl UsageRuntimeAccess for GatewayDataState {
    fn has_usage_writer(&self) -> bool {
        GatewayDataState::has_usage_writer(self)
    }

    fn has_usage_worker_queue(&self) -> bool {
        GatewayDataState::has_usage_worker_queue(self)
    }

    fn usage_worker_queue(&self) -> Option<std::sync::Arc<dyn RuntimeQueueStore>> {
        GatewayDataState::usage_worker_queue(self)
    }

    fn supports_first_byte_usage_fast_path(&self) -> bool {
        self.usage_writer
            .as_ref()
            .is_some_and(|repository| repository.supports_first_byte_usage_fast_path())
    }

    fn usage_worker_should_defer_for_database_pressure(&self) -> bool {
        self.database_pool_summary()
            .as_ref()
            .is_some_and(GatewayDataState::database_pool_summary_under_usage_worker_pressure)
    }

    async fn body_capture_policy(&self) -> Result<UsageBodyCapturePolicy, DataLayerError> {
        let value = match GatewayDataState::find_system_config_value(self, REQUEST_RECORD_LEVEL_KEY)
            .await?
        {
            Some(value) => Some(value),
            None => {
                GatewayDataState::find_system_config_value(self, LEGACY_REQUEST_LOG_LEVEL_KEY)
                    .await?
            }
        };
        Ok(UsageBodyCapturePolicy {
            record_level: usage_request_record_level_from_value(value.as_ref()),
        })
    }
}

#[async_trait]
impl UsageRecordWriter for GatewayDataState {
    fn supports_first_byte_usage_batch(&self) -> bool {
        self.usage_writer
            .as_ref()
            .is_some_and(|repository| repository.supports_first_byte_usage_batch())
    }

    fn first_byte_usage_writer_identity(&self) -> Option<usize> {
        self.usage_writer
            .as_ref()
            .map(|repository| std::sync::Arc::as_ptr(repository) as *const () as usize)
    }

    fn supports_pending_usage_batch(&self) -> bool {
        self.usage_writer
            .as_ref()
            .is_some_and(|repository| repository.supports_pending_usage_batch())
    }

    fn pending_usage_writer_identity(&self) -> Option<usize> {
        self.usage_writer
            .as_ref()
            .map(|repository| std::sync::Arc::as_ptr(repository) as *const () as usize)
    }

    async fn upsert_usage_record(
        &self,
        record: UpsertUsageRecord,
    ) -> Result<Option<StoredRequestUsageAudit>, DataLayerError> {
        let stored = GatewayDataState::upsert_usage(self, record).await?;
        if let (Some(runtime_state), Some(usage)) =
            (self.daily_usage_runtime_state.as_ref(), stored.as_ref())
        {
            if let Err(err) =
                crate::daily_usage_limit::record_finalized_daily_usage(runtime_state, usage).await
            {
                warn!(
                    event_name = "daily_usage_limit_increment_failed",
                    log_type = "ops",
                    request_id = %usage.request_id,
                    user_id = usage.user_id.as_deref().unwrap_or("-"),
                    api_key_id = usage.api_key_id.as_deref().unwrap_or("-"),
                    error = ?err,
                    "daily usage limit increment failed; usage recording continues"
                );
            }
        }
        Ok(stored)
    }

    async fn upsert_first_byte_usage_record(
        &self,
        record: UpsertUsageRecord,
    ) -> Result<(), DataLayerError> {
        GatewayDataState::upsert_first_byte_usage(self, record).await
    }

    async fn upsert_first_byte_usage_records(
        &self,
        records: Vec<UpsertUsageRecord>,
    ) -> Result<(), DataLayerError> {
        GatewayDataState::upsert_first_byte_usage_many(self, records).await
    }

    async fn upsert_pending_usage_records(
        &self,
        records: Vec<UpsertUsageRecord>,
    ) -> Result<(), DataLayerError> {
        GatewayDataState::upsert_pending_usage_many(self, records).await
    }
}
