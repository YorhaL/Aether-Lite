use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::RwLock;

use super::auth::GatewayAuthApiKeySnapshot;
use super::candidates::{read_request_candidate_trace, RequestCandidateTrace};
use super::config::GatewayDataConfig;
use super::decision_trace::{read_decision_trace, DecisionTrace};
use crate::provider_transport::{
    read_provider_transport_snapshot, GatewayProviderTransportSnapshot,
};
use aether_cache::ExpiringMap;
use aether_data::repository::announcements::{
    AnnouncementListQuery, AnnouncementReadRepository, AnnouncementWriteRepository,
    CreateAnnouncementRecord, StoredAnnouncement, StoredAnnouncementPage, UpdateAnnouncementRecord,
};
use aether_data::repository::audit::RequestAuditBundle;
use aether_data::repository::auth::{
    AuthApiKeyLookupKey, AuthApiKeyReadRepository, AuthApiKeyWriteRepository,
    StoredAuthApiKeyExportRecord, StoredAuthApiKeySnapshot,
};
use aether_data::repository::auth_modules::{
    AuthModuleReadRepository, AuthModuleWriteRepository, StoredLdapModuleConfig,
    StoredOAuthProviderModuleConfig,
};
use aether_data::repository::management_tokens::{
    CreateManagementTokenRecord, ManagementTokenListQuery, ManagementTokenReadRepository,
    ManagementTokenWriteRepository, RegenerateManagementTokenSecret, StoredManagementToken,
    StoredManagementTokenListPage, StoredManagementTokenWithUser, UpdateManagementTokenRecord,
};
use aether_data::repository::oauth_providers::{
    OAuthProviderReadRepository, OAuthProviderWriteRepository, StoredOAuthProviderConfig,
    UpsertOAuthProviderConfigRecord,
};
pub(crate) use aether_data::repository::system::{AdminSystemStats, StoredSystemConfigEntry};
use aether_data::repository::users::{
    StoredUserAuthRecord, StoredUserExportRow, StoredUserOAuthLinkSummary, StoredUserSummary,
    UserReadRepository,
};
pub(crate) use aether_data::repository::users::{
    StoredUserPreferenceRecord, StoredUserSessionRecord,
};
use aether_data::repository::wallet::{
    AdjustWalletBalanceInput, AdminWalletListQuery, StoredAdminWalletListPage,
    StoredWalletSnapshot, WalletLookupKey, WalletReadRepository, WalletWriteRepository,
};
use aether_data::{DataBackends, DataLayerError, DatabaseMaintenanceSummary};
use aether_data_contracts::repository::background_tasks::{
    BackgroundTaskListQuery, BackgroundTaskReadRepository, BackgroundTaskSummary,
    BackgroundTaskWriteRepository, StoredBackgroundTaskEvent, StoredBackgroundTaskRun,
    StoredBackgroundTaskRunPage, UpsertBackgroundTaskEvent, UpsertBackgroundTaskRun,
};
use aether_data_contracts::repository::billing::{
    BillingReadRepository, StoredBillingModelContext,
};
use aether_data_contracts::repository::candidate_selection::{
    MinimalCandidateSelectionReadRepository, StoredApiFormatCandidateRowsQuery,
    StoredMinimalCandidateSelectionRow, StoredRequestedModelCandidateRowsQuery,
};
use aether_data_contracts::repository::candidates::{
    PublicHealthStatusCount, PublicHealthTimelineBucket, RequestCandidateReadRepository,
    RequestCandidateWriteRepository, StoredRequestCandidate, UpsertRequestCandidateRecord,
};
use aether_data_contracts::repository::global_models::{
    AdminGlobalModelListQuery, AdminProviderModelListQuery, CreateAdminGlobalModelRecord,
    GlobalModelReadRepository, GlobalModelWriteRepository, PublicCatalogModelListQuery,
    PublicCatalogModelSearchQuery, PublicGlobalModelQuery, StoredAdminGlobalModel,
    StoredAdminGlobalModelPage, StoredAdminProviderModel, StoredProviderActiveGlobalModel,
    StoredProviderModelStats, StoredPublicCatalogModel, StoredPublicGlobalModel,
    StoredPublicGlobalModelPage, UpdateAdminGlobalModelRecord, UpsertAdminProviderModelRecord,
};
use aether_data_contracts::repository::provider_catalog::{
    ProviderCatalogKeyAdaptiveStateUpdate, ProviderCatalogKeyHealthStateUpdate,
    ProviderCatalogKeyListQuery, ProviderCatalogKeyRuntimeMetadataUpdate,
    ProviderCatalogKeyStatusSnapshotUpdate, ProviderCatalogReadRepository,
    ProviderCatalogWriteRepository, StoredProviderCatalogEndpoint, StoredProviderCatalogKey,
    StoredProviderCatalogKeyMaintenanceSummary, StoredProviderCatalogKeyPage,
    StoredProviderCatalogKeyStats, StoredProviderCatalogProvider,
};
use aether_data_contracts::repository::routing_profiles::{
    RoutingGroupReadRepository, RoutingGroupWriteRepository,
};
use aether_data_contracts::repository::settlement::{
    SettlementWriteRepository, StoredUsageSettlement, UsageSettlementInput,
};
use aether_data_contracts::repository::usage::{
    ApiKeyLastUsedDelta, ManagementTokenCounterDelta, PendingUsageCleanupSummary,
    StoredProviderUsageSummary, StoredRequestUsageAudit, StoredUsageDailyActualCostRollup,
    UpsertUsageRecord, UsageDailyActualCostRollupQuery, UsageReadRepository, UsageWriteRepository,
};
use aether_runtime_state::{RuntimeQueueStore, RuntimeState};

#[derive(Clone, Default)]
pub(crate) struct GatewayDataState {
    config: GatewayDataConfig,
    backends: Option<DataBackends>,
    auth_api_key_reader: Option<Arc<dyn AuthApiKeyReadRepository>>,
    auth_api_key_writer: Option<Arc<dyn AuthApiKeyWriteRepository>>,
    auth_module_reader: Option<Arc<dyn AuthModuleReadRepository>>,
    auth_module_writer: Option<Arc<dyn AuthModuleWriteRepository>>,
    announcement_reader: Option<Arc<dyn AnnouncementReadRepository>>,
    announcement_writer: Option<Arc<dyn AnnouncementWriteRepository>>,
    management_token_reader: Option<Arc<dyn ManagementTokenReadRepository>>,
    management_token_writer: Option<Arc<dyn ManagementTokenWriteRepository>>,
    oauth_provider_reader: Option<Arc<dyn OAuthProviderReadRepository>>,
    oauth_provider_writer: Option<Arc<dyn OAuthProviderWriteRepository>>,
    billing_reader: Option<Arc<dyn BillingReadRepository>>,
    background_task_reader: Option<Arc<dyn BackgroundTaskReadRepository>>,
    background_task_writer: Option<Arc<dyn BackgroundTaskWriteRepository>>,
    global_model_reader: Option<Arc<dyn GlobalModelReadRepository>>,
    global_model_writer: Option<Arc<dyn GlobalModelWriteRepository>>,
    minimal_candidate_selection_reader: Option<Arc<dyn MinimalCandidateSelectionReadRepository>>,
    request_candidate_reader: Option<Arc<dyn RequestCandidateReadRepository>>,
    request_candidate_writer: Option<Arc<dyn RequestCandidateWriteRepository>>,
    provider_catalog_reader: Option<Arc<dyn ProviderCatalogReadRepository>>,
    provider_catalog_writer: Option<Arc<dyn ProviderCatalogWriteRepository>>,
    routing_group_reader: Option<Arc<dyn RoutingGroupReadRepository>>,
    routing_group_writer: Option<Arc<dyn RoutingGroupWriteRepository>>,
    usage_reader: Option<Arc<dyn UsageReadRepository>>,
    usage_writer: Option<Arc<dyn UsageWriteRepository>>,
    user_reader: Option<Arc<dyn UserReadRepository>>,
    user_preferences: Option<Arc<RwLock<BTreeMap<String, StoredUserPreferenceRecord>>>>,
    usage_worker_queue: Option<Arc<dyn RuntimeQueueStore>>,
    daily_usage_runtime_state: Option<Arc<RuntimeState>>,
    wallet_reader: Option<Arc<dyn WalletReadRepository>>,
    wallet_writer: Option<Arc<dyn WalletWriteRepository>>,
    settlement_writer: Option<Arc<dyn SettlementWriteRepository>>,
    system_config_values: Option<Arc<RwLock<BTreeMap<String, StoredSystemConfigEntry>>>>,
    system_config_value_cache: Arc<SystemConfigValueCacheState>,
    billing_model_context_cache: Arc<BillingModelContextCacheState>,
}

pub(super) struct SystemConfigValueCacheState {
    pub(super) entries: ExpiringMap<String, Option<serde_json::Value>>,
    pub(super) inflight: std::sync::Mutex<HashMap<String, Arc<SystemConfigValueInflightState>>>,
    pub(super) mutation: std::sync::Mutex<()>,
    pub(super) admission: Arc<tokio::sync::Semaphore>,
}

pub(super) struct SystemConfigValueInflightState {
    pub(super) notify: Arc<tokio::sync::Notify>,
    pub(super) completion: OnceLock<SystemConfigValueInflightCompletion>,
}

#[derive(Clone)]
pub(super) enum SystemConfigValueInflightCompletion {
    Loaded,
    Failed(DataLayerError),
    Cancelled,
    Invalidated,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) enum BillingModelContextCacheKey {
    ByModelId {
        provider_id: String,
        provider_api_key_id: Option<String>,
        model_id: String,
    },
    ByGlobalModelName {
        provider_id: String,
        provider_api_key_id: Option<String>,
        global_model_name: String,
    },
}

pub(super) struct BillingModelContextCacheState {
    pub(super) entries: ExpiringMap<BillingModelContextCacheKey, Option<StoredBillingModelContext>>,
    pub(super) inflight: std::sync::Mutex<
        HashMap<BillingModelContextCacheKey, Arc<BillingModelContextInflightState>>,
    >,
    pub(super) epoch: std::sync::atomic::AtomicU64,
    pub(super) mutation: std::sync::Mutex<()>,
    pub(super) admission: Arc<tokio::sync::Semaphore>,
}

pub(super) struct BillingModelContextInflightState {
    pub(super) epoch: u64,
    pub(super) completion: std::sync::OnceLock<Result<(), DataLayerError>>,
    pub(super) notify: tokio::sync::Notify,
}

impl fmt::Debug for GatewayDataState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GatewayDataState")
            .field("config", &self.config)
            .field("has_backends", &self.backends.is_some())
            .field(
                "has_auth_api_key_reader",
                &self.auth_api_key_reader.is_some(),
            )
            .field(
                "has_auth_api_key_writer",
                &self.auth_api_key_writer.is_some(),
            )
            .field("has_auth_module_reader", &self.auth_module_reader.is_some())
            .field("has_auth_module_writer", &self.auth_module_writer.is_some())
            .field(
                "has_announcement_reader",
                &self.announcement_reader.is_some(),
            )
            .field(
                "has_announcement_writer",
                &self.announcement_writer.is_some(),
            )
            .field(
                "has_management_token_reader",
                &self.management_token_reader.is_some(),
            )
            .field(
                "has_management_token_writer",
                &self.management_token_writer.is_some(),
            )
            .field(
                "has_oauth_provider_reader",
                &self.oauth_provider_reader.is_some(),
            )
            .field(
                "has_oauth_provider_writer",
                &self.oauth_provider_writer.is_some(),
            )
            .field("has_billing_reader", &self.billing_reader.is_some())
            .field(
                "has_background_task_reader",
                &self.background_task_reader.is_some(),
            )
            .field(
                "has_background_task_writer",
                &self.background_task_writer.is_some(),
            )
            .field(
                "has_global_model_reader",
                &self.global_model_reader.is_some(),
            )
            .field(
                "has_global_model_writer",
                &self.global_model_writer.is_some(),
            )
            .field(
                "has_minimal_candidate_selection_reader",
                &self.minimal_candidate_selection_reader.is_some(),
            )
            .field(
                "has_request_candidate_reader",
                &self.request_candidate_reader.is_some(),
            )
            .field(
                "has_request_candidate_writer",
                &self.request_candidate_writer.is_some(),
            )
            .field(
                "has_provider_catalog_reader",
                &self.provider_catalog_reader.is_some(),
            )
            .field(
                "has_provider_catalog_writer",
                &self.provider_catalog_writer.is_some(),
            )
            .field(
                "has_routing_group_reader",
                &self.routing_group_reader.is_some(),
            )
            .field(
                "has_routing_group_writer",
                &self.routing_group_writer.is_some(),
            )
            .field("has_usage_reader", &self.usage_reader.is_some())
            .field("has_usage_writer", &self.usage_writer.is_some())
            .field("has_user_preferences", &self.user_preferences.is_some())
            .field("has_usage_worker_queue", &self.usage_worker_queue.is_some())
            .field("has_wallet_reader", &self.wallet_reader.is_some())
            .field("has_wallet_writer", &self.wallet_writer.is_some())
            .field("has_settlement_writer", &self.settlement_writer.is_some())
            .field(
                "has_system_config_values",
                &self.system_config_values.is_some(),
            )
            .finish()
    }
}

mod auth;
pub(crate) use admission::{GatewayAuthApiKeyExportRecord, GatewayUserGroup};
pub(crate) use auth::GatewayUserGroupPolicySets;
mod admission;
mod auth_api_key_cache;
mod candidate_cache;
mod catalog;
mod core;
mod integrations;
mod models;
mod provider_catalog_cache;
mod request_candidate_cache;
mod routing_group_cache;
mod routing_profiles;
mod runtime;
#[cfg(test)]
mod testing;
#[cfg(feature = "testkit")]
pub(crate) mod testkit;
