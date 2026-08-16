//! Lite-only database migrations.
//!
//! These migrations deliberately do not use SQLx's `_sqlx_migrations` table. The core migration
//! chain must remain byte-for-byte compatible with the main edition while Lite-owned data evolves
//! in its own namespace and history.

use std::collections::{HashMap, HashSet};

use aether_data_contracts::PendingMigrationInfo;
use sha2::{Digest, Sha256};
use sqlx::{migrate::MigrateError, Row};

#[cfg(feature = "postgres")]
use sqlx::PgPool;
#[cfg(feature = "sqlite")]
use sqlx::SqlitePool;

#[derive(Debug, Clone, Copy)]
struct LiteMigration {
    version: i64,
    description: &'static str,
    sql: &'static str,
}

impl LiteMigration {
    fn checksum(self) -> Vec<u8> {
        Sha256::digest(self.sql.as_bytes()).to_vec()
    }

    fn pending_info(self) -> PendingMigrationInfo {
        PendingMigrationInfo {
            version: self.version,
            description: format!("lite: {}", self.description),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AppliedLiteMigration {
    version: i64,
    checksum: Vec<u8>,
}

#[cfg(feature = "postgres")]
const POSTGRES_MIGRATIONS: &[LiteMigration] = &[LiteMigration {
    version: 20260803000000,
    description: "daily usage limits",
    sql: include_str!("../../migrations/lite/postgres/20260803000000_daily_usage_limits.sql"),
}];

#[cfg(feature = "sqlite")]
const SQLITE_MIGRATIONS: &[LiteMigration] = &[LiteMigration {
    version: 20260803000000,
    description: "daily usage limits",
    sql: include_str!("../../migrations/lite/sqlite/20260803000000_daily_usage_limits.sql"),
}];

fn validate_applied(
    migrations: &[LiteMigration],
    applied: &[AppliedLiteMigration],
) -> Result<(), MigrateError> {
    let known_versions = migrations
        .iter()
        .map(|migration| migration.version)
        .collect::<HashSet<_>>();
    for migration in applied {
        if !known_versions.contains(&migration.version) {
            return Err(MigrateError::VersionMissing(migration.version));
        }
    }

    for migration in migrations {
        if let Some(applied) = applied
            .iter()
            .find(|applied| applied.version == migration.version)
        {
            if applied.checksum != migration.checksum() {
                return Err(MigrateError::VersionMismatch(migration.version));
            }
        }
    }
    Ok(())
}

fn pending_from_applied(
    migrations: &[LiteMigration],
    applied: &[AppliedLiteMigration],
) -> Vec<PendingMigrationInfo> {
    let applied_versions = applied
        .iter()
        .map(|migration| migration.version)
        .collect::<HashSet<_>>();
    migrations
        .iter()
        .copied()
        .filter(|migration| !applied_versions.contains(&migration.version))
        .map(LiteMigration::pending_info)
        .collect()
}

#[cfg(feature = "postgres")]
const POSTGRES_TABLE_EXISTS_SQL: &str =
    "SELECT to_regclass('aether_lite._aether_lite_migrations') IS NOT NULL";
#[cfg(feature = "postgres")]
const POSTGRES_ENSURE_TABLE_SQL: &str = r#"
CREATE SCHEMA IF NOT EXISTS aether_lite;
CREATE TABLE IF NOT EXISTS aether_lite._aether_lite_migrations (
    version bigint PRIMARY KEY,
    description text NOT NULL,
    checksum bytea NOT NULL,
    applied_at timestamp with time zone NOT NULL DEFAULT now()
)
"#;
#[cfg(feature = "postgres")]
const POSTGRES_LIST_APPLIED_SQL: &str = r#"
SELECT version, checksum
FROM aether_lite._aether_lite_migrations
ORDER BY version ASC
"#;

#[cfg(feature = "postgres")]
pub async fn pending_postgres_migrations(
    pool: &PgPool,
) -> Result<Vec<PendingMigrationInfo>, MigrateError> {
    let mut conn = pool.acquire().await?;
    let table_exists: bool = sqlx::query_scalar(POSTGRES_TABLE_EXISTS_SQL)
        .fetch_one(&mut *conn)
        .await?;
    if !table_exists {
        return Ok(POSTGRES_MIGRATIONS
            .iter()
            .copied()
            .map(LiteMigration::pending_info)
            .collect());
    }
    let applied = list_postgres_applied(&mut *conn).await?;
    validate_applied(POSTGRES_MIGRATIONS, &applied)?;
    Ok(pending_from_applied(POSTGRES_MIGRATIONS, &applied))
}

#[cfg(feature = "postgres")]
pub async fn run_postgres_migrations(pool: &PgPool) -> Result<(), MigrateError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(0x4145_5448_4c49_5445_i64)
        .execute(&mut *tx)
        .await?;
    sqlx::raw_sql(POSTGRES_ENSURE_TABLE_SQL)
        .execute(&mut *tx)
        .await?;

    let applied = list_postgres_applied(&mut *tx).await?;
    validate_applied(POSTGRES_MIGRATIONS, &applied)?;
    let applied_by_version = applied
        .iter()
        .map(|migration| (migration.version, migration))
        .collect::<HashMap<_, _>>();

    for migration in POSTGRES_MIGRATIONS {
        if applied_by_version.contains_key(&migration.version) {
            continue;
        }
        sqlx::raw_sql(migration.sql)
            .execute(&mut *tx)
            .await
            .map_err(|err| MigrateError::ExecuteMigration(err, migration.version))?;
        sqlx::query(
            r#"
INSERT INTO aether_lite._aether_lite_migrations (version, description, checksum)
VALUES ($1, $2, $3)
"#,
        )
        .bind(migration.version)
        .bind(migration.description)
        .bind(migration.checksum())
        .execute(&mut *tx)
        .await
        .map_err(|err| MigrateError::ExecuteMigration(err, migration.version))?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(feature = "postgres")]
async fn list_postgres_applied<'e, E>(
    executor: E,
) -> Result<Vec<AppliedLiteMigration>, MigrateError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows = sqlx::query(POSTGRES_LIST_APPLIED_SQL)
        .fetch_all(executor)
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(AppliedLiteMigration {
                version: row.try_get("version")?,
                checksum: row.try_get("checksum")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(MigrateError::Execute)
}

#[cfg(feature = "sqlite")]
const SQLITE_ENSURE_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS _aether_lite_migrations (
    version INTEGER PRIMARY KEY NOT NULL,
    description TEXT NOT NULL,
    checksum BLOB NOT NULL,
    applied_at INTEGER NOT NULL DEFAULT (CAST(strftime('%s', 'now') AS INTEGER))
)
"#;
#[cfg(feature = "sqlite")]
const SQLITE_LIST_APPLIED_SQL: &str = r#"
SELECT version, checksum
FROM _aether_lite_migrations
ORDER BY version ASC
"#;

#[cfg(feature = "sqlite")]
pub async fn pending_sqlite_migrations(
    pool: &SqlitePool,
) -> Result<Vec<PendingMigrationInfo>, MigrateError> {
    let mut conn = pool.acquire().await?;
    let table_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_aether_lite_migrations')",
    )
    .fetch_one(&mut *conn)
    .await?;
    if !table_exists {
        return Ok(SQLITE_MIGRATIONS
            .iter()
            .copied()
            .map(LiteMigration::pending_info)
            .collect());
    }
    let applied = list_sqlite_applied(&mut *conn).await?;
    validate_applied(SQLITE_MIGRATIONS, &applied)?;
    Ok(pending_from_applied(SQLITE_MIGRATIONS, &applied))
}

#[cfg(feature = "sqlite")]
pub async fn run_sqlite_migrations(pool: &SqlitePool) -> Result<(), MigrateError> {
    let mut tx = pool.begin().await?;
    sqlx::query(SQLITE_ENSURE_TABLE_SQL)
        .execute(&mut *tx)
        .await?;
    let applied = list_sqlite_applied(&mut *tx).await?;
    validate_applied(SQLITE_MIGRATIONS, &applied)?;
    let applied_by_version = applied
        .iter()
        .map(|migration| (migration.version, migration))
        .collect::<HashMap<_, _>>();

    for migration in SQLITE_MIGRATIONS {
        if applied_by_version.contains_key(&migration.version) {
            continue;
        }
        sqlx::raw_sql(migration.sql)
            .execute(&mut *tx)
            .await
            .map_err(|err| MigrateError::ExecuteMigration(err, migration.version))?;
        sqlx::query(
            r#"
INSERT INTO _aether_lite_migrations (version, description, checksum)
VALUES (?, ?, ?)
"#,
        )
        .bind(migration.version)
        .bind(migration.description)
        .bind(migration.checksum())
        .execute(&mut *tx)
        .await
        .map_err(|err| MigrateError::ExecuteMigration(err, migration.version))?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(feature = "sqlite")]
async fn list_sqlite_applied<'e, E>(executor: E) -> Result<Vec<AppliedLiteMigration>, MigrateError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let rows = sqlx::query(SQLITE_LIST_APPLIED_SQL)
        .fetch_all(executor)
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(AppliedLiteMigration {
                version: row.try_get("version")?,
                checksum: row.try_get("checksum")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(MigrateError::Execute)
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::{pending_sqlite_migrations, run_sqlite_migrations};
    use sqlx::migrate::MigrateError;

    #[tokio::test]
    async fn sqlite_lite_migrations_have_an_independent_checked_history() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        sqlx::query("CREATE TABLE user_groups (id TEXT PRIMARY KEY NOT NULL)")
            .execute(&pool)
            .await
            .expect("core user_groups table");

        assert_eq!(
            pending_sqlite_migrations(&pool)
                .await
                .expect("initial pending migrations")
                .len(),
            1
        );
        run_sqlite_migrations(&pool)
            .await
            .expect("run Lite migrations");
        assert!(pending_sqlite_migrations(&pool)
            .await
            .expect("pending after migration")
            .is_empty());

        let table_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name LIKE 'lite_%_daily_usage_limits'",
        )
        .fetch_one(&pool)
        .await
        .expect("Lite tables");
        assert_eq!(table_count, 2);

        sqlx::query("UPDATE _aether_lite_migrations SET checksum = X'00'")
            .execute(&pool)
            .await
            .expect("corrupt checksum");
        let error = pending_sqlite_migrations(&pool)
            .await
            .expect_err("checksum mismatch");
        assert!(matches!(
            error,
            MigrateError::VersionMismatch(20260803000000)
        ));
    }

    #[tokio::test]
    async fn sqlite_lite_migrations_reject_unknown_applied_versions() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite pool");
        sqlx::query("CREATE TABLE user_groups (id TEXT PRIMARY KEY NOT NULL)")
            .execute(&pool)
            .await
            .expect("core user_groups table");
        run_sqlite_migrations(&pool)
            .await
            .expect("run Lite migrations");
        sqlx::query(
            "INSERT INTO _aether_lite_migrations (version, description, checksum) VALUES (?, ?, ?)",
        )
        .bind(20260803000001_i64)
        .bind("unknown")
        .bind(vec![0_u8; 32])
        .execute(&pool)
        .await
        .expect("insert unknown version");

        let error = pending_sqlite_migrations(&pool)
            .await
            .expect_err("unknown version");
        assert!(matches!(
            error,
            MigrateError::VersionMissing(20260803000001)
        ));
    }
}
