use async_trait::async_trait;
use sqlx::{postgres::PgRow, PgPool, Row};

use aether_data_contracts::repository::wallet::{
    AdjustWalletBalanceInput, AdminWalletListQuery, StoredAdminWalletListItem,
    StoredAdminWalletListPage, StoredWalletSnapshot, WalletLookupKey, WalletReadRepository,
    WalletWriteRepository,
};
use aether_data_contracts::DataLayerError;

use crate::error::SqlxResultExt;

const WALLET_COLUMNS: &str = r#"
SELECT
  id,
  user_id,
  api_key_id,
  CAST(balance AS DOUBLE PRECISION) AS balance,
  limit_mode,
  currency,
  status,
  CAST(total_consumed AS DOUBLE PRECISION) AS total_consumed,
  CAST(EXTRACT(EPOCH FROM updated_at) AS BIGINT) AS updated_at_unix_secs
FROM wallets
"#;

#[derive(Debug, Clone)]
pub struct SqlxWalletRepository {
    pool: PgPool,
}

impl SqlxWalletRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WalletReadRepository for SqlxWalletRepository {
    async fn find(
        &self,
        key: WalletLookupKey<'_>,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        let (column, value) = match key {
            WalletLookupKey::WalletId(value) => ("id", value),
            WalletLookupKey::UserId(value) => ("user_id", value),
            WalletLookupKey::ApiKeyId(value) => ("api_key_id", value),
        };
        let row = sqlx::query(&format!("{WALLET_COLUMNS} WHERE {column} = $1 LIMIT 1"))
            .bind(value)
            .fetch_optional(&self.pool)
            .await
            .map_postgres_err()?;
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
WHERE ($1::TEXT IS NULL OR status = $1)
  AND (
    $2::TEXT IS NULL
    OR ($2 = 'user' AND user_id IS NOT NULL)
    OR ($2 = 'api_key' AND api_key_id IS NOT NULL)
  )
"#,
        )
        .bind(query.status.as_deref())
        .bind(query.owner_type.as_deref())
        .fetch_one(&self.pool)
        .await
        .map_postgres_err()?
        .try_get::<i64, _>("total")
        .map_postgres_err()?
        .max(0) as u64;

        let rows = sqlx::query(
            r#"
SELECT
  w.id,
  w.user_id,
  w.api_key_id,
  CAST(w.balance AS DOUBLE PRECISION) AS balance,
  w.limit_mode,
  w.currency,
  w.status,
  CAST(w.total_consumed AS DOUBLE PRECISION) AS total_consumed,
  users.username AS user_name,
  api_keys.name AS api_key_name,
  CAST(EXTRACT(EPOCH FROM w.created_at) AS BIGINT) AS created_at_unix_ms,
  CAST(EXTRACT(EPOCH FROM w.updated_at) AS BIGINT) AS updated_at_unix_secs
FROM wallets w
LEFT JOIN users ON users.id = w.user_id
LEFT JOIN api_keys ON api_keys.id = w.api_key_id
WHERE ($1::TEXT IS NULL OR w.status = $1)
  AND (
    $2::TEXT IS NULL
    OR ($2 = 'user' AND w.user_id IS NOT NULL)
    OR ($2 = 'api_key' AND w.api_key_id IS NOT NULL)
  )
ORDER BY w.updated_at DESC
OFFSET $3
LIMIT $4
"#,
        )
        .bind(query.status.as_deref())
        .bind(query.owner_type.as_deref())
        .bind(i64_from_usize(query.offset)?)
        .bind(i64_from_usize(query.limit)?)
        .fetch_all(&self.pool)
        .await
        .map_postgres_err()?;
        let items = rows
            .iter()
            .map(map_admin_wallet_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(StoredAdminWalletListPage { items, total })
    }
}

impl SqlxWalletRepository {
    async fn update_limit_mode(
        &self,
        column: &str,
        owner_id: &str,
        limit_mode: &str,
    ) -> Result<(), DataLayerError> {
        sqlx::query(&format!(
            "UPDATE wallets SET limit_mode = $2, updated_at = NOW() WHERE {column} = $1"
        ))
        .bind(owner_id)
        .bind(limit_mode)
        .execute(&self.pool)
        .await
        .map_postgres_err()?;
        Ok(())
    }
}

#[async_trait]
impl WalletWriteRepository for SqlxWalletRepository {
    async fn adjust_wallet_balance(
        &self,
        input: AdjustWalletBalanceInput,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        if !input.amount_usd.is_finite() || input.amount_usd == 0.0 {
            return Err(DataLayerError::InvalidInput(
                "wallet adjustment must be a finite non-zero value".to_string(),
            ));
        }
        let result = sqlx::query(
            "UPDATE wallets SET balance = balance + $2, updated_at = NOW() WHERE id = $1",
        )
        .bind(&input.wallet_id)
        .bind(input.amount_usd)
        .execute(&self.pool)
        .await
        .map_postgres_err()?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.find(WalletLookupKey::WalletId(&input.wallet_id)).await
    }
}

async fn initialize_wallet(
    pool: &PgPool,
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
    sqlx::query(
        r#"
INSERT INTO wallets (
  id, user_id, api_key_id, balance, limit_mode, currency, status, created_at, updated_at
) VALUES ($1, $2, $3, $4, $5, 'USD', 'active', NOW(), NOW())
ON CONFLICT DO NOTHING
"#,
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(user_id)
    .bind(api_key_id)
    .bind(initial_balance_usd)
    .bind(if unlimited { "unlimited" } else { "finite" })
    .execute(pool)
    .await
    .map_postgres_err()?;
    let repository = SqlxWalletRepository::new(pool.clone());
    match (user_id, api_key_id) {
        (Some(id), _) => repository.find(WalletLookupKey::UserId(id)).await,
        (_, Some(id)) => repository.find(WalletLookupKey::ApiKeyId(id)).await,
        _ => Ok(None),
    }
}

#[allow(clippy::too_many_arguments)]
async fn update_wallet_snapshot(
    pool: &PgPool,
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
        .map_err(|_| DataLayerError::InvalidInput("wallet timestamp is too large".to_string()))?;
    sqlx::query(&format!(
        r#"
UPDATE wallets
SET balance = $2,
    limit_mode = $3,
    currency = $4,
    status = $5,
    total_consumed = $6,
    updated_at = COALESCE(to_timestamp($7), NOW())
WHERE {column} = $1
"#
    ))
    .bind(owner_id)
    .bind(balance)
    .bind(limit_mode)
    .bind(currency)
    .bind(status)
    .bind(total_consumed)
    .bind(updated_at)
    .execute(pool)
    .await
    .map_postgres_err()?;
    Ok(())
}

async fn list_wallets(
    pool: &PgPool,
    column: &str,
    ids: &[String],
) -> Result<Vec<StoredWalletSnapshot>, DataLayerError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(&format!("{WALLET_COLUMNS} WHERE {column} = ANY($1)"))
        .bind(ids)
        .fetch_all(pool)
        .await
        .map_postgres_err()?;
    rows.iter().map(map_wallet_row).collect()
}

fn map_wallet_row(row: &PgRow) -> Result<StoredWalletSnapshot, DataLayerError> {
    StoredWalletSnapshot::new(
        row.try_get("id").map_postgres_err()?,
        row.try_get("user_id").map_postgres_err()?,
        row.try_get("api_key_id").map_postgres_err()?,
        row.try_get("balance").map_postgres_err()?,
        row.try_get("limit_mode").map_postgres_err()?,
        row.try_get("currency").map_postgres_err()?,
        row.try_get("status").map_postgres_err()?,
        row.try_get("total_consumed").map_postgres_err()?,
        row.try_get("updated_at_unix_secs").map_postgres_err()?,
    )
}

fn map_admin_wallet_row(row: &PgRow) -> Result<StoredAdminWalletListItem, DataLayerError> {
    Ok(StoredAdminWalletListItem {
        id: row.try_get("id").map_postgres_err()?,
        user_id: row.try_get("user_id").map_postgres_err()?,
        api_key_id: row.try_get("api_key_id").map_postgres_err()?,
        balance: row.try_get("balance").map_postgres_err()?,
        limit_mode: row.try_get("limit_mode").map_postgres_err()?,
        currency: row.try_get("currency").map_postgres_err()?,
        status: row.try_get("status").map_postgres_err()?,
        total_consumed: row.try_get("total_consumed").map_postgres_err()?,
        user_name: row.try_get("user_name").map_postgres_err()?,
        api_key_name: row.try_get("api_key_name").map_postgres_err()?,
        created_at_unix_ms: optional_u64(row, "created_at_unix_ms")?,
        updated_at_unix_secs: optional_u64(row, "updated_at_unix_secs")?,
    })
}

fn optional_u64(row: &PgRow, column: &str) -> Result<Option<u64>, DataLayerError> {
    row.try_get::<Option<i64>, _>(column)
        .map_postgres_err()?
        .map(|value| {
            u64::try_from(value)
                .map_err(|_| DataLayerError::UnexpectedValue(format!("{column} is negative")))
        })
        .transpose()
}

fn i64_from_usize(value: usize) -> Result<i64, DataLayerError> {
    i64::try_from(value)
        .map_err(|_| DataLayerError::InvalidInput("wallet page value is too large".to_string()))
}
