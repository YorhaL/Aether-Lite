//! Backend composition layer.
//!
//! `DataBackends` chooses the configured SQL driver, builds low-level pools,
//! instantiates concrete repositories, and exposes app-facing read/write,
//! lease, transaction, and maintenance handles. Request-path repository SQL
//! belongs in the selected `aether-data-*` adapter; backend-owned maintenance
//! SQL lives in focused modules such as `stats` and `system`.

mod leases;
mod maintenance;
#[cfg(feature = "postgres")]
mod postgres;
mod privacy;
mod read;
#[cfg(feature = "sqlite")]
mod sqlite;
mod stats;
#[cfg(feature = "sqlite")]
mod stats_common;
mod system;
mod transactions;
mod write;

use crate::maintenance::DatabasePoolSummary;
pub use leases::DataLeaseBackends;
#[cfg(feature = "postgres")]
pub use postgres::PostgresBackend;
pub use privacy::PrivacyDataState;
pub use read::DataReadRepositories;
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteBackend;
pub use transactions::DataTransactionBackends;
pub use write::DataWriteRepositories;

use crate::DatabaseDriver;
use crate::{DataLayerConfig, DataLayerError};

#[derive(Clone, Copy)]
enum SqlBackendRef<'a> {
    #[cfg(feature = "postgres")]
    Postgres(&'a PostgresBackend),
    #[cfg(feature = "sqlite")]
    Sqlite(&'a SqliteBackend),
}

#[derive(Debug, Clone, Default)]
pub struct DataBackends {
    config: DataLayerConfig,
    #[cfg(feature = "postgres")]
    postgres: Option<PostgresBackend>,
    #[cfg(feature = "sqlite")]
    sqlite: Option<SqliteBackend>,
    leases: DataLeaseBackends,
    read: DataReadRepositories,
    transactions: DataTransactionBackends,
    write: DataWriteRepositories,
}

fn summarize_pool(
    driver: DatabaseDriver,
    pool_size: usize,
    idle: usize,
    max_connections: u32,
) -> DatabasePoolSummary {
    let max_connections = max_connections.max(1);
    let checked_out = pool_size.saturating_sub(idle);
    let usage_rate = checked_out as f64 / f64::from(max_connections) * 100.0;

    DatabasePoolSummary {
        driver,
        checked_out,
        pool_size,
        idle,
        max_connections,
        usage_rate,
    }
}

fn ensure_driver_enabled(driver: DatabaseDriver) -> Result<(), DataLayerError> {
    match driver {
        #[cfg(feature = "postgres")]
        DatabaseDriver::Postgres => Ok(()),
        #[cfg(not(feature = "postgres"))]
        DatabaseDriver::Postgres => Err(DataLayerError::InvalidInput(
            "PostgreSQL driver is not enabled for this aether-data build".to_string(),
        )),
        #[cfg(feature = "sqlite")]
        DatabaseDriver::Sqlite => Ok(()),
        #[cfg(not(feature = "sqlite"))]
        DatabaseDriver::Sqlite => Err(DataLayerError::InvalidInput(
            "SQLite driver is not enabled for this aether-data build".to_string(),
        )),
    }
}

impl DataBackends {
    fn sql_backend(&self) -> Option<SqlBackendRef<'_>> {
        #[cfg(feature = "postgres")]
        if let Some(postgres) = self.postgres.as_ref() {
            return Some(SqlBackendRef::Postgres(postgres));
        }
        #[cfg(feature = "sqlite")]
        if let Some(sqlite) = self.sqlite.as_ref() {
            return Some(SqlBackendRef::Sqlite(sqlite));
        }
        None
    }

    pub fn from_config(config: DataLayerConfig) -> Result<Self, DataLayerError> {
        config.validate()?;

        let database = config.effective_database();
        if let Some(database) = database.as_ref() {
            ensure_driver_enabled(database.driver)?;
        }
        #[cfg(feature = "postgres")]
        let postgres = match database.clone() {
            Some(database) if database.driver == DatabaseDriver::Postgres => Some(
                PostgresBackend::from_config(database.to_postgres_config()?)?,
            ),
            _ => None,
        };
        #[cfg(feature = "sqlite")]
        let sqlite = match database.clone() {
            Some(database) if database.driver == DatabaseDriver::Sqlite => {
                Some(SqliteBackend::from_config(database)?)
            }
            _ => None,
        };
        #[cfg(feature = "postgres")]
        let leases = DataLeaseBackends::from_postgres(postgres.as_ref())?;
        #[cfg(not(feature = "postgres"))]
        let leases = DataLeaseBackends::default();
        let read = DataReadRepositories::from_backends(
            #[cfg(feature = "postgres")]
            postgres.as_ref(),
            #[cfg(feature = "sqlite")]
            sqlite.as_ref(),
        );
        #[cfg(feature = "postgres")]
        let transactions = DataTransactionBackends::from_postgres(postgres.as_ref());
        #[cfg(not(feature = "postgres"))]
        let transactions = DataTransactionBackends::default();
        let write = DataWriteRepositories::from_backends(
            #[cfg(feature = "postgres")]
            postgres.as_ref(),
            #[cfg(feature = "sqlite")]
            sqlite.as_ref(),
        );

        Ok(Self {
            config,
            #[cfg(feature = "postgres")]
            postgres,
            #[cfg(feature = "sqlite")]
            sqlite,
            leases,
            read,
            transactions,
            write,
        })
    }

    pub fn config(&self) -> &DataLayerConfig {
        &self.config
    }

    #[cfg(feature = "postgres")]
    pub fn postgres(&self) -> Option<&PostgresBackend> {
        self.postgres.as_ref()
    }

    pub fn database_driver(&self) -> Option<DatabaseDriver> {
        self.config
            .effective_database()
            .map(|database| database.driver)
    }

    #[cfg(feature = "sqlite")]
    pub fn sqlite(&self) -> Option<&SqliteBackend> {
        self.sqlite.as_ref()
    }

    pub fn read(&self) -> &DataReadRepositories {
        &self.read
    }

    pub fn leases(&self) -> &DataLeaseBackends {
        &self.leases
    }

    pub fn transactions(&self) -> &DataTransactionBackends {
        &self.transactions
    }

    pub fn write(&self) -> &DataWriteRepositories {
        &self.write
    }

    pub fn has_runtime_backends(&self) -> bool {
        self.leases.has_any()
            || self.read.has_any()
            || self.transactions.has_any()
            || self.write.has_any()
    }
}
