use async_trait::async_trait;
use sqlx::{sqlite::SqliteRow, QueryBuilder, Row, Sqlite};

use aether_data_contracts::repository::wallet::{
    AdjustWalletBalanceInput, AdminWalletListQuery, StoredAdminWalletListItem,
    StoredAdminWalletListPage, StoredWalletSnapshot, WalletLookupKey, WalletReadRepository,
    WalletWriteRepository,
};
use aether_data_contracts::DataLayerError;

use crate::error::SqlResultExt;
use crate::{sqlite_real, SqlitePool};

#[derive(Debug, Clone)]
pub struct SqliteWalletReadRepository {
    pool: SqlitePool,
}

impl SqliteWalletReadRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WalletReadRepository for SqliteWalletReadRepository {
    async fn find(
        &self,
        key: WalletLookupKey<'_>,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        let (column, value) = match key {
            WalletLookupKey::WalletId(value) => ("id", value),
            WalletLookupKey::UserId(value) => ("user_id", value),
            WalletLookupKey::ApiKeyId(value) => ("api_key_id", value),
        };
        let row = sqlx::query(&format!(
            "{} WHERE {column} = ? LIMIT 1",
            wallet_select_sql()
        ))
        .bind(value)
        .fetch_optional(&self.pool)
        .await
        .map_sql_err()?;
        row.as_ref().map(map_wallet_row).transpose()
    }

    async fn update_auth_user_wallet_limit_mode(
        &self,
        user_id: &str,
        limit_mode: &str,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        self.update_limit_mode("user_id", user_id, limit_mode)
            .await?;
        self.find(WalletLookupKey::UserId(user_id)).await
    }

    async fn update_auth_api_key_wallet_limit_mode(
        &self,
        api_key_id: &str,
        limit_mode: &str,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        self.update_limit_mode("api_key_id", api_key_id, limit_mode)
            .await?;
        self.find(WalletLookupKey::ApiKeyId(api_key_id)).await
    }

    async fn initialize_auth_user_wallet(
        &self,
        user_id: &str,
        initial_balance_usd: f64,
        unlimited: bool,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        initialize_wallet(
            &self.pool,
            Some(user_id),
            None,
            initial_balance_usd,
            unlimited,
        )
        .await
    }

    async fn initialize_auth_api_key_wallet(
        &self,
        api_key_id: &str,
        initial_balance_usd: f64,
        unlimited: bool,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        initialize_wallet(
            &self.pool,
            None,
            Some(api_key_id),
            initial_balance_usd,
            unlimited,
        )
        .await
    }

    async fn update_auth_user_wallet_snapshot(
        &self,
        user_id: &str,
        balance: f64,
        limit_mode: &str,
        currency: &str,
        status: &str,
        total_consumed: f64,
        updated_at_unix_secs: Option<u64>,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        update_wallet_snapshot(
            &self.pool,
            "user_id",
            user_id,
            balance,
            limit_mode,
            currency,
            status,
            total_consumed,
            updated_at_unix_secs,
        )
        .await?;
        self.find(WalletLookupKey::UserId(user_id)).await
    }

    async fn update_auth_api_key_wallet_snapshot(
        &self,
        api_key_id: &str,
        balance: f64,
        limit_mode: &str,
        currency: &str,
        status: &str,
        total_consumed: f64,
        updated_at_unix_secs: Option<u64>,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        update_wallet_snapshot(
            &self.pool,
            "api_key_id",
            api_key_id,
            balance,
            limit_mode,
            currency,
            status,
            total_consumed,
            updated_at_unix_secs,
        )
        .await?;
        self.find(WalletLookupKey::ApiKeyId(api_key_id)).await
    }

    async fn list_wallets_by_user_ids(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<StoredWalletSnapshot>, DataLayerError> {
        list_wallets(&self.pool, "user_id", user_ids).await
    }

    async fn list_wallets_by_api_key_ids(
        &self,
        api_key_ids: &[String],
    ) -> Result<Vec<StoredWalletSnapshot>, DataLayerError> {
        list_wallets(&self.pool, "api_key_id", api_key_ids).await
    }

    async fn list_admin_wallets(
        &self,
        query: &AdminWalletListQuery,
    ) -> Result<StoredAdminWalletListPage, DataLayerError> {
        let total = sqlx::query(
            r#"
SELECT COUNT(*) AS total
FROM wallets
WHERE (? IS NULL OR status = ?)
  AND (
    ? IS NULL
    OR (? = 'user' AND user_id IS NOT NULL)
    OR (? = 'api_key' AND api_key_id IS NOT NULL)
  )
"#,
        )
        .bind(query.status.as_deref())
        .bind(query.status.as_deref())
        .bind(query.owner_type.as_deref())
        .bind(query.owner_type.as_deref())
        .bind(query.owner_type.as_deref())
        .fetch_one(&self.pool)
        .await
        .map_sql_err()?
        .try_get::<i64, _>("total")
        .map_sql_err()?
        .max(0) as u64;

        let rows = sqlx::query(
            r#"
SELECT
  w.id, w.user_id, w.api_key_id, w.balance, w.limit_mode, w.currency, w.status,
  w.total_consumed, users.username AS user_name, api_keys.name AS api_key_name,
  w.created_at AS created_at_unix_ms, w.updated_at AS updated_at_unix_secs
FROM wallets w
LEFT JOIN users ON users.id = w.user_id
LEFT JOIN api_keys ON api_keys.id = w.api_key_id
WHERE (? IS NULL OR w.status = ?)
  AND (
    ? IS NULL
    OR (? = 'user' AND w.user_id IS NOT NULL)
    OR (? = 'api_key' AND w.api_key_id IS NOT NULL)
  )
ORDER BY w.updated_at DESC
LIMIT ? OFFSET ?
"#,
        )
        .bind(query.status.as_deref())
        .bind(query.status.as_deref())
        .bind(query.owner_type.as_deref())
        .bind(query.owner_type.as_deref())
        .bind(query.owner_type.as_deref())
        .bind(i64_from_usize(query.limit)?)
        .bind(i64_from_usize(query.offset)?)
        .fetch_all(&self.pool)
        .await
        .map_sql_err()?;
        let items = rows
            .iter()
            .map(map_admin_wallet_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(StoredAdminWalletListPage { items, total })
    }
}

impl SqliteWalletReadRepository {
    async fn update_limit_mode(
        &self,
        column: &str,
        owner_id: &str,
        limit_mode: &str,
    ) -> Result<(), DataLayerError> {
        sqlx::query(&format!(
            "UPDATE wallets SET limit_mode = ?, updated_at = ? WHERE {column} = ?"
        ))
        .bind(limit_mode)
        .bind(current_unix_secs_i64())
        .bind(owner_id)
        .execute(&self.pool)
        .await
        .map_sql_err()?;
        Ok(())
    }
}

#[async_trait]
impl WalletWriteRepository for SqliteWalletReadRepository {
    async fn adjust_wallet_balance(
        &self,
        input: AdjustWalletBalanceInput,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        if !input.amount_usd.is_finite() || input.amount_usd == 0.0 {
            return Err(DataLayerError::InvalidInput(
                "wallet adjustment must be a finite non-zero value".to_string(),
            ));
        }
        let result =
            sqlx::query("UPDATE wallets SET balance = balance + ?, updated_at = ? WHERE id = ?")
                .bind(input.amount_usd)
                .bind(current_unix_secs_i64())
                .bind(&input.wallet_id)
                .execute(&self.pool)
                .await
                .map_sql_err()?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.find(WalletLookupKey::WalletId(&input.wallet_id)).await
    }
}

async fn initialize_wallet(
    pool: &SqlitePool,
    user_id: Option<&str>,
    api_key_id: Option<&str>,
    initial_balance_usd: f64,
    unlimited: bool,
) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
    if !initial_balance_usd.is_finite() {
        return Err(DataLayerError::InvalidInput(
            "initial wallet balance must be finite".to_string(),
        ));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = current_unix_secs_i64();
    sqlx::query(
        r#"
INSERT OR IGNORE INTO wallets (
  id, user_id, api_key_id, balance, limit_mode, currency, status, created_at, updated_at
) VALUES (?, ?, ?, ?, ?, 'USD', 'active', ?, ?)
"#,
    )
    .bind(&id)
    .bind(user_id)
    .bind(api_key_id)
    .bind(initial_balance_usd)
    .bind(if unlimited { "unlimited" } else { "finite" })
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .map_sql_err()?;
    let repository = SqliteWalletReadRepository::new(pool.clone());
    match (user_id, api_key_id) {
        (Some(id), _) => repository.find(WalletLookupKey::UserId(id)).await,
        (_, Some(id)) => repository.find(WalletLookupKey::ApiKeyId(id)).await,
        _ => Ok(None),
    }
}

#[allow(clippy::too_many_arguments)]
async fn update_wallet_snapshot(
    pool: &SqlitePool,
    column: &str,
    owner_id: &str,
    balance: f64,
    limit_mode: &str,
    currency: &str,
    status: &str,
    total_consumed: f64,
    updated_at_unix_secs: Option<u64>,
) -> Result<(), DataLayerError> {
    let updated_at = updated_at_unix_secs
        .map(i64::try_from)
        .transpose()
        .map_err(|_| DataLayerError::InvalidInput("wallet timestamp is too large".to_string()))?
        .unwrap_or_else(current_unix_secs_i64);
    sqlx::query(&format!(
        r#"
UPDATE wallets
SET balance = ?, limit_mode = ?, currency = ?, status = ?, total_consumed = ?, updated_at = ?
WHERE {column} = ?
"#
    ))
    .bind(balance)
    .bind(limit_mode)
    .bind(currency)
    .bind(status)
    .bind(total_consumed)
    .bind(updated_at)
    .bind(owner_id)
    .execute(pool)
    .await
    .map_sql_err()?;
    Ok(())
}

async fn list_wallets(
    pool: &SqlitePool,
    column: &str,
    ids: &[String],
) -> Result<Vec<StoredWalletSnapshot>, DataLayerError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut builder =
        QueryBuilder::<Sqlite>::new(format!("{} WHERE {column} IN (", wallet_select_sql()));
    let mut separated = builder.separated(", ");
    for id in ids {
        separated.push_bind(id);
    }
    separated.push_unseparated(")");
    let rows = builder.build().fetch_all(pool).await.map_sql_err()?;
    rows.iter().map(map_wallet_row).collect()
}

fn wallet_select_sql() -> &'static str {
    r#"
SELECT
  id, user_id, api_key_id, balance, limit_mode, currency, status,
  total_consumed, updated_at AS updated_at_unix_secs
FROM wallets
"#
}

fn map_wallet_row(row: &SqliteRow) -> Result<StoredWalletSnapshot, DataLayerError> {
    StoredWalletSnapshot::new(
        row.try_get("id").map_sql_err()?,
        row.try_get("user_id").map_sql_err()?,
        row.try_get("api_key_id").map_sql_err()?,
        sqlite_real(row, "balance")?,
        row.try_get("limit_mode").map_sql_err()?,
        row.try_get("currency").map_sql_err()?,
        row.try_get("status").map_sql_err()?,
        sqlite_real(row, "total_consumed")?,
        row.try_get("updated_at_unix_secs").map_sql_err()?,
    )
}

fn map_admin_wallet_row(row: &SqliteRow) -> Result<StoredAdminWalletListItem, DataLayerError> {
    Ok(StoredAdminWalletListItem {
        id: row.try_get("id").map_sql_err()?,
        user_id: row.try_get("user_id").map_sql_err()?,
        api_key_id: row.try_get("api_key_id").map_sql_err()?,
        balance: sqlite_real(row, "balance")?,
        limit_mode: row.try_get("limit_mode").map_sql_err()?,
        currency: row.try_get("currency").map_sql_err()?,
        status: row.try_get("status").map_sql_err()?,
        total_consumed: sqlite_real(row, "total_consumed")?,
        user_name: row.try_get("user_name").map_sql_err()?,
        api_key_name: row.try_get("api_key_name").map_sql_err()?,
        created_at_unix_ms: optional_u64(row, "created_at_unix_ms")?,
        updated_at_unix_secs: optional_u64(row, "updated_at_unix_secs")?,
    })
}

fn optional_u64(row: &SqliteRow, column: &str) -> Result<Option<u64>, DataLayerError> {
    row.try_get::<Option<i64>, _>(column)
        .map_sql_err()?
        .map(|value| {
            u64::try_from(value)
                .map_err(|_| DataLayerError::UnexpectedValue(format!("{column} is negative")))
        })
        .transpose()
}

fn current_unix_secs_i64() -> i64 {
    chrono::Utc::now().timestamp().max(0)
}

fn i64_from_usize(value: usize) -> Result<i64, DataLayerError> {
    i64::try_from(value)
        .map_err(|_| DataLayerError::InvalidInput("wallet page value is too large".to_string()))
}
