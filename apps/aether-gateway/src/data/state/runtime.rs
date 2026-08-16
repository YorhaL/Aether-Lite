use super::{
    read_decision_trace, read_provider_transport_snapshot, read_request_candidate_trace,
    AdjustWalletBalanceInput, AdminWalletListQuery, AnnouncementListQuery, AuditLogListQuery,
    BackgroundTaskListQuery, BackgroundTaskSummary, BillingModelContextCacheKey,
    BillingModelContextCacheState, BillingModelContextInflightState, CreateAnnouncementRecord,
    DataLayerError, DatabaseMaintenanceSummary, DecisionTrace, GatewayDataState,
    GatewayProviderTransportSnapshot, LocalVideoTaskReadResponse, RequestAuditBundle,
    RequestCandidateTrace, StoredAdminAuditLogPage, StoredAdminWalletListPage, StoredAnnouncement,
    StoredAnnouncementPage, StoredBackgroundTaskEvent, StoredBackgroundTaskRun,
    StoredBackgroundTaskRunPage, StoredBillingModelContext, StoredProviderUsageSummary,
    StoredRequestUsageAudit, StoredSuspiciousActivity, StoredUsageSettlement,
    StoredUserAuditLogPage, StoredUserAuthRecord, StoredUserExportRow, StoredUserSummary,
    StoredVideoTask, StoredWalletSnapshot, UpdateAnnouncementRecord, UpsertBackgroundTaskEvent,
    UpsertBackgroundTaskRun, UpsertUsageRecord, UpsertVideoTask, UsageSettlementInput,
    VideoTaskLookupKey, VideoTaskModelCount, VideoTaskQueryFilter, VideoTaskStatusCount,
    WalletLookupKey,
};
use aether_data_contracts::repository::usage::{
    PendingUsageCleanupSummary, ProviderApiKeyWindowUsageRequest,
    StoredProviderApiKeyWindowUsageSummary, StoredUsageDailyActualCostRollup,
    StoredUsageDailySummary, UsageAuditListQuery, UsageCleanupExecutionMode, UsageCleanupSummary,
    UsageCleanupTargets, UsageCleanupWindow, UsageCounterFlushSummary, UsageCounterHealthSnapshot,
    UsageCounterPendingHealthSnapshot, UsageDailyActualCostRollupQuery, UsageDailyHeatmapQuery,
};
use aether_runtime_state::RuntimeQueueStore;
use aether_video_tasks_core::read_data_backed_video_task_response;
use std::time::{Duration, Instant};
use tokio::time::timeout;

fn normalize_billing_context_cache_part(value: &str) -> String {
    value.trim().to_string()
}

fn normalize_optional_billing_context_cache_part(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

enum BillingModelContextInflightRegistration<'a> {
    Leader(BillingModelContextInflightGuard<'a>),
    Follower(std::sync::Arc<BillingModelContextInflightState>),
    Saturated,
}

struct BillingModelContextInflightGuard<'a> {
    state: &'a GatewayDataState,
    key: Option<BillingModelContextCacheKey>,
    inflight_state: std::sync::Arc<BillingModelContextInflightState>,
    admission: Option<tokio::sync::OwnedSemaphorePermit>,
}

impl<'a> BillingModelContextInflightGuard<'a> {
    fn new(
        state: &'a GatewayDataState,
        key: BillingModelContextCacheKey,
        inflight_state: std::sync::Arc<BillingModelContextInflightState>,
        admission: tokio::sync::OwnedSemaphorePermit,
    ) -> Self {
        Self {
            state,
            key: Some(key),
            inflight_state,
            admission: Some(admission),
        }
    }

    fn epoch(&self) -> u64 {
        self.inflight_state.epoch
    }

    fn finish(&mut self, error: Option<DataLayerError>) {
        let removed = self.key.take().and_then(|key| {
            self.state.finish_billing_model_context_inflight(
                &key,
                &self.inflight_state,
                self.admission.take(),
            )
        });
        self.admission.take();
        if let Some(removed) = removed {
            removed.complete(error.map_or(Ok(()), Err));
        }
    }
}

impl Drop for BillingModelContextInflightGuard<'_> {
    fn drop(&mut self) {
        self.finish(None);
    }
}

impl BillingModelContextInflightState {
    fn complete(&self, result: Result<(), DataLayerError>) {
        if self.completion.set(result).is_ok() {
            self.notify.notify_waiters();
        }
    }

    async fn wait(&self) -> Result<(), DataLayerError> {
        loop {
            if let Some(result) = self.completion.get() {
                return result.clone();
            }

            let mut notified = Box::pin(self.notify.notified());
            notified.as_mut().enable();
            if let Some(result) = self.completion.get() {
                return result.clone();
            }
            notified.await;
        }
    }
}

impl Default for BillingModelContextCacheState {
    fn default() -> Self {
        Self {
            entries: aether_cache::ExpiringMap::default(),
            inflight: std::sync::Mutex::new(std::collections::HashMap::new()),
            epoch: std::sync::atomic::AtomicU64::new(0),
            mutation: std::sync::Mutex::new(()),
            admission: std::sync::Arc::new(tokio::sync::Semaphore::new(
                GatewayDataState::BILLING_MODEL_CONTEXT_CACHE_MAX_INFLIGHT,
            )),
        }
    }
}

impl GatewayDataState {
    const MAINTENANCE_POOL_IDLE_RESERVE_ENV: &'static str =
        "AETHER_GATEWAY_MAINTENANCE_POOL_IDLE_RESERVE";
    const MAINTENANCE_POOL_PRESSURE_MAX_DEFER: Duration = Duration::from_secs(30);
    const BILLING_MODEL_CONTEXT_CACHE_TTL: Duration = Duration::from_secs(30);
    const BILLING_MODEL_CONTEXT_CACHE_MAX_ENTRIES: usize = 4096;
    const BILLING_MODEL_CONTEXT_CACHE_MAX_INFLIGHT: usize = 4096;
    #[cfg(not(test))]
    const BILLING_MODEL_CONTEXT_CACHE_INFLIGHT_WAIT_TIMEOUT: Duration = Duration::from_secs(10);
    #[cfg(test)]
    const BILLING_MODEL_CONTEXT_CACHE_INFLIGHT_WAIT_TIMEOUT: Duration = Duration::from_millis(100);

    pub(crate) async fn run_database_maintenance(
        &self,
        table_names: &[&str],
    ) -> Result<DatabaseMaintenanceSummary, DataLayerError> {
        match &self.backends {
            Some(backends) => backends.run_database_maintenance(table_names).await,
            None => Ok(DatabaseMaintenanceSummary::default()),
        }
    }

    pub(crate) async fn run_database_migrations(
        &self,
    ) -> Result<bool, sqlx::migrate::MigrateError> {
        match &self.backends {
            Some(backends) => backends.run_database_migrations().await,
            None => Ok(false),
        }
    }

    pub(crate) async fn run_database_backfills(&self) -> Result<bool, sqlx::migrate::MigrateError> {
        match &self.backends {
            Some(backends) => backends.run_database_backfills().await,
            None => Ok(false),
        }
    }

    pub(crate) async fn pending_database_migrations(
        &self,
    ) -> Result<
        Option<Vec<aether_data::lifecycle::migrate::PendingMigrationInfo>>,
        sqlx::migrate::MigrateError,
    > {
        match &self.backends {
            Some(backends) => backends.pending_database_migrations().await,
            None => Ok(None),
        }
    }

    pub(crate) async fn prepare_database_for_startup(
        &self,
    ) -> Result<
        Option<Vec<aether_data::lifecycle::migrate::PendingMigrationInfo>>,
        sqlx::migrate::MigrateError,
    > {
        match &self.backends {
            Some(backends) => backends.prepare_database_for_startup().await,
            None => Ok(None),
        }
    }

    pub(crate) async fn warm_database_pool(&self) -> Result<(), DataLayerError> {
        match &self.backends {
            Some(backends) => backends.warm_database_pool().await,
            None => Ok(()),
        }
    }

    pub(crate) async fn pending_database_backfills(
        &self,
    ) -> Result<
        Option<Vec<aether_data::lifecycle::backfill::PendingBackfillInfo>>,
        sqlx::migrate::MigrateError,
    > {
        match &self.backends {
            Some(backends) => backends.pending_database_backfills().await,
            None => Ok(None),
        }
    }

    pub(crate) fn database_pool_summary(&self) -> Option<aether_data::DatabasePoolSummary> {
        self.backends
            .as_ref()
            .and_then(|backends| backends.database_pool_summary())
    }

    pub(crate) async fn postgres_observability_snapshot(
        &self,
    ) -> Result<Option<aether_data::DatabasePostgresObservabilitySnapshot>, DataLayerError> {
        match &self.backends {
            Some(backends) => backends.postgres_observability_snapshot().await,
            None => Ok(None),
        }
    }

    pub(crate) async fn postgres_activity_groups(
        &self,
        limit: i64,
    ) -> Result<Vec<aether_data::DatabasePostgresActivityGroup>, DataLayerError> {
        match &self.backends {
            Some(backends) => backends.postgres_activity_groups(limit).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) fn database_pool_under_maintenance_pressure(&self) -> bool {
        self.database_pool_summary()
            .as_ref()
            .is_some_and(Self::database_pool_summary_under_maintenance_pressure)
    }

    pub(crate) fn database_pool_summary_under_maintenance_pressure(
        summary: &aether_data::DatabasePoolSummary,
    ) -> bool {
        summary.checked_out > 0
            && Self::database_pool_available_capacity(summary)
                <= Self::maintenance_pool_idle_reserve(summary)
    }

    pub(crate) fn database_pool_summary_under_usage_worker_pressure(
        summary: &aether_data::DatabasePoolSummary,
    ) -> bool {
        summary.checked_out > 0
            && Self::database_pool_available_capacity(summary)
                <= Self::usage_worker_pool_idle_reserve(summary)
    }

    fn database_pool_available_capacity(summary: &aether_data::DatabasePoolSummary) -> usize {
        let unopened = (summary.max_connections as usize).saturating_sub(summary.pool_size);
        summary.idle.saturating_add(unopened)
    }

    pub(crate) fn maintenance_pool_idle_reserve(
        summary: &aether_data::DatabasePoolSummary,
    ) -> usize {
        if let Some(override_value) = std::env::var(Self::MAINTENANCE_POOL_IDLE_RESERVE_ENV)
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
        {
            return override_value;
        }

        let max_connections = summary.max_connections as usize;
        if max_connections == 0 {
            return 0;
        }

        let ten_percent_ceil = (max_connections + 9) / 10;
        ten_percent_ceil.clamp(2, 10).min(max_connections)
    }

    fn usage_worker_pool_idle_reserve(summary: &aether_data::DatabasePoolSummary) -> usize {
        if summary.max_connections <= 1 {
            return 0;
        }
        1
    }

    pub(crate) fn should_defer_maintenance_for_database_pool_pressure(
        &self,
        deferred_since: &mut Option<Instant>,
    ) -> bool {
        Self::should_defer_maintenance_for_pool_pressure_state(
            self.database_pool_under_maintenance_pressure(),
            deferred_since,
        )
    }

    pub(crate) fn should_defer_maintenance_for_pool_pressure_state(
        pool_under_pressure: bool,
        deferred_since: &mut Option<Instant>,
    ) -> bool {
        if !pool_under_pressure {
            *deferred_since = None;
            return false;
        }

        let now = Instant::now();
        let since = deferred_since.get_or_insert(now);
        if now.duration_since(*since) >= Self::MAINTENANCE_POOL_PRESSURE_MAX_DEFER {
            *deferred_since = None;
            return false;
        }

        true
    }

    pub(crate) async fn aggregate_stats_hourly(
        &self,
        input: &aether_data::StatsHourlyAggregationInput,
    ) -> Result<Option<aether_data::StatsHourlyAggregationSummary>, DataLayerError> {
        match &self.backends {
            Some(backends) => backends.aggregate_stats_hourly(input).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn aggregate_stats_daily(
        &self,
        input: &aether_data::StatsDailyAggregationInput,
    ) -> Result<Option<aether_data::StatsDailyAggregationSummary>, DataLayerError> {
        match &self.backends {
            Some(backends) => backends.aggregate_stats_daily(input).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn list_announcements(
        &self,
        query: &AnnouncementListQuery,
    ) -> Result<StoredAnnouncementPage, DataLayerError> {
        match &self.announcement_reader {
            Some(repository) => repository.list_announcements(query).await,
            None => Ok(StoredAnnouncementPage::default()),
        }
    }

    pub(crate) async fn find_announcement_by_id(
        &self,
        announcement_id: &str,
    ) -> Result<Option<StoredAnnouncement>, DataLayerError> {
        match &self.announcement_reader {
            Some(repository) => repository.find_by_id(announcement_id).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn list_admin_audit_logs(
        &self,
        query: &AuditLogListQuery,
    ) -> Result<StoredAdminAuditLogPage, DataLayerError> {
        let Some(repository) = self
            .backends
            .as_ref()
            .and_then(|backends| backends.read().audit_logs())
        else {
            return Ok(StoredAdminAuditLogPage {
                items: Vec::new(),
                total: 0,
            });
        };
        repository.list_admin_audit_logs(query).await
    }

    pub(crate) async fn list_admin_suspicious_activities(
        &self,
        cutoff_unix_secs: u64,
    ) -> Result<Vec<StoredSuspiciousActivity>, DataLayerError> {
        let Some(repository) = self
            .backends
            .as_ref()
            .and_then(|backends| backends.read().audit_logs())
        else {
            return Ok(Vec::new());
        };
        repository
            .list_admin_suspicious_activities(cutoff_unix_secs)
            .await
    }

    pub(crate) async fn read_admin_user_behavior_event_counts(
        &self,
        user_id: &str,
        cutoff_unix_secs: u64,
    ) -> Result<std::collections::BTreeMap<String, u64>, DataLayerError> {
        let Some(repository) = self
            .backends
            .as_ref()
            .and_then(|backends| backends.read().audit_logs())
        else {
            return Ok(std::collections::BTreeMap::new());
        };
        repository
            .read_admin_user_behavior_event_counts(user_id, cutoff_unix_secs)
            .await
    }

    pub(crate) async fn list_user_audit_logs(
        &self,
        user_id: &str,
        query: &AuditLogListQuery,
    ) -> Result<StoredUserAuditLogPage, DataLayerError> {
        let Some(repository) = self
            .backends
            .as_ref()
            .and_then(|backends| backends.read().audit_logs())
        else {
            return Ok(StoredUserAuditLogPage {
                items: Vec::new(),
                total: 0,
            });
        };
        repository.list_user_audit_logs(user_id, query).await
    }

    pub(crate) async fn delete_audit_logs_before(
        &self,
        cutoff_unix_secs: u64,
        limit: usize,
    ) -> Result<usize, DataLayerError> {
        let Some(repository) = self
            .backends
            .as_ref()
            .and_then(|backends| backends.read().audit_logs())
        else {
            return Ok(0);
        };
        repository
            .delete_audit_logs_before(cutoff_unix_secs, limit)
            .await
    }

    pub(crate) async fn count_unread_active_announcements(
        &self,
        user_id: &str,
        now_unix_secs: u64,
    ) -> Result<u64, DataLayerError> {
        match &self.announcement_reader {
            Some(repository) => {
                repository
                    .count_unread_active_announcements(user_id, now_unix_secs)
                    .await
            }
            None => Ok(0),
        }
    }

    pub(crate) async fn list_required_unread_active_announcements(
        &self,
        user_id: &str,
        now_unix_secs: u64,
        limit: usize,
    ) -> Result<Vec<StoredAnnouncement>, DataLayerError> {
        match &self.announcement_reader {
            Some(repository) => {
                repository
                    .list_required_unread_active_announcements(user_id, now_unix_secs, limit)
                    .await
            }
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn create_announcement(
        &self,
        record: CreateAnnouncementRecord,
    ) -> Result<Option<StoredAnnouncement>, DataLayerError> {
        match &self.announcement_writer {
            Some(repository) => repository.create_announcement(record).await.map(Some),
            None => Ok(None),
        }
    }

    pub(crate) async fn update_announcement(
        &self,
        record: UpdateAnnouncementRecord,
    ) -> Result<Option<StoredAnnouncement>, DataLayerError> {
        match &self.announcement_writer {
            Some(repository) => repository.update_announcement(record).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn delete_announcement(
        &self,
        announcement_id: &str,
    ) -> Result<bool, DataLayerError> {
        match &self.announcement_writer {
            Some(repository) => repository.delete_announcement(announcement_id).await,
            None => Ok(false),
        }
    }

    pub(crate) async fn mark_announcement_as_read(
        &self,
        user_id: &str,
        announcement_id: &str,
        read_at_unix_secs: u64,
    ) -> Result<bool, DataLayerError> {
        match &self.announcement_writer {
            Some(repository) => {
                repository
                    .mark_announcement_as_read(user_id, announcement_id, read_at_unix_secs)
                    .await
            }
            None => Ok(false),
        }
    }

    pub(crate) async fn find_video_task(
        &self,
        key: VideoTaskLookupKey<'_>,
    ) -> Result<Option<StoredVideoTask>, DataLayerError> {
        match &self.video_task_reader {
            Some(repository) => repository.find(key).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn list_video_task_page(
        &self,
        filter: &VideoTaskQueryFilter,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<StoredVideoTask>, DataLayerError> {
        match &self.video_task_reader {
            Some(repository) => repository.list_page(filter, offset, limit).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn list_video_task_page_summary(
        &self,
        filter: &VideoTaskQueryFilter,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<StoredVideoTask>, DataLayerError> {
        match &self.video_task_reader {
            Some(repository) => repository.list_page_summary(filter, offset, limit).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn count_video_tasks(
        &self,
        filter: &VideoTaskQueryFilter,
    ) -> Result<u64, DataLayerError> {
        match &self.video_task_reader {
            Some(repository) => repository.count(filter).await,
            None => Ok(0),
        }
    }

    pub(crate) async fn count_video_tasks_by_status(
        &self,
        filter: &VideoTaskQueryFilter,
    ) -> Result<Vec<VideoTaskStatusCount>, DataLayerError> {
        match &self.video_task_reader {
            Some(repository) => repository.count_by_status(filter).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn count_distinct_video_task_users(
        &self,
        filter: &VideoTaskQueryFilter,
    ) -> Result<u64, DataLayerError> {
        match &self.video_task_reader {
            Some(repository) => repository.count_distinct_users(filter).await,
            None => Ok(0),
        }
    }

    pub(crate) async fn top_video_task_models(
        &self,
        filter: &VideoTaskQueryFilter,
        limit: usize,
    ) -> Result<Vec<VideoTaskModelCount>, DataLayerError> {
        match &self.video_task_reader {
            Some(repository) => repository.top_models(filter, limit).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn count_video_tasks_created_since(
        &self,
        filter: &VideoTaskQueryFilter,
        created_since_unix_secs: u64,
    ) -> Result<u64, DataLayerError> {
        match &self.video_task_reader {
            Some(repository) => {
                repository
                    .count_created_since(filter, created_since_unix_secs)
                    .await
            }
            None => Ok(0),
        }
    }

    pub(crate) async fn upsert_video_task(
        &self,
        task: UpsertVideoTask,
    ) -> Result<Option<StoredVideoTask>, DataLayerError> {
        match &self.video_task_writer {
            Some(repository) => repository.upsert(task).await.map(Some),
            None => Ok(None),
        }
    }

    pub(crate) async fn update_active_video_task(
        &self,
        task: UpsertVideoTask,
    ) -> Result<Option<StoredVideoTask>, DataLayerError> {
        match &self.video_task_writer {
            Some(repository) => repository.update_if_active(task).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn claim_due_video_tasks(
        &self,
        now_unix_secs: u64,
        claim_until_unix_secs: u64,
        limit: usize,
    ) -> Result<Vec<StoredVideoTask>, DataLayerError> {
        match &self.video_task_writer {
            Some(repository) => {
                repository
                    .claim_due(now_unix_secs, claim_until_unix_secs, limit)
                    .await
            }
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn find_wallet(
        &self,
        key: WalletLookupKey<'_>,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        match &self.wallet_reader {
            Some(repository) => repository.find(key).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn list_wallets_by_api_key_ids(
        &self,
        api_key_ids: &[String],
    ) -> Result<Vec<StoredWalletSnapshot>, DataLayerError> {
        match &self.wallet_reader {
            Some(repository) => repository.list_wallets_by_api_key_ids(api_key_ids).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn list_wallets_by_user_ids(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<StoredWalletSnapshot>, DataLayerError> {
        match &self.wallet_reader {
            Some(repository) => repository.list_wallets_by_user_ids(user_ids).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn list_admin_wallets(
        &self,
        query: &AdminWalletListQuery,
    ) -> Result<StoredAdminWalletListPage, DataLayerError> {
        match &self.wallet_reader {
            Some(repository) => repository.list_admin_wallets(query).await,
            None => Ok(StoredAdminWalletListPage::default()),
        }
    }

    pub(crate) async fn adjust_wallet_balance(
        &self,
        input: AdjustWalletBalanceInput,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        match &self.wallet_writer {
            Some(repository) => repository.adjust_wallet_balance(input).await,
            None => Ok(None),
        }
    }
    pub(crate) async fn settle_usage(
        &self,
        input: UsageSettlementInput,
    ) -> Result<Option<StoredUsageSettlement>, DataLayerError> {
        match &self.settlement_writer {
            Some(repository) => repository.settle_usage(input).await,
            None => Ok(None),
        }
    }

    #[allow(dead_code)]

    pub(crate) async fn upsert_usage(
        &self,
        usage: UpsertUsageRecord,
    ) -> Result<Option<StoredRequestUsageAudit>, DataLayerError> {
        crate::request_diagnostics::observe_db_operation(
            "usage_upsert",
            self.database_pool_summary(),
            async {
                match &self.usage_writer {
                    Some(repository) => repository.upsert(usage).await.map(Some),
                    None => Ok(None),
                }
            },
        )
        .await
    }

    pub(crate) async fn upsert_first_byte_usage(
        &self,
        usage: UpsertUsageRecord,
    ) -> Result<(), DataLayerError> {
        crate::request_diagnostics::observe_db_operation(
            "usage_first_byte_upsert",
            self.database_pool_summary(),
            async {
                match &self.usage_writer {
                    Some(repository) => repository.upsert_first_byte(usage).await,
                    None => Ok(()),
                }
            },
        )
        .await
    }

    pub(crate) async fn upsert_first_byte_usage_many(
        &self,
        usages: Vec<UpsertUsageRecord>,
    ) -> Result<(), DataLayerError> {
        if usages.is_empty() {
            return Ok(());
        }
        crate::request_diagnostics::observe_db_operation(
            "usage_first_byte_upsert_batch",
            self.database_pool_summary(),
            async {
                match &self.usage_writer {
                    Some(repository) => repository.upsert_first_byte_many(usages).await,
                    None => Ok(()),
                }
            },
        )
        .await
    }

    pub(crate) async fn upsert_pending_usage_many(
        &self,
        usages: Vec<UpsertUsageRecord>,
    ) -> Result<(), DataLayerError> {
        if usages.is_empty() {
            return Ok(());
        }
        crate::request_diagnostics::observe_db_operation(
            "usage_pending_upsert_batch",
            self.database_pool_summary(),
            async {
                match &self.usage_writer {
                    Some(repository) => repository.upsert_pending_many(usages).await,
                    None => Ok(()),
                }
            },
        )
        .await
    }

    #[allow(dead_code)]
    pub(crate) async fn rebuild_api_key_usage_stats(&self) -> Result<u64, DataLayerError> {
        match &self.usage_writer {
            Some(repository) => repository.rebuild_api_key_usage_stats().await,
            None => Ok(0),
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn rebuild_provider_api_key_usage_stats(&self) -> Result<u64, DataLayerError> {
        match &self.usage_writer {
            Some(repository) => repository.rebuild_provider_api_key_usage_stats().await,
            None => Ok(0),
        }
    }

    pub(crate) async fn flush_usage_counter_deltas(
        &self,
        batch_size: usize,
    ) -> Result<UsageCounterFlushSummary, DataLayerError> {
        match &self.usage_writer {
            Some(repository) => repository.flush_usage_counter_deltas(batch_size).await,
            None => Ok(UsageCounterFlushSummary::default()),
        }
    }

    pub(crate) async fn cleanup_processed_usage_counter_deltas(
        &self,
        cutoff_unix_secs: u64,
        batch_size: usize,
    ) -> Result<usize, DataLayerError> {
        match &self.usage_writer {
            Some(repository) => {
                repository
                    .cleanup_processed_usage_counter_deltas(cutoff_unix_secs, batch_size)
                    .await
            }
            None => Ok(0),
        }
    }

    pub(crate) async fn cleanup_stale_pending_requests(
        &self,
        cutoff_unix_secs: u64,
        now_unix_secs: u64,
        timeout_minutes: u64,
        batch_size: usize,
    ) -> Result<PendingUsageCleanupSummary, DataLayerError> {
        match &self.usage_writer {
            Some(repository) => {
                repository
                    .cleanup_stale_pending_requests(
                        cutoff_unix_secs,
                        now_unix_secs,
                        timeout_minutes,
                        batch_size,
                    )
                    .await
            }
            None => Ok(PendingUsageCleanupSummary::default()),
        }
    }

    pub(crate) async fn cleanup_usage(
        &self,
        window: &UsageCleanupWindow,
        batch_size: usize,
        auto_delete_expired_keys: bool,
        targets: UsageCleanupTargets,
        mode: UsageCleanupExecutionMode,
    ) -> Result<UsageCleanupSummary, DataLayerError> {
        match &self.usage_writer {
            Some(repository) => {
                repository
                    .cleanup_usage(window, batch_size, auto_delete_expired_keys, targets, mode)
                    .await
            }
            None => Ok(UsageCleanupSummary::default()),
        }
    }

    pub(crate) async fn preview_usage_cleanup(
        &self,
        window: &UsageCleanupWindow,
        targets: UsageCleanupTargets,
        mode: UsageCleanupExecutionMode,
    ) -> Result<aether_data_contracts::repository::usage::UsageCleanupPreviewCounts, DataLayerError>
    {
        match &self.usage_writer {
            Some(repository) => {
                repository
                    .preview_usage_cleanup(window, targets, mode)
                    .await
            }
            None => {
                Ok(aether_data_contracts::repository::usage::UsageCleanupPreviewCounts::default())
            }
        }
    }

    pub(crate) async fn find_request_usage_by_request_id(
        &self,
        request_id: &str,
    ) -> Result<Option<StoredRequestUsageAudit>, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => repository.find_by_request_id(request_id).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn find_request_usage_by_request_id_shallow(
        &self,
        request_id: &str,
    ) -> Result<Option<StoredRequestUsageAudit>, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => repository.find_by_request_id_shallow(request_id).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn find_request_usage_by_id(
        &self,
        usage_id: &str,
    ) -> Result<Option<StoredRequestUsageAudit>, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => repository.find_by_id(usage_id).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn list_request_usage_by_ids(
        &self,
        usage_ids: &[String],
    ) -> Result<Vec<StoredRequestUsageAudit>, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => repository.list_by_ids(usage_ids).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn resolve_request_usage_body_ref(
        &self,
        body_ref: &str,
    ) -> Result<Option<serde_json::Value>, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => repository.resolve_body_ref(body_ref).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn list_usage_audits(
        &self,
        query: &UsageAuditListQuery,
    ) -> Result<Vec<StoredRequestUsageAudit>, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => repository.list_usage_audits(query).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn count_usage_audits(
        &self,
        query: &UsageAuditListQuery,
    ) -> Result<u64, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => repository.count_usage_audits(query).await,
            None => Ok(0),
        }
    }

    pub(crate) async fn list_usage_audits_by_keyword_search(
        &self,
        query: &aether_data_contracts::repository::usage::UsageAuditKeywordSearchQuery,
    ) -> Result<Vec<StoredRequestUsageAudit>, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => repository.list_usage_audits_by_keyword_search(query).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn count_usage_audits_by_keyword_search(
        &self,
        query: &aether_data_contracts::repository::usage::UsageAuditKeywordSearchQuery,
    ) -> Result<u64, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => repository.count_usage_audits_by_keyword_search(query).await,
            None => Ok(0),
        }
    }

    pub(crate) async fn aggregate_usage_audits(
        &self,
        query: &aether_data_contracts::repository::usage::UsageAuditAggregationQuery,
    ) -> Result<
        Vec<aether_data_contracts::repository::usage::StoredUsageAuditAggregation>,
        DataLayerError,
    > {
        match &self.usage_reader {
            Some(repository) => repository.aggregate_usage_audits(query).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn summarize_usage_audits(
        &self,
        query: &aether_data_contracts::repository::usage::UsageAuditSummaryQuery,
    ) -> Result<aether_data_contracts::repository::usage::StoredUsageAuditSummary, DataLayerError>
    {
        match &self.usage_reader {
            Some(repository) => repository.summarize_usage_audits(query).await,
            None => {
                Ok(aether_data_contracts::repository::usage::StoredUsageAuditSummary::default())
            }
        }
    }

    pub(crate) async fn read_usage_counter_health(
        &self,
    ) -> Result<UsageCounterHealthSnapshot, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => repository.read_usage_counter_health().await,
            None => Ok(UsageCounterHealthSnapshot::default()),
        }
    }

    pub(crate) async fn read_usage_counter_pending_health(
        &self,
    ) -> Result<UsageCounterPendingHealthSnapshot, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => repository.read_usage_counter_pending_health().await,
            None => Ok(UsageCounterPendingHealthSnapshot::default()),
        }
    }

    pub(crate) async fn summarize_usage_totals_by_user_ids(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<aether_data_contracts::repository::usage::StoredUsageUserTotals>, DataLayerError>
    {
        match &self.usage_reader {
            Some(repository) => {
                repository
                    .summarize_usage_totals_by_user_ids(user_ids)
                    .await
            }
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn summarize_usage_cache_hit_summary(
        &self,
        query: &aether_data_contracts::repository::usage::UsageCacheHitSummaryQuery,
    ) -> Result<aether_data_contracts::repository::usage::StoredUsageCacheHitSummary, DataLayerError>
    {
        match &self.usage_reader {
            Some(repository) => repository.summarize_usage_cache_hit_summary(query).await,
            None => {
                Ok(aether_data_contracts::repository::usage::StoredUsageCacheHitSummary::default())
            }
        }
    }

    pub(crate) async fn summarize_usage_settled_cost(
        &self,
        query: &aether_data_contracts::repository::usage::UsageSettledCostSummaryQuery,
    ) -> Result<
        aether_data_contracts::repository::usage::StoredUsageSettledCostSummary,
        DataLayerError,
    > {
        match &self.usage_reader {
            Some(repository) => repository.summarize_usage_settled_cost(query).await,
            None => Ok(
                aether_data_contracts::repository::usage::StoredUsageSettledCostSummary::default(),
            ),
        }
    }

    pub(crate) async fn summarize_usage_daily_actual_cost_rollups(
        &self,
        query: &UsageDailyActualCostRollupQuery,
    ) -> Result<Vec<StoredUsageDailyActualCostRollup>, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => {
                repository
                    .summarize_usage_daily_actual_cost_rollups(query)
                    .await
            }
            None => Err(DataLayerError::InvalidConfiguration(
                "daily usage recovery requires a usage reader".to_string(),
            )),
        }
    }

    pub(crate) async fn summarize_usage_cache_affinity_hit_summary(
        &self,
        query: &aether_data_contracts::repository::usage::UsageCacheAffinityHitSummaryQuery,
    ) -> Result<
        aether_data_contracts::repository::usage::StoredUsageCacheAffinityHitSummary,
        DataLayerError,
    > {
        match &self.usage_reader {
            Some(repository) => repository
                .summarize_usage_cache_affinity_hit_summary(query)
                .await,
            None => Ok(
                aether_data_contracts::repository::usage::StoredUsageCacheAffinityHitSummary::default(),
            ),
        }
    }

    pub(crate) async fn list_usage_cache_affinity_intervals(
        &self,
        query: &aether_data_contracts::repository::usage::UsageCacheAffinityIntervalQuery,
    ) -> Result<
        Vec<aether_data_contracts::repository::usage::StoredUsageCacheAffinityIntervalRow>,
        DataLayerError,
    > {
        match &self.usage_reader {
            Some(repository) => repository.list_usage_cache_affinity_intervals(query).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn summarize_dashboard_usage(
        &self,
        query: &aether_data_contracts::repository::usage::UsageDashboardSummaryQuery,
    ) -> Result<aether_data_contracts::repository::usage::StoredUsageDashboardSummary, DataLayerError>
    {
        match &self.usage_reader {
            Some(repository) => repository.summarize_dashboard_usage(query).await,
            None => Ok(
                aether_data_contracts::repository::usage::StoredUsageDashboardSummary::default(),
            ),
        }
    }

    pub(crate) async fn summarize_dashboard_stats(
        &self,
        query: &aether_data_contracts::repository::usage::UsageDashboardSummaryQuery,
    ) -> Result<
        aether_data_contracts::repository::usage::StoredUsageDashboardStatsSummary,
        DataLayerError,
    > {
        match &self.usage_reader {
            Some(repository) => repository.summarize_dashboard_stats(query).await,
            None => Ok(
                aether_data_contracts::repository::usage::StoredUsageDashboardStatsSummary::default(
                ),
            ),
        }
    }

    pub(crate) async fn list_dashboard_daily_breakdown(
        &self,
        query: &aether_data_contracts::repository::usage::UsageDashboardDailyBreakdownQuery,
    ) -> Result<
        Vec<aether_data_contracts::repository::usage::StoredUsageDashboardDailyBreakdownRow>,
        DataLayerError,
    > {
        match &self.usage_reader {
            Some(repository) => repository.list_dashboard_daily_breakdown(query).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn summarize_dashboard_provider_counts(
        &self,
        query: &aether_data_contracts::repository::usage::UsageDashboardProviderCountsQuery,
    ) -> Result<
        Vec<aether_data_contracts::repository::usage::StoredUsageDashboardProviderCount>,
        DataLayerError,
    > {
        match &self.usage_reader {
            Some(repository) => repository.summarize_dashboard_provider_counts(query).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn summarize_usage_breakdown(
        &self,
        query: &aether_data_contracts::repository::usage::UsageBreakdownSummaryQuery,
    ) -> Result<
        Vec<aether_data_contracts::repository::usage::StoredUsageBreakdownSummaryRow>,
        DataLayerError,
    > {
        match &self.usage_reader {
            Some(repository) => repository.summarize_usage_breakdown(query).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn count_monitoring_usage_errors(
        &self,
        query: &aether_data_contracts::repository::usage::UsageMonitoringErrorCountQuery,
    ) -> Result<u64, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => repository.count_monitoring_usage_errors(query).await,
            None => Ok(0),
        }
    }

    pub(crate) async fn list_monitoring_usage_errors(
        &self,
        query: &aether_data_contracts::repository::usage::UsageMonitoringErrorListQuery,
    ) -> Result<Vec<StoredRequestUsageAudit>, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => repository.list_monitoring_usage_errors(query).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn summarize_usage_error_distribution(
        &self,
        query: &aether_data_contracts::repository::usage::UsageErrorDistributionQuery,
    ) -> Result<
        Vec<aether_data_contracts::repository::usage::StoredUsageErrorDistributionRow>,
        DataLayerError,
    > {
        match &self.usage_reader {
            Some(repository) => repository.summarize_usage_error_distribution(query).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn summarize_usage_performance_percentiles(
        &self,
        query: &aether_data_contracts::repository::usage::UsagePerformancePercentilesQuery,
    ) -> Result<
        Vec<aether_data_contracts::repository::usage::StoredUsagePerformancePercentilesRow>,
        DataLayerError,
    > {
        match &self.usage_reader {
            Some(repository) => {
                repository
                    .summarize_usage_performance_percentiles(query)
                    .await
            }
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn summarize_usage_provider_performance(
        &self,
        query: &aether_data_contracts::repository::usage::UsageProviderPerformanceQuery,
    ) -> Result<
        aether_data_contracts::repository::usage::StoredUsageProviderPerformance,
        DataLayerError,
    > {
        match &self.usage_reader {
            Some(repository) => repository.summarize_usage_provider_performance(query).await,
            None => Ok(
                aether_data_contracts::repository::usage::StoredUsageProviderPerformance::default(),
            ),
        }
    }

    pub(crate) async fn summarize_usage_cost_savings(
        &self,
        query: &aether_data_contracts::repository::usage::UsageCostSavingsSummaryQuery,
    ) -> Result<
        aether_data_contracts::repository::usage::StoredUsageCostSavingsSummary,
        DataLayerError,
    > {
        match &self.usage_reader {
            Some(repository) => repository.summarize_usage_cost_savings(query).await,
            None => Ok(
                aether_data_contracts::repository::usage::StoredUsageCostSavingsSummary::default(),
            ),
        }
    }

    pub(crate) async fn summarize_usage_time_series(
        &self,
        query: &aether_data_contracts::repository::usage::UsageTimeSeriesQuery,
    ) -> Result<
        Vec<aether_data_contracts::repository::usage::StoredUsageTimeSeriesBucket>,
        DataLayerError,
    > {
        match &self.usage_reader {
            Some(repository) => repository.summarize_usage_time_series(query).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn summarize_usage_leaderboard(
        &self,
        query: &aether_data_contracts::repository::usage::UsageLeaderboardQuery,
    ) -> Result<
        Vec<aether_data_contracts::repository::usage::StoredUsageLeaderboardSummary>,
        DataLayerError,
    > {
        match &self.usage_reader {
            Some(repository) => repository.summarize_usage_leaderboard(query).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn summarize_usage_daily_heatmap(
        &self,
        query: &UsageDailyHeatmapQuery,
    ) -> Result<Vec<StoredUsageDailySummary>, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => repository.summarize_usage_daily_heatmap(query).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn list_recent_usage_audits(
        &self,
        user_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<StoredRequestUsageAudit>, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => repository.list_recent_usage_audits(user_id, limit).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn summarize_usage_total_tokens_by_api_key_ids(
        &self,
        api_key_ids: &[String],
    ) -> Result<std::collections::BTreeMap<String, u64>, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => {
                repository
                    .summarize_total_tokens_by_api_key_ids(api_key_ids)
                    .await
            }
            None => Ok(std::collections::BTreeMap::new()),
        }
    }

    pub(crate) async fn summarize_usage_by_provider_api_key_ids(
        &self,
        provider_api_key_ids: &[String],
    ) -> Result<
        std::collections::BTreeMap<
            String,
            aether_data_contracts::repository::usage::StoredProviderApiKeyUsageSummary,
        >,
        DataLayerError,
    > {
        match &self.usage_reader {
            Some(repository) => {
                repository
                    .summarize_usage_by_provider_api_key_ids(provider_api_key_ids)
                    .await
            }
            None => Ok(std::collections::BTreeMap::new()),
        }
    }

    pub(crate) async fn summarize_usage_by_provider_api_key_windows(
        &self,
        requests: &[ProviderApiKeyWindowUsageRequest],
    ) -> Result<Vec<StoredProviderApiKeyWindowUsageSummary>, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => {
                repository
                    .summarize_usage_by_provider_api_key_windows(requests)
                    .await
            }
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn list_users_by_ids(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<StoredUserSummary>, DataLayerError> {
        match &self.user_reader {
            Some(repository) => repository.list_users_by_ids(user_ids).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn list_users_by_username_search(
        &self,
        username_search: &str,
    ) -> Result<Vec<StoredUserSummary>, DataLayerError> {
        match &self.user_reader {
            Some(repository) => {
                repository
                    .list_users_by_username_search(username_search)
                    .await
            }
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn list_export_users(
        &self,
    ) -> Result<Vec<StoredUserExportRow>, DataLayerError> {
        match &self.user_reader {
            Some(repository) => repository.list_export_users().await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn list_export_users_page(
        &self,
        query: &aether_data::repository::users::UserExportListQuery,
    ) -> Result<Vec<StoredUserExportRow>, DataLayerError> {
        match &self.user_reader {
            Some(repository) => repository.list_export_users_page(query).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn count_export_users(
        &self,
        query: &aether_data::repository::users::UserExportListQuery,
    ) -> Result<u64, DataLayerError> {
        match &self.user_reader {
            Some(repository) => repository.count_export_users(query).await,
            None => Ok(0),
        }
    }

    pub(crate) async fn summarize_export_users(
        &self,
    ) -> Result<aether_data::repository::users::UserExportSummary, DataLayerError> {
        match &self.user_reader {
            Some(repository) => repository.summarize_export_users().await,
            None => Ok(aether_data::repository::users::UserExportSummary::default()),
        }
    }

    pub(crate) async fn find_export_user_by_id(
        &self,
        user_id: &str,
    ) -> Result<Option<StoredUserExportRow>, DataLayerError> {
        match &self.user_reader {
            Some(repository) => repository.find_export_user_by_id(user_id).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn read_user_feature_settings(
        &self,
        user_id: &str,
    ) -> Result<Option<serde_json::Value>, DataLayerError> {
        let user_id = user_id.trim();
        if user_id.is_empty() {
            return Ok(None);
        }
        if let Some(user) = self.find_export_user_by_id(user_id).await? {
            return Ok(user.feature_settings);
        }
        Ok(None)
    }

    pub(crate) async fn list_non_admin_export_users(
        &self,
    ) -> Result<Vec<StoredUserExportRow>, DataLayerError> {
        match &self.user_reader {
            Some(repository) => repository.list_non_admin_export_users().await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn list_user_auth_by_ids(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<StoredUserAuthRecord>, DataLayerError> {
        match &self.user_reader {
            Some(repository) => repository.list_user_auth_by_ids(user_ids).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn summarize_provider_usage_since(
        &self,
        provider_id: &str,
        since_unix_secs: u64,
    ) -> Result<StoredProviderUsageSummary, DataLayerError> {
        match &self.usage_reader {
            Some(repository) => {
                repository
                    .summarize_provider_usage_since(provider_id, since_unix_secs)
                    .await
            }
            None => Ok(StoredProviderUsageSummary::default()),
        }
    }

    pub(crate) fn usage_worker_queue(&self) -> Option<std::sync::Arc<dyn RuntimeQueueStore>> {
        self.usage_worker_queue.clone()
    }

    pub(crate) async fn find_billing_model_context(
        &self,
        provider_id: &str,
        provider_api_key_id: Option<&str>,
        global_model_name: &str,
    ) -> Result<Option<StoredBillingModelContext>, DataLayerError> {
        let key = BillingModelContextCacheKey::ByGlobalModelName {
            provider_id: normalize_billing_context_cache_part(provider_id),
            provider_api_key_id: normalize_optional_billing_context_cache_part(provider_api_key_id),
            global_model_name: normalize_billing_context_cache_part(global_model_name),
        };
        if let Some(value) = self.cached_billing_model_context(&key) {
            return Ok(value);
        }
        loop {
            match self.register_billing_model_context_inflight(&key) {
                BillingModelContextInflightRegistration::Saturated => {
                    return Err(DataLayerError::TimedOut(format!(
                        "billing model context cache admission saturated for {key:?}"
                    )));
                }
                BillingModelContextInflightRegistration::Follower(inflight_state) => {
                    match timeout(
                        Self::BILLING_MODEL_CONTEXT_CACHE_INFLIGHT_WAIT_TIMEOUT,
                        inflight_state.wait(),
                    )
                    .await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => return Err(error),
                        Err(_) => self.expire_billing_model_context_inflight(&key, &inflight_state),
                    }
                    if let Some(value) = self.cached_billing_model_context(&key) {
                        return Ok(value);
                    }
                    continue;
                }
                BillingModelContextInflightRegistration::Leader(mut guard) => {
                    if let Some(value) = self.cached_billing_model_context(&key) {
                        return Ok(value);
                    }
                    let load_epoch = guard.epoch();
                    let result = match timeout(
                        Self::BILLING_MODEL_CONTEXT_CACHE_INFLIGHT_WAIT_TIMEOUT,
                        self.load_billing_model_context_by_name(
                            key,
                            provider_id,
                            provider_api_key_id,
                            global_model_name,
                            load_epoch,
                            &guard.inflight_state,
                        ),
                    )
                    .await
                    {
                        Ok(result) => result,
                        Err(_) => Err(DataLayerError::TimedOut(
                            "billing model context load timed out".to_string(),
                        )),
                    };
                    guard.finish(result.as_ref().err().cloned());
                    return result;
                }
            }
        }
    }

    pub(crate) async fn find_billing_model_context_by_model_id(
        &self,
        provider_id: &str,
        provider_api_key_id: Option<&str>,
        model_id: &str,
    ) -> Result<Option<StoredBillingModelContext>, DataLayerError> {
        let key = BillingModelContextCacheKey::ByModelId {
            provider_id: normalize_billing_context_cache_part(provider_id),
            provider_api_key_id: normalize_optional_billing_context_cache_part(provider_api_key_id),
            model_id: normalize_billing_context_cache_part(model_id),
        };
        if let Some(value) = self.cached_billing_model_context(&key) {
            return Ok(value);
        }
        loop {
            match self.register_billing_model_context_inflight(&key) {
                BillingModelContextInflightRegistration::Saturated => {
                    return Err(DataLayerError::TimedOut(format!(
                        "billing model context cache admission saturated for {key:?}"
                    )));
                }
                BillingModelContextInflightRegistration::Follower(inflight_state) => {
                    match timeout(
                        Self::BILLING_MODEL_CONTEXT_CACHE_INFLIGHT_WAIT_TIMEOUT,
                        inflight_state.wait(),
                    )
                    .await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => return Err(error),
                        Err(_) => self.expire_billing_model_context_inflight(&key, &inflight_state),
                    }
                    if let Some(value) = self.cached_billing_model_context(&key) {
                        return Ok(value);
                    }
                    continue;
                }
                BillingModelContextInflightRegistration::Leader(mut guard) => {
                    if let Some(value) = self.cached_billing_model_context(&key) {
                        return Ok(value);
                    }
                    let load_epoch = guard.epoch();
                    let result = match timeout(
                        Self::BILLING_MODEL_CONTEXT_CACHE_INFLIGHT_WAIT_TIMEOUT,
                        self.load_billing_model_context_by_model_id(
                            key,
                            provider_id,
                            provider_api_key_id,
                            model_id,
                            load_epoch,
                            &guard.inflight_state,
                        ),
                    )
                    .await
                    {
                        Ok(result) => result,
                        Err(_) => Err(DataLayerError::TimedOut(
                            "billing model context load timed out".to_string(),
                        )),
                    };
                    guard.finish(result.as_ref().err().cloned());
                    return result;
                }
            }
        }
    }

    async fn load_billing_model_context_by_name(
        &self,
        key: BillingModelContextCacheKey,
        provider_id: &str,
        provider_api_key_id: Option<&str>,
        global_model_name: &str,
        load_epoch: u64,
        load_flight: &std::sync::Arc<BillingModelContextInflightState>,
    ) -> Result<Option<StoredBillingModelContext>, DataLayerError> {
        crate::request_diagnostics::observe_db_operation(
            "billing_model_context",
            self.database_pool_summary(),
            async {
                match &self.billing_reader {
                    Some(repository) => {
                        let value = repository
                            .find_model_context(provider_id, provider_api_key_id, global_model_name)
                            .await?;
                        self.remember_billing_model_context(
                            key,
                            value.clone(),
                            load_epoch,
                            load_flight,
                        );
                        Ok(value)
                    }
                    None => {
                        self.remember_billing_model_context(key, None, load_epoch, load_flight);
                        Ok(None)
                    }
                }
            },
        )
        .await
    }

    async fn load_billing_model_context_by_model_id(
        &self,
        key: BillingModelContextCacheKey,
        provider_id: &str,
        provider_api_key_id: Option<&str>,
        model_id: &str,
        load_epoch: u64,
        load_flight: &std::sync::Arc<BillingModelContextInflightState>,
    ) -> Result<Option<StoredBillingModelContext>, DataLayerError> {
        crate::request_diagnostics::observe_db_operation(
            "billing_model_context",
            self.database_pool_summary(),
            async {
                match &self.billing_reader {
                    Some(repository) => {
                        let value = repository
                            .find_model_context_by_model_id(
                                provider_id,
                                provider_api_key_id,
                                model_id,
                            )
                            .await?;
                        self.remember_billing_model_context(
                            key,
                            value.clone(),
                            load_epoch,
                            load_flight,
                        );
                        Ok(value)
                    }
                    None => {
                        self.remember_billing_model_context(key, None, load_epoch, load_flight);
                        Ok(None)
                    }
                }
            },
        )
        .await
    }

    fn register_billing_model_context_inflight(
        &self,
        key: &BillingModelContextCacheKey,
    ) -> BillingModelContextInflightRegistration<'_> {
        let mut inflight = self
            .billing_model_context_cache
            .inflight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(inflight_state) = inflight.get(key) {
            return BillingModelContextInflightRegistration::Follower(std::sync::Arc::clone(
                inflight_state,
            ));
        }
        if inflight.len() >= Self::BILLING_MODEL_CONTEXT_CACHE_MAX_INFLIGHT {
            return BillingModelContextInflightRegistration::Saturated;
        }
        let Ok(admission) =
            std::sync::Arc::clone(&self.billing_model_context_cache.admission).try_acquire_owned()
        else {
            return BillingModelContextInflightRegistration::Saturated;
        };
        let inflight_state = std::sync::Arc::new(BillingModelContextInflightState {
            epoch: self
                .billing_model_context_cache
                .epoch
                .load(std::sync::atomic::Ordering::Acquire),
            completion: std::sync::OnceLock::new(),
            notify: tokio::sync::Notify::new(),
        });
        inflight.insert(key.clone(), std::sync::Arc::clone(&inflight_state));
        BillingModelContextInflightRegistration::Leader(BillingModelContextInflightGuard::new(
            self,
            key.clone(),
            inflight_state,
            admission,
        ))
    }

    fn finish_billing_model_context_inflight(
        &self,
        key: &BillingModelContextCacheKey,
        inflight_state: &std::sync::Arc<BillingModelContextInflightState>,
        admission: Option<tokio::sync::OwnedSemaphorePermit>,
    ) -> Option<std::sync::Arc<BillingModelContextInflightState>> {
        let mut inflight = self
            .billing_model_context_cache
            .inflight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        drop(admission);
        if inflight
            .get(key)
            .is_some_and(|current| std::sync::Arc::ptr_eq(current, inflight_state))
        {
            inflight.remove(key)
        } else {
            None
        }
    }

    fn expire_billing_model_context_inflight(
        &self,
        key: &BillingModelContextCacheKey,
        inflight_state: &std::sync::Arc<BillingModelContextInflightState>,
    ) {
        let _mutation = self
            .billing_model_context_cache
            .mutation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let removed = {
            let mut inflight = self
                .billing_model_context_cache
                .inflight
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if inflight
                .get(key)
                .is_some_and(|current| std::sync::Arc::ptr_eq(current, inflight_state))
            {
                inflight.remove(key)
            } else {
                None
            }
        };
        drop(_mutation);
        if let Some(removed) = removed {
            tracing::warn!(
                event_name = "billing_model_context_cache_inflight_expired",
                log_type = "ops",
                cache_key = ?key,
                wait_timeout_ms = Self::BILLING_MODEL_CONTEXT_CACHE_INFLIGHT_WAIT_TIMEOUT.as_millis() as u64,
                "gateway billing model context cache expired stale inflight load"
            );
            removed.complete(Ok(()));
        }
    }

    fn cached_billing_model_context(
        &self,
        key: &BillingModelContextCacheKey,
    ) -> Option<Option<StoredBillingModelContext>> {
        self.billing_model_context_cache
            .entries
            .get_fresh(key, Self::BILLING_MODEL_CONTEXT_CACHE_TTL)
    }

    fn remember_billing_model_context(
        &self,
        key: BillingModelContextCacheKey,
        value: Option<StoredBillingModelContext>,
        load_epoch: u64,
        load_flight: &std::sync::Arc<BillingModelContextInflightState>,
    ) {
        let _mutation = self
            .billing_model_context_cache
            .mutation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if load_epoch
            != self
                .billing_model_context_cache
                .epoch
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        let inflight = self
            .billing_model_context_cache
            .inflight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !inflight
            .get(&key)
            .is_some_and(|current| std::sync::Arc::ptr_eq(current, load_flight))
        {
            return;
        }
        self.billing_model_context_cache.entries.insert(
            key,
            value,
            Self::BILLING_MODEL_CONTEXT_CACHE_TTL,
            Self::BILLING_MODEL_CONTEXT_CACHE_MAX_ENTRIES,
        );
    }

    pub(super) fn clear_billing_model_context_cache(&self) {
        let _mutation = self
            .billing_model_context_cache
            .mutation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.billing_model_context_cache
            .epoch
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.billing_model_context_cache.entries.clear();
        let inflight_states = self
            .billing_model_context_cache
            .inflight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain()
            .map(|(_, state)| state)
            .collect::<Vec<_>>();
        drop(_mutation);
        if !inflight_states.is_empty() {
            tracing::warn!(
                event_name = "billing_model_context_cache_inflight_cleared",
                log_type = "ops",
                "gateway billing model context cache cleared in-flight loads"
            );
            for inflight_state in inflight_states {
                inflight_state.complete(Ok(()));
            }
        }
    }

    pub(crate) async fn read_request_candidate_trace(
        &self,
        request_id: &str,
        attempted_only: bool,
    ) -> Result<Option<RequestCandidateTrace>, DataLayerError> {
        read_request_candidate_trace(self, request_id, attempted_only).await
    }

    pub(crate) async fn read_decision_trace(
        &self,
        request_id: &str,
        attempted_only: bool,
    ) -> Result<Option<DecisionTrace>, DataLayerError> {
        read_decision_trace(self, request_id, attempted_only).await
    }

    pub(crate) async fn read_request_usage_audit(
        &self,
        request_id: &str,
    ) -> Result<Option<StoredRequestUsageAudit>, DataLayerError> {
        self.find_request_usage_by_request_id(request_id).await
    }

    pub(crate) async fn read_request_usage_audit_shallow(
        &self,
        request_id: &str,
    ) -> Result<Option<StoredRequestUsageAudit>, DataLayerError> {
        self.find_request_usage_by_request_id_shallow(request_id)
            .await
    }

    pub(crate) async fn read_request_audit_bundle(
        &self,
        request_id: &str,
        attempted_only: bool,
        now_unix_secs: u64,
    ) -> Result<Option<RequestAuditBundle>, DataLayerError> {
        aether_data::repository::audit::read_request_audit_bundle(
            self,
            request_id,
            attempted_only,
            now_unix_secs,
        )
        .await
    }

    #[allow(dead_code)]
    pub(crate) async fn read_provider_transport_snapshot(
        &self,
        provider_id: &str,
        endpoint_id: &str,
        key_id: &str,
    ) -> Result<Option<GatewayProviderTransportSnapshot>, DataLayerError> {
        read_provider_transport_snapshot(self, provider_id, endpoint_id, key_id).await
    }

    pub(crate) async fn read_video_task_response(
        &self,
        route_family: Option<&str>,
        request_path: &str,
    ) -> Result<Option<LocalVideoTaskReadResponse>, DataLayerError> {
        read_data_backed_video_task_response(self, route_family, request_path).await
    }

    pub(crate) async fn find_background_task_run(
        &self,
        run_id: &str,
    ) -> Result<Option<StoredBackgroundTaskRun>, DataLayerError> {
        match &self.background_task_reader {
            Some(repository) => repository.find_run(run_id).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn list_background_task_runs(
        &self,
        query: &BackgroundTaskListQuery,
    ) -> Result<StoredBackgroundTaskRunPage, DataLayerError> {
        match &self.background_task_reader {
            Some(repository) => repository.list_runs(query).await,
            None => Ok(StoredBackgroundTaskRunPage::default()),
        }
    }

    pub(crate) async fn list_background_task_events(
        &self,
        run_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<StoredBackgroundTaskEvent>, DataLayerError> {
        match &self.background_task_reader {
            Some(repository) => repository.list_events(run_id, offset, limit).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn summarize_background_task_runs(
        &self,
    ) -> Result<BackgroundTaskSummary, DataLayerError> {
        match &self.background_task_reader {
            Some(repository) => repository.summarize_runs().await,
            None => Ok(BackgroundTaskSummary::default()),
        }
    }

    pub(crate) async fn upsert_background_task_run(
        &self,
        run: UpsertBackgroundTaskRun,
    ) -> Result<Option<StoredBackgroundTaskRun>, DataLayerError> {
        match &self.background_task_writer {
            Some(repository) => repository.upsert_run(run).await.map(Some),
            None => Ok(None),
        }
    }

    pub(crate) async fn request_cancel_background_task_run(
        &self,
        run_id: &str,
        updated_at_unix_secs: u64,
    ) -> Result<bool, DataLayerError> {
        match &self.background_task_writer {
            Some(repository) => {
                repository
                    .request_cancel(run_id, updated_at_unix_secs)
                    .await
            }
            None => Ok(false),
        }
    }

    pub(crate) async fn upsert_background_task_event(
        &self,
        event: UpsertBackgroundTaskEvent,
    ) -> Result<Option<StoredBackgroundTaskEvent>, DataLayerError> {
        match &self.background_task_writer {
            Some(repository) => repository.upsert_event(event).await.map(Some),
            None => Ok(None),
        }
    }
}
