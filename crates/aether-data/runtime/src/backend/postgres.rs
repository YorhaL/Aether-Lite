use std::sync::Arc;

use crate::repository::announcements::{
    AnnouncementReadRepository, AnnouncementWriteRepository, SqlxAnnouncementReadRepository,
};
use crate::repository::audit::{AuditLogReadRepository, PostgresAuditLogReadRepository};
use crate::repository::auth::{
    AuthApiKeyReadRepository, AuthApiKeyWriteRepository, SqlxAuthApiKeySnapshotReadRepository,
};
use crate::repository::auth_modules::{
    AuthModuleReadRepository, AuthModuleWriteRepository, SqlxAuthModuleReadRepository,
    SqlxAuthModuleRepository,
};
use crate::repository::background_tasks::{
    BackgroundTaskReadRepository, BackgroundTaskWriteRepository, SqlxBackgroundTaskRepository,
};
use crate::repository::billing::{BillingReadRepository, SqlxBillingReadRepository};
use crate::repository::candidate_selection::{
    MinimalCandidateSelectionReadRepository, SqlxMinimalCandidateSelectionReadRepository,
};
use crate::repository::candidates::{
    RequestCandidateReadRepository, RequestCandidateWriteRepository,
    SqlxRequestCandidateReadRepository,
};
use crate::repository::gemini_file_mappings::{
    GeminiFileMappingReadRepository, GeminiFileMappingWriteRepository,
    SqlxGeminiFileMappingRepository,
};
use crate::repository::global_models::{
    GlobalModelReadRepository, GlobalModelWriteRepository, SqlxGlobalModelReadRepository,
};
use crate::repository::management_tokens::{
    ManagementTokenReadRepository, ManagementTokenWriteRepository, SqlxManagementTokenRepository,
};
use crate::repository::oauth_providers::{
    OAuthProviderReadRepository, OAuthProviderWriteRepository, SqlxOAuthProviderRepository,
};
use crate::repository::provider_catalog::{
    ProviderCatalogReadRepository, ProviderCatalogWriteRepository,
    SqlxProviderCatalogReadRepository,
};
use crate::repository::routing_profiles::{
    PostgresRoutingGroupRepository, RoutingGroupReadRepository, RoutingGroupWriteRepository,
};
use crate::repository::settlement::{SettlementWriteRepository, SqlxSettlementRepository};
use crate::repository::usage::{
    SqlxUsageReadRepository, UsageReadRepository, UsageWriteRepository,
};
use crate::repository::users::{SqlxUserReadRepository, UserReadRepository};
use crate::repository::video_tasks::{
    SqlxVideoTaskReadRepository, SqlxVideoTaskRepository, VideoTaskReadRepository,
    VideoTaskWriteRepository,
};
use crate::repository::wallet::{
    SqlxWalletRepository, WalletReadRepository, WalletWriteRepository,
};
use crate::DataLayerError;
use aether_data_postgres::{
    PostgresLeaseRunner, PostgresLeaseRunnerConfig, PostgresPool, PostgresPoolConfig,
    PostgresPoolFactory, PostgresTransactionRunner,
};

#[derive(Debug, Clone)]
pub struct PostgresBackend {
    config: PostgresPoolConfig,
    pool: PostgresPool,
}

impl PostgresBackend {
    pub fn from_config(config: PostgresPoolConfig) -> Result<Self, DataLayerError> {
        let factory = PostgresPoolFactory::new(config.clone())?;
        let pool = factory.connect_lazy()?;

        Ok(Self { config, pool })
    }

    pub fn config(&self) -> &PostgresPoolConfig {
        &self.config
    }

    pub fn pool(&self) -> &PostgresPool {
        &self.pool
    }

    pub fn pool_clone(&self) -> PostgresPool {
        self.pool.clone()
    }

    pub fn auth_api_key_read_repository(&self) -> Arc<dyn AuthApiKeyReadRepository> {
        Arc::new(SqlxAuthApiKeySnapshotReadRepository::new(self.pool_clone()))
    }

    pub fn announcement_read_repository(&self) -> Arc<dyn AnnouncementReadRepository> {
        Arc::new(SqlxAnnouncementReadRepository::new(self.pool_clone()))
    }

    pub fn audit_log_read_repository(&self) -> Arc<dyn AuditLogReadRepository> {
        Arc::new(PostgresAuditLogReadRepository::new(self.pool_clone()))
    }

    pub fn announcement_write_repository(&self) -> Arc<dyn AnnouncementWriteRepository> {
        Arc::new(SqlxAnnouncementReadRepository::new(self.pool_clone()))
    }

    pub fn auth_api_key_write_repository(&self) -> Arc<dyn AuthApiKeyWriteRepository> {
        Arc::new(SqlxAuthApiKeySnapshotReadRepository::new(self.pool_clone()))
    }

    pub fn auth_module_read_repository(&self) -> Arc<dyn AuthModuleReadRepository> {
        Arc::new(SqlxAuthModuleReadRepository::new(self.pool_clone()))
    }

    pub fn auth_module_write_repository(&self) -> Arc<dyn AuthModuleWriteRepository> {
        Arc::new(SqlxAuthModuleRepository::new(self.pool_clone()))
    }

    pub fn billing_read_repository(&self) -> Arc<dyn BillingReadRepository> {
        Arc::new(SqlxBillingReadRepository::new(self.pool_clone()))
    }

    pub fn background_task_read_repository(&self) -> Arc<dyn BackgroundTaskReadRepository> {
        Arc::new(SqlxBackgroundTaskRepository::new(self.pool_clone()))
    }

    pub fn background_task_write_repository(&self) -> Arc<dyn BackgroundTaskWriteRepository> {
        Arc::new(SqlxBackgroundTaskRepository::new(self.pool_clone()))
    }

    pub fn minimal_candidate_selection_read_repository(
        &self,
    ) -> Arc<dyn MinimalCandidateSelectionReadRepository> {
        Arc::new(SqlxMinimalCandidateSelectionReadRepository::new(
            self.pool_clone(),
        ))
    }

    pub fn request_candidate_read_repository(&self) -> Arc<dyn RequestCandidateReadRepository> {
        Arc::new(SqlxRequestCandidateReadRepository::new(self.pool_clone()))
    }

    pub fn request_candidate_write_repository(&self) -> Arc<dyn RequestCandidateWriteRepository> {
        Arc::new(SqlxRequestCandidateReadRepository::new(self.pool_clone()))
    }

    pub fn gemini_file_mapping_read_repository(&self) -> Arc<dyn GeminiFileMappingReadRepository> {
        Arc::new(SqlxGeminiFileMappingRepository::new(self.pool_clone()))
    }

    pub fn gemini_file_mapping_write_repository(
        &self,
    ) -> Arc<dyn GeminiFileMappingWriteRepository> {
        Arc::new(SqlxGeminiFileMappingRepository::new(self.pool_clone()))
    }

    pub fn global_model_read_repository(&self) -> Arc<dyn GlobalModelReadRepository> {
        Arc::new(SqlxGlobalModelReadRepository::new(self.pool_clone()))
    }

    pub fn global_model_write_repository(&self) -> Arc<dyn GlobalModelWriteRepository> {
        Arc::new(SqlxGlobalModelReadRepository::new(self.pool_clone()))
    }

    pub fn management_token_read_repository(&self) -> Arc<dyn ManagementTokenReadRepository> {
        Arc::new(SqlxManagementTokenRepository::new(self.pool_clone()))
    }

    pub fn management_token_write_repository(&self) -> Arc<dyn ManagementTokenWriteRepository> {
        Arc::new(SqlxManagementTokenRepository::new(self.pool_clone()))
    }

    pub fn oauth_provider_read_repository(&self) -> Arc<dyn OAuthProviderReadRepository> {
        Arc::new(SqlxOAuthProviderRepository::new(self.pool_clone()))
    }

    pub fn oauth_provider_write_repository(&self) -> Arc<dyn OAuthProviderWriteRepository> {
        Arc::new(SqlxOAuthProviderRepository::new(self.pool_clone()))
    }

    pub fn provider_catalog_read_repository(&self) -> Arc<dyn ProviderCatalogReadRepository> {
        Arc::new(SqlxProviderCatalogReadRepository::new(self.pool_clone()))
    }

    pub fn provider_catalog_write_repository(&self) -> Arc<dyn ProviderCatalogWriteRepository> {
        Arc::new(SqlxProviderCatalogReadRepository::new(self.pool_clone()))
    }

    pub fn routing_group_read_repository(&self) -> Arc<dyn RoutingGroupReadRepository> {
        Arc::new(PostgresRoutingGroupRepository::new(self.pool_clone()))
    }

    pub fn routing_group_write_repository(&self) -> Arc<dyn RoutingGroupWriteRepository> {
        Arc::new(PostgresRoutingGroupRepository::new(self.pool_clone()))
    }

    pub fn usage_read_repository(&self) -> Arc<dyn UsageReadRepository> {
        Arc::new(SqlxUsageReadRepository::new(self.pool_clone()))
    }

    pub fn user_read_repository(&self) -> Arc<dyn UserReadRepository> {
        Arc::new(SqlxUserReadRepository::new(self.pool_clone()))
    }

    pub fn usage_write_repository(&self) -> Arc<dyn UsageWriteRepository> {
        Arc::new(SqlxUsageReadRepository::new(self.pool_clone()))
    }

    pub fn wallet_read_repository(&self) -> Arc<dyn WalletReadRepository> {
        Arc::new(SqlxWalletRepository::new(self.pool_clone()))
    }

    pub fn wallet_write_repository(&self) -> Arc<dyn WalletWriteRepository> {
        Arc::new(SqlxWalletRepository::new(self.pool_clone()))
    }

    pub fn settlement_write_repository(&self) -> Arc<dyn SettlementWriteRepository> {
        Arc::new(SqlxSettlementRepository::new(self.pool_clone()))
    }

    pub fn video_task_read_repository(&self) -> Arc<dyn VideoTaskReadRepository> {
        Arc::new(SqlxVideoTaskReadRepository::new(self.pool_clone()))
    }

    pub fn video_task_write_repository(&self) -> Arc<dyn VideoTaskWriteRepository> {
        Arc::new(SqlxVideoTaskRepository::new(self.pool_clone()))
    }

    pub fn transaction_runner(&self) -> PostgresTransactionRunner {
        PostgresTransactionRunner::new(self.pool_clone())
    }

    pub fn lease_runner(
        &self,
        config: PostgresLeaseRunnerConfig,
    ) -> Result<PostgresLeaseRunner, DataLayerError> {
        PostgresLeaseRunner::new(self.transaction_runner(), config)
    }
}
