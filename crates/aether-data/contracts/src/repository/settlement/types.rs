use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UsageSettlementInput {
    pub request_id: String,
    pub user_id: Option<String>,
    pub api_key_id: Option<String>,
    #[serde(default)]
    pub api_key_is_standalone: bool,
    pub status: String,
    pub billing_status: String,
    pub total_cost_usd: f64,
    pub actual_total_cost_usd: f64,
    pub finalized_at_unix_secs: Option<u64>,
}

impl UsageSettlementInput {
    pub fn validate(&self) -> Result<(), crate::DataLayerError> {
        if self.request_id.trim().is_empty() {
            return Err(crate::DataLayerError::InvalidInput(
                "settlement request_id cannot be empty".to_string(),
            ));
        }
        if self.status.trim().is_empty() || self.billing_status.trim().is_empty() {
            return Err(crate::DataLayerError::InvalidInput(
                "settlement status cannot be empty".to_string(),
            ));
        }
        if !self.total_cost_usd.is_finite() || !self.actual_total_cost_usd.is_finite() {
            return Err(crate::DataLayerError::InvalidInput(
                "settlement cost must be finite".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoredUsageSettlement {
    pub request_id: String,
    pub wallet_id: Option<String>,
    pub billing_status: String,
    pub wallet_balance_before: Option<f64>,
    pub wallet_balance_after: Option<f64>,
    pub finalized_at_unix_secs: Option<u64>,
}

#[async_trait]
pub trait SettlementWriteRepository: Send + Sync {
    async fn settle_usage(
        &self,
        input: UsageSettlementInput,
    ) -> Result<Option<StoredUsageSettlement>, crate::DataLayerError>;
}

pub trait SettlementRepository: SettlementWriteRepository + Send + Sync {}

impl<T> SettlementRepository for T where T: SettlementWriteRepository + Send + Sync {}

pub const SETTLEMENT_EPSILON_USD: f64 = 0.000_000_01;

pub fn settlement_billing_status_for_usage_status(status: &str) -> &'static str {
    match status {
        "completed" | "cancelled" => "settled",
        _ => "void",
    }
}

pub fn settlement_billable_cost_usd(input: &UsageSettlementInput) -> f64 {
    input.actual_total_cost_usd.max(0.0)
}
