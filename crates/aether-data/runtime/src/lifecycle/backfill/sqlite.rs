use std::collections::{HashMap, HashSet};

use sqlx::{
    migrate::{Migrate, MigrateError, Migrator},
    query, Connection, Row, SqliteConnection,
};
use tracing::{error, info, warn};

use super::types::PendingBackfillInfo;
use aether_data_sqlite::SqlitePool;

static BACKFILL_MIGRATOR: Migrator = sqlx::migrate!("./backfills/sqlite");

const ENSURE_SCHEMA_BACKFILLS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS schema_backfills (
    version INTEGER NOT NULL PRIMARY KEY,
    description TEXT NOT NULL,
    success INTEGER NOT NULL DEFAULT 1,
    checksum BLOB NOT NULL,
    execution_time INTEGER NOT NULL DEFAULT 0,
    applied_at INTEGER NOT NULL DEFAULT (CAST(strftime('%s', 'now') AS INTEGER))
)
"#;
const LIST_APPLIED_BACKFILLS_SQL: &str = r#"
SELECT version, checksum
FROM schema_backfills
WHERE success = 1
ORDER BY version ASC
"#;
const INSERT_APPLIED_BACKFILL_SQL: &str = r#"
INSERT INTO schema_backfills (
    version,
    description,
    success,
    checksum,
    execution_time,
    applied_at
) VALUES (
    ?,
    ?,
    1,
    ?,
    ?,
    CAST(strftime('%s', 'now') AS INTEGER)
)
ON CONFLICT(version) DO NOTHING
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
struct AppliedBackfill {
    version: i64,
    checksum: Vec<u8>,
}

pub async fn run_backfills(pool: &SqlitePool) -> Result<(), MigrateError> {
    let mut conn = pool.acquire().await?;

    if BACKFILL_MIGRATOR.locking {
        conn.lock().await?;
    }

    let result = run_backfills_locked(&mut conn).await;

    if BACKFILL_MIGRATOR.locking {
        match conn.unlock().await {
            Ok(()) => {}
            Err(unlock_error) if result.is_ok() => return Err(unlock_error),
            Err(unlock_error) => {
                warn!(
                    error = %unlock_error,
                    "sqlite database backfill lock release failed after backfill error"
                );
            }
        }
    }

    result
}

pub async fn pending_backfills(
    pool: &SqlitePool,
) -> Result<Vec<PendingBackfillInfo>, MigrateError> {
    let mut conn = pool.acquire().await?;
    pending_backfills_locked(&mut conn).await
}

async fn run_backfills_locked(conn: &mut SqliteConnection) -> Result<(), MigrateError> {
    ensure_schema_backfills_table(conn).await?;

    let applied_backfills = list_applied_backfills(conn).await?;
    validate_applied_backfills(&applied_backfills)?;

    let applied_by_version: HashMap<_, _> = applied_backfills
        .iter()
        .map(|backfill| (backfill.version, backfill))
        .collect();
    let pending_backfills: Vec<_> = BACKFILL_MIGRATOR
        .iter()
        .filter(|backfill| backfill.migration_type.is_up_migration())
        .filter(|backfill| !applied_by_version.contains_key(&backfill.version))
        .collect();

    if pending_backfills.is_empty() {
        info!(
            driver = "sqlite",
            pending_backfills = 0,
            "database backfills already up to date"
        );
        return Ok(());
    }

    info!(
        driver = "sqlite",
        pending_backfills = pending_backfills.len(),
        "database backfills pending"
    );

    for (index, backfill) in pending_backfills.iter().enumerate() {
        let current = index + 1;
        let total = pending_backfills.len();
        info!(
            driver = "sqlite",
            current,
            total,
            version = backfill.version,
            description = %backfill.description,
            "applying database backfill"
        );

        let mut tx = conn.begin().await?;
        let started_at = std::time::Instant::now();
        sqlx::raw_sql(&backfill.sql).execute(&mut *tx).await?;
        let elapsed_ms = i64::try_from(started_at.elapsed().as_millis()).unwrap_or(i64::MAX);
        query(INSERT_APPLIED_BACKFILL_SQL)
            .bind(backfill.version)
            .bind(backfill.description.as_ref())
            .bind(backfill.checksum.as_ref())
            .bind(elapsed_ms)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;

        info!(
            driver = "sqlite",
            current,
            total,
            version = backfill.version,
            description = %backfill.description,
            elapsed_ms,
            "applied database backfill"
        );
    }

    info!(
        driver = "sqlite",
        pending_backfills = 0,
        "database backfills complete"
    );
    Ok(())
}

async fn pending_backfills_locked(
    conn: &mut SqliteConnection,
) -> Result<Vec<PendingBackfillInfo>, MigrateError> {
    ensure_schema_backfills_table(conn).await?;
    let applied_backfills = list_applied_backfills(conn).await?;
    validate_applied_backfills(&applied_backfills)?;
    Ok(pending_backfills_from_applied(&applied_backfills))
}

async fn ensure_schema_backfills_table(conn: &mut SqliteConnection) -> Result<(), MigrateError> {
    query(ENSURE_SCHEMA_BACKFILLS_TABLE_SQL)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

async fn list_applied_backfills(
    conn: &mut SqliteConnection,
) -> Result<Vec<AppliedBackfill>, MigrateError> {
    let rows = query(LIST_APPLIED_BACKFILLS_SQL)
        .fetch_all(&mut *conn)
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(AppliedBackfill {
                version: row.try_get("version")?,
                checksum: row.try_get("checksum")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(MigrateError::from)
}

fn validate_applied_backfills(applied_backfills: &[AppliedBackfill]) -> Result<(), MigrateError> {
    if BACKFILL_MIGRATOR.ignore_missing {
        return Ok(());
    }

    let known_versions: HashSet<_> = BACKFILL_MIGRATOR
        .iter()
        .map(|backfill| backfill.version)
        .collect();
    for applied_backfill in applied_backfills {
        if !known_versions.contains(&applied_backfill.version) {
            error!(
                driver = "sqlite",
                version = applied_backfill.version,
                "applied database backfill is missing from embedded backfills"
            );
            return Err(MigrateError::VersionMissing(applied_backfill.version));
        }
    }

    for backfill in BACKFILL_MIGRATOR
        .iter()
        .filter(|backfill| backfill.migration_type.is_up_migration())
    {
        let Some(applied) = applied_backfills
            .iter()
            .find(|applied| applied.version == backfill.version)
        else {
            continue;
        };
        if backfill.checksum != applied.checksum {
            warn!(
                driver = "sqlite",
                version = backfill.version,
                description = %backfill.description,
                "applied database backfill checksum differs from embedded backfill; skipping strict enforcement"
            );
        }
    }

    Ok(())
}

fn pending_backfills_from_applied(
    applied_backfills: &[AppliedBackfill],
) -> Vec<PendingBackfillInfo> {
    let applied_versions: HashSet<_> = applied_backfills
        .iter()
        .map(|backfill| backfill.version)
        .collect();
    BACKFILL_MIGRATOR
        .iter()
        .filter(|backfill| backfill.migration_type.is_up_migration())
        .filter(|backfill| !applied_versions.contains(&backfill.version))
        .map(|backfill| PendingBackfillInfo {
            version: backfill.version,
            description: backfill.description.to_string(),
        })
        .collect()
}
