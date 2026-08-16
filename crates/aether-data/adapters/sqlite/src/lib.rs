//! SQLite repositories, pool primitives, and migrations.

mod admission;
mod announcements;
mod auth;
mod auth_modules;
mod background_tasks;
mod billing;
mod candidate_selection;
mod candidates;
mod error;
mod global_models;
mod management_tokens;
mod migrations;
mod oauth_providers;
mod pool;
mod provider_catalog;
mod routing_profiles;
mod settlement;
mod usage;
mod users;
mod video_tasks;
mod wallet;

pub use admission::SqliteAdmissionPolicyRepository;
pub use aether_data_contracts::{DataLayerError, DatabaseDriver, SqlDatabaseConfig, SqlPoolConfig};
pub use announcements::SqliteAnnouncementRepository;
pub use auth::SqliteAuthApiKeyReadRepository;
pub use auth_modules::{SqliteAuthModuleReadRepository, SqliteAuthModuleRepository};
pub use background_tasks::SqliteBackgroundTaskRepository;
pub use billing::SqliteBillingReadRepository;
pub use candidate_selection::SqliteMinimalCandidateSelectionReadRepository;
pub use candidates::SqliteRequestCandidateRepository;
pub use global_models::SqliteGlobalModelReadRepository;
pub use management_tokens::SqliteManagementTokenRepository;
pub use migrations::{pending_migrations, prepare_database_for_startup, run_migrations, MIGRATOR};
pub use oauth_providers::SqliteOAuthProviderRepository;
pub use pool::{SqlitePool, SqlitePoolConfig, SqlitePoolFactory};
pub use provider_catalog::SqliteProviderCatalogReadRepository;
pub use routing_profiles::SqliteRoutingGroupRepository;
pub use settlement::SqliteSettlementRepository;
pub use usage::{SqliteUsageReadRepository, SqliteUsageWriteRepository};
pub use users::SqliteUserReadRepository;
pub use video_tasks::SqliteVideoTaskRepository;
pub use wallet::SqliteWalletReadRepository;

use sqlx::{sqlite::SqliteRow, Row};

pub fn sqlite_real(row: &SqliteRow, field: &str) -> Result<f64, DataLayerError> {
    match row.try_get::<f64, _>(field) {
        Ok(value) => Ok(value),
        Err(real_err) => match row.try_get::<i64, _>(field) {
            Ok(value) => Ok(value as f64),
            Err(_) => Err(DataLayerError::sql(real_err)),
        },
    }
}

pub fn sqlite_optional_real(row: &SqliteRow, field: &str) -> Result<Option<f64>, DataLayerError> {
    match row.try_get::<Option<f64>, _>(field) {
        Ok(value) => Ok(value),
        Err(real_err) => match row.try_get::<Option<i64>, _>(field) {
            Ok(value) => Ok(value.map(|value| value as f64)),
            Err(_) => Err(DataLayerError::sql(real_err)),
        },
    }
}
