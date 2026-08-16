use async_trait::async_trait;
use sqlx::{sqlite::SqliteRow, Row};

use aether_data_contracts::repository::settlement::{
    settlement_billable_cost_usd, settlement_billing_status_for_usage_status,
    SettlementWriteRepository, StoredUsageSettlement, UsageSettlementInput, SETTLEMENT_EPSILON_USD,
};
use aether_data_contracts::DataLayerError;

use crate::error::SqlResultExt;
use crate::{sqlite_optional_real, sqlite_real, SqlitePool};

#[derive(Debug, Clone)]
pub struct SqliteSettlementRepository {
    pool: SqlitePool,
}

impl SqliteSettlementRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SettlementWriteRepository for SqliteSettlementRepository {
    async fn settle_usage(
        &self,
        input: UsageSettlementInput,
    ) -> Result<Option<StoredUsageSettlement>, DataLayerError> {
        input.validate()?;
        if let Some(existing) = find_settlement(&self.pool, &input.request_id).await? {
            return Ok(Some(existing));
        }

        let mut tx = self.pool.begin().await.map_sql_err()?;
        let billable_cost_usd = settlement_billable_cost_usd(&input);
        let mut billing_status =
            settlement_billing_status_for_usage_status(&input.status).to_string();

        let wallet_row = if let Some(api_key_id) = input.api_key_id.as_deref() {
            sqlx::query("SELECT id, balance, limit_mode FROM wallets WHERE api_key_id = ? LIMIT 1")
                .bind(api_key_id)
                .fetch_optional(&mut *tx)
                .await
                .map_sql_err()?
        } else {
            None
        };
        let wallet_row = if wallet_row.is_none() && !input.api_key_is_standalone {
            if let Some(user_id) = input.user_id.as_deref() {
                sqlx::query("SELECT id, balance, limit_mode FROM wallets WHERE user_id = ? LIMIT 1")
                    .bind(user_id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_sql_err()?
            } else {
                None
            }
        } else {
            wallet_row
        };

        let mut settlement = StoredUsageSettlement {
            request_id: input.request_id.clone(),
            wallet_id: None,
            billing_status: billing_status.clone(),
            wallet_balance_before: None,
            wallet_balance_after: None,
            finalized_at_unix_secs: input.finalized_at_unix_secs,
        };

        if let Some(row) = wallet_row.as_ref() {
            let wallet_id: String = row.try_get("id").map_sql_err()?;
            let balance = sqlite_real(row, "balance")?;
            let limit_mode: String = row.try_get("limit_mode").map_sql_err()?;
            settlement.wallet_id = Some(wallet_id.clone());
            settlement.wallet_balance_before = Some(balance);
            settlement.wallet_balance_after = Some(balance);
            if billing_status == "settled" {
                let next_balance = if limit_mode.eq_ignore_ascii_case("unlimited") {
                    balance
                } else {
                    balance - billable_cost_usd
                };
                sqlx::query(
                    r#"
UPDATE wallets
SET balance = ?, total_consumed = total_consumed + ?, updated_at = ?
WHERE id = ?
"#,
                )
                .bind(next_balance)
                .bind(billable_cost_usd)
                .bind(current_unix_secs_i64())
                .bind(&wallet_id)
                .execute(&mut *tx)
                .await
                .map_sql_err()?;
                settlement.wallet_balance_after = Some(next_balance);
            }
        } else if billing_status == "settled" && billable_cost_usd > SETTLEMENT_EPSILON_USD {
            billing_status = "insufficient_quota".to_string();
            settlement.billing_status = billing_status.clone();
        }

        let finalized_at = input
            .finalized_at_unix_secs
            .map(i64::try_from)
            .transpose()
            .map_err(|_| {
                DataLayerError::InvalidInput("settlement timestamp is too large".to_string())
            })?;
        let updated = sqlx::query(
            r#"
UPDATE "usage"
SET billing_status = ?, wallet_balance_before = ?, wallet_balance_after = ?,
    finalized_at = ?, updated_at_unix_secs = ?
WHERE request_id = ?
"#,
        )
        .bind(&billing_status)
        .bind(settlement.wallet_balance_before)
        .bind(settlement.wallet_balance_after)
        .bind(finalized_at)
        .bind(current_unix_secs_i64())
        .bind(&input.request_id)
        .execute(&mut *tx)
        .await
        .map_sql_err()?;
        if updated.rows_affected() == 0 {
            tx.rollback().await.map_sql_err()?;
            return Ok(None);
        }

        sqlx::query(
            r#"
INSERT INTO usage_settlement_snapshots (
  request_id, billing_status, wallet_id, wallet_balance_before, wallet_balance_after,
  finalized_at, created_at, updated_at
) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(request_id) DO NOTHING
"#,
        )
        .bind(&input.request_id)
        .bind(&billing_status)
        .bind(settlement.wallet_id.as_deref())
        .bind(settlement.wallet_balance_before)
        .bind(settlement.wallet_balance_after)
        .bind(finalized_at)
        .bind(current_unix_secs_i64())
        .bind(current_unix_secs_i64())
        .execute(&mut *tx)
        .await
        .map_sql_err()?;
        tx.commit().await.map_sql_err()?;
        settlement.billing_status = billing_status;
        Ok(Some(settlement))
    }
}

async fn find_settlement(
    pool: &SqlitePool,
    request_id: &str,
) -> Result<Option<StoredUsageSettlement>, DataLayerError> {
    let row = sqlx::query(
        r#"
SELECT request_id, billing_status, wallet_id,
       CAST(wallet_balance_before AS REAL) AS wallet_balance_before,
       CAST(wallet_balance_after AS REAL) AS wallet_balance_after,
       finalized_at AS finalized_at_unix_secs
FROM usage_settlement_snapshots
WHERE request_id = ?
LIMIT 1
"#,
    )
    .bind(request_id)
    .fetch_optional(pool)
    .await
    .map_sql_err()?;
    row.as_ref().map(map_settlement).transpose()
}

fn map_settlement(row: &SqliteRow) -> Result<StoredUsageSettlement, DataLayerError> {
    Ok(StoredUsageSettlement {
        request_id: row.try_get("request_id").map_sql_err()?,
        wallet_id: row.try_get("wallet_id").map_sql_err()?,
        billing_status: row.try_get("billing_status").map_sql_err()?,
        wallet_balance_before: sqlite_optional_real(row, "wallet_balance_before")?,
        wallet_balance_after: sqlite_optional_real(row, "wallet_balance_after")?,
        finalized_at_unix_secs: row
            .try_get::<Option<i64>, _>("finalized_at_unix_secs")
            .map_sql_err()?
            .map(|value| {
                u64::try_from(value).map_err(|_| {
                    DataLayerError::UnexpectedValue("settlement timestamp is negative".to_string())
                })
            })
            .transpose()?,
    })
}

fn current_unix_secs_i64() -> i64 {
    chrono::Utc::now().timestamp().max(0)
}
