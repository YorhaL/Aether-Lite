use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalletLookupKey<'a> {
    WalletId(&'a str),
    UserId(&'a str),
    ApiKeyId(&'a str),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoredWalletSnapshot {
    pub id: String,
    pub user_id: Option<String>,
    pub api_key_id: Option<String>,
    pub balance: f64,
    pub limit_mode: String,
    pub currency: String,
    pub status: String,
    pub total_consumed: f64,
    pub updated_at_unix_secs: u64,
}

impl StoredWalletSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        user_id: Option<String>,
        api_key_id: Option<String>,
        balance: f64,
        limit_mode: String,
        currency: String,
        status: String,
        total_consumed: f64,
        updated_at_unix_secs: i64,
    ) -> Result<Self, crate::DataLayerError> {
        if id.trim().is_empty() {
            return Err(crate::DataLayerError::UnexpectedValue(
                "wallet.id is empty".to_string(),
            ));
        }
        if limit_mode.trim().is_empty() || currency.trim().is_empty() || status.trim().is_empty() {
            return Err(crate::DataLayerError::UnexpectedValue(
                "wallet state is incomplete".to_string(),
            ));
        }
        if !balance.is_finite() || !total_consumed.is_finite() {
            return Err(crate::DataLayerError::UnexpectedValue(
                "wallet numeric value is not finite".to_string(),
            ));
        }
        Ok(Self {
            id,
            user_id,
            api_key_id,
            balance,
            limit_mode,
            currency,
            status,
            total_consumed,
            updated_at_unix_secs: u64::try_from(updated_at_unix_secs).map_err(|_| {
                crate::DataLayerError::UnexpectedValue(
                    "wallet.updated_at_unix_secs is negative".to_string(),
                )
            })?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct AdminWalletListQuery {
    pub status: Option<String>,
    pub owner_type: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoredAdminWalletListItem {
    pub id: String,
    pub user_id: Option<String>,
    pub api_key_id: Option<String>,
    pub balance: f64,
    pub limit_mode: String,
    pub currency: String,
    pub status: String,
    pub total_consumed: f64,
    pub user_name: Option<String>,
    pub api_key_name: Option<String>,
    pub created_at_unix_ms: Option<u64>,
    pub updated_at_unix_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct StoredAdminWalletListPage {
    pub items: Vec<StoredAdminWalletListItem>,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AdjustWalletBalanceInput {
    pub wallet_id: String,
    pub amount_usd: f64,
}

#[async_trait]
pub trait WalletReadRepository: Send + Sync {
    async fn find(
        &self,
        key: WalletLookupKey<'_>,
    ) -> Result<Option<StoredWalletSnapshot>, crate::DataLayerError>;

    async fn update_auth_user_wallet_limit_mode(
        &self,
        user_id: &str,
        limit_mode: &str,
    ) -> Result<Option<StoredWalletSnapshot>, crate::DataLayerError>;

    async fn update_auth_api_key_wallet_limit_mode(
        &self,
        api_key_id: &str,
        limit_mode: &str,
    ) -> Result<Option<StoredWalletSnapshot>, crate::DataLayerError>;

    async fn initialize_auth_user_wallet(
        &self,
        user_id: &str,
        initial_balance_usd: f64,
        unlimited: bool,
    ) -> Result<Option<StoredWalletSnapshot>, crate::DataLayerError>;

    async fn initialize_auth_api_key_wallet(
        &self,
        api_key_id: &str,
        initial_balance_usd: f64,
        unlimited: bool,
    ) -> Result<Option<StoredWalletSnapshot>, crate::DataLayerError>;

    #[allow(clippy::too_many_arguments)]
    async fn update_auth_user_wallet_snapshot(
        &self,
        user_id: &str,
        balance: f64,
        limit_mode: &str,
        currency: &str,
        status: &str,
        total_consumed: f64,
        updated_at_unix_secs: Option<u64>,
    ) -> Result<Option<StoredWalletSnapshot>, crate::DataLayerError>;

    #[allow(clippy::too_many_arguments)]
    async fn update_auth_api_key_wallet_snapshot(
        &self,
        api_key_id: &str,
        balance: f64,
        limit_mode: &str,
        currency: &str,
        status: &str,
        total_consumed: f64,
        updated_at_unix_secs: Option<u64>,
    ) -> Result<Option<StoredWalletSnapshot>, crate::DataLayerError>;

    async fn list_wallets_by_user_ids(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<StoredWalletSnapshot>, crate::DataLayerError>;

    async fn list_wallets_by_api_key_ids(
        &self,
        api_key_ids: &[String],
    ) -> Result<Vec<StoredWalletSnapshot>, crate::DataLayerError>;

    async fn list_admin_wallets(
        &self,
        query: &AdminWalletListQuery,
    ) -> Result<StoredAdminWalletListPage, crate::DataLayerError>;
}

#[async_trait]
pub trait WalletWriteRepository: Send + Sync {
    async fn adjust_wallet_balance(
        &self,
        input: AdjustWalletBalanceInput,
    ) -> Result<Option<StoredWalletSnapshot>, crate::DataLayerError>;
}

pub trait WalletRepository: WalletReadRepository + WalletWriteRepository + Send + Sync {}

impl<T> WalletRepository for T where T: WalletReadRepository + WalletWriteRepository + Send + Sync {}
