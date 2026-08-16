//! Runtime database migration entry points.
//!
//! Each driver owns its migrator and startup preparation. This module adds the
//! Postgres empty-database bootstrap and exposes the application entry points.

#[cfg(feature = "postgres")]
mod postgres;

#[cfg(all(test, feature = "postgres", feature = "sqlite"))]
mod tests;

pub use aether_data_contracts::PendingMigrationInfo;
#[cfg(feature = "sqlite")]
pub use aether_data_sqlite::{
    pending_migrations as pending_sqlite_migrations,
    prepare_database_for_startup as prepare_sqlite_database_for_startup,
    run_migrations as run_sqlite_migrations,
};
#[cfg(feature = "postgres")]
pub use postgres::{pending_migrations, prepare_database_for_startup, run_migrations};
