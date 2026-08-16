use async_trait::async_trait;
use sqlx::{postgres::PgRow, PgPool, Row};

use aether_data_contracts::repository::settlement::{
    settlement_billable_cost_usd, settlement_billing_status_for_usage_status,
    SettlementWriteRepository, StoredUsageSettlement, UsageSettlementInput, SETTLEMENT_EPSILON_USD,
};
use aether_data_contracts::DataLayerError;

use crate::error::SqlxResultExt;

#[derive(Debug, Clone)]
pub struct SqlxSettlementRepository {
    pool: PgPool,
}

impl SqlxSettlementRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SettlementWriteRepository for SqlxSettlementRepository {
    async fn settle_usage(
        &self,
        input: UsageSettlementInput,
    ) -> Result<Option<StoredUsageSettlement>, DataLayerError> {
        input.validate()?;
        let mut tx = self.pool.begin().await.map_postgres_err()?;

        let usage_row = sqlx::query(
            r#"
SELECT billing_status
FROM "usage"
WHERE request_id = $1
FOR UPDATE
"#,
        )
        .bind(&input.request_id)
        .fetch_optional(&mut *tx)
        .await
        .map_postgres_err()?;
        let Some(usage_row) = usage_row else {
            tx.rollback().await.map_postgres_err()?;
            return Ok(None);
        };
        let current_status: String = usage_row.try_get("billing_status").map_postgres_err()?;
        if current_status != "pending" {
            let existing = find_settlement_in_transaction(&mut tx, &input.request_id).await?;
            tx.commit().await.map_postgres_err()?;
            return Ok(existing);
        }

        let billable_cost_usd = settlement_billable_cost_usd(&input);
        let mut billing_status =
            settlement_billing_status_for_usage_status(&input.status).to_string();
        let wallet_row = if let Some(api_key_id) = input.api_key_id.as_deref() {
            sqlx::query(
                r#"
SELECT id, CAST(balance AS DOUBLE PRECISION) AS balance, limit_mode
FROM wallets
WHERE api_key_id = $1
LIMIT 1
FOR UPDATE
"#,
            )
            .bind(api_key_id)
            .fetch_optional(&mut *tx)
            .await
            .map_postgres_err()?
        } else {
            None
        };
        let wallet_row = if wallet_row.is_none() && !input.api_key_is_standalone {
            if let Some(user_id) = input.user_id.as_deref() {
                sqlx::query(
                    r#"
SELECT id, CAST(balance AS DOUBLE PRECISION) AS balance, limit_mode
FROM wallets
WHERE user_id = $1
LIMIT 1
FOR UPDATE
"#,
                )
                .bind(user_id)
                .fetch_optional(&mut *tx)
                .await
                .map_postgres_err()?
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
            let wallet_id: String = row.try_get("id").map_postgres_err()?;
            let balance: f64 = row.try_get("balance").map_postgres_err()?;
            let limit_mode: String = row.try_get("limit_mode").map_postgres_err()?;
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
SET balance = $2, total_consumed = total_consumed + $3, updated_at = NOW()
WHERE id = $1
"#,
                )
                .bind(&wallet_id)
                .bind(next_balance)
                .bind(billable_cost_usd)
                .execute(&mut *tx)
                .await
                .map_postgres_err()?;
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
        sqlx::query(
            r#"
UPDATE "usage"
SET billing_status = $2,
    wallet_balance_before = $3,
    wallet_balance_after = $4,
    finalized_at = CASE WHEN $5::BIGINT IS NULL THEN finalized_at ELSE to_timestamp($5) END
WHERE request_id = $1
"#,
        )
        .bind(&input.request_id)
        .bind(&billing_status)
        .bind(settlement.wallet_balance_before)
        .bind(settlement.wallet_balance_after)
        .bind(finalized_at)
        .execute(&mut *tx)
        .await
        .map_postgres_err()?;

        sqlx::query(
            r#"
INSERT INTO usage_settlement_snapshots (
  request_id, billing_status, wallet_id, wallet_balance_before, wallet_balance_after, finalized_at
) VALUES (
  $1, $2, $3, $4, $5,
  CASE WHEN $6::BIGINT IS NULL THEN NULL ELSE to_timestamp($6) END
)
ON CONFLICT (request_id) DO NOTHING
"#,
        )
        .bind(&input.request_id)
        .bind(&billing_status)
        .bind(settlement.wallet_id.as_deref())
        .bind(settlement.wallet_balance_before)
        .bind(settlement.wallet_balance_after)
        .bind(finalized_at)
        .execute(&mut *tx)
        .await
        .map_postgres_err()?;

        tx.commit().await.map_postgres_err()?;
        settlement.billing_status = billing_status;
        Ok(Some(settlement))
    }
}

async fn find_settlement_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    request_id: &str,
) -> Result<Option<StoredUsageSettlement>, DataLayerError> {
    let row = sqlx::query(
        r#"
SELECT request_id, billing_status, wallet_id,
       CAST(wallet_balance_before AS DOUBLE PRECISION) AS wallet_balance_before,
       CAST(wallet_balance_after AS DOUBLE PRECISION) AS wallet_balance_after,
       CAST(EXTRACT(EPOCH FROM finalized_at) AS BIGINT) AS finalized_at_unix_secs
FROM usage_settlement_snapshots
WHERE request_id = $1
LIMIT 1
"#,
    )
    .bind(request_id)
    .fetch_optional(&mut **tx)
    .await
    .map_postgres_err()?;
    row.as_ref().map(map_settlement).transpose()
}

fn map_settlement(row: &PgRow) -> Result<StoredUsageSettlement, DataLayerError> {
    Ok(StoredUsageSettlement {
        request_id: row.try_get("request_id").map_postgres_err()?,
        wallet_id: row.try_get("wallet_id").map_postgres_err()?,
        billing_status: row.try_get("billing_status").map_postgres_err()?,
        wallet_balance_before: row.try_get("wallet_balance_before").map_postgres_err()?,
        wallet_balance_after: row.try_get("wallet_balance_after").map_postgres_err()?,
        finalized_at_unix_secs: row
            .try_get::<Option<i64>, _>("finalized_at_unix_secs")
            .map_postgres_err()?
            .map(|value| {
                u64::try_from(value).map_err(|_| {
                    DataLayerError::UnexpectedValue("settlement timestamp is negative".to_string())
                })
            })
            .transpose()?,
    })
}
