#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "sqlite")]
mod sqlite;
mod types;

#[cfg(all(test, feature = "postgres", feature = "sqlite"))]
mod tests;

#[cfg(feature = "postgres")]
pub use postgres::{pending_backfills, run_backfills};
#[cfg(feature = "sqlite")]
pub use sqlite::{
    pending_backfills as pending_sqlite_backfills, run_backfills as run_sqlite_backfills,
};
pub use types::PendingBackfillInfo;

#[cfg(all(test, feature = "postgres", feature = "sqlite"))]
use postgres::{pending_backfills_from_applied, AppliedBackfill};
