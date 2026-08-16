use std::collections::BTreeMap;
use std::sync::RwLock;

use async_trait::async_trait;

use super::{
    AdjustWalletBalanceInput, AdminWalletListQuery, StoredAdminWalletListItem,
    StoredAdminWalletListPage, StoredWalletSnapshot, WalletLookupKey, WalletReadRepository,
    WalletWriteRepository,
};
use crate::DataLayerError;

#[derive(Debug, Default)]
pub struct InMemoryWalletRepository {
    wallets_by_id: RwLock<BTreeMap<String, StoredWalletSnapshot>>,
}

impl InMemoryWalletRepository {
    pub fn seed<I>(items: I) -> Self
    where
        I: IntoIterator<Item = StoredWalletSnapshot>,
    {
        Self {
            wallets_by_id: RwLock::new(
                items
                    .into_iter()
                    .map(|wallet| (wallet.id.clone(), wallet))
                    .collect(),
            ),
        }
    }

    pub(crate) fn with_wallets_mut<R>(
        &self,
        f: impl FnOnce(&mut BTreeMap<String, StoredWalletSnapshot>) -> R,
    ) -> R {
        let mut wallets = self.wallets_by_id.write().expect("wallet repo lock");
        f(&mut wallets)
    }
}

fn current_unix_secs() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

fn update_by_owner(
    wallets: &RwLock<BTreeMap<String, StoredWalletSnapshot>>,
    matches: impl Fn(&StoredWalletSnapshot) -> bool,
    update: impl FnOnce(&mut StoredWalletSnapshot),
) -> Option<StoredWalletSnapshot> {
    let mut wallets = wallets.write().expect("wallet repo lock");
    let wallet = wallets.values_mut().find(|wallet| matches(wallet))?;
    update(wallet);
    Some(wallet.clone())
}

#[async_trait]
impl WalletReadRepository for InMemoryWalletRepository {
    async fn find(
        &self,
        key: WalletLookupKey<'_>,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        let wallets = self.wallets_by_id.read().expect("wallet repo lock");
        Ok(match key {
            WalletLookupKey::WalletId(id) => wallets.get(id).cloned(),
            WalletLookupKey::UserId(id) => wallets
                .values()
                .find(|wallet| wallet.user_id.as_deref() == Some(id))
                .cloned(),
            WalletLookupKey::ApiKeyId(id) => wallets
                .values()
                .find(|wallet| wallet.api_key_id.as_deref() == Some(id))
                .cloned(),
        })
    }

    async fn update_auth_user_wallet_limit_mode(
        &self,
        user_id: &str,
        limit_mode: &str,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        Ok(update_by_owner(
            &self.wallets_by_id,
            |wallet| wallet.user_id.as_deref() == Some(user_id),
            |wallet| {
                wallet.limit_mode = limit_mode.to_string();
                wallet.updated_at_unix_secs = current_unix_secs();
            },
        ))
    }

    async fn update_auth_api_key_wallet_limit_mode(
        &self,
        api_key_id: &str,
        limit_mode: &str,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        Ok(update_by_owner(
            &self.wallets_by_id,
            |wallet| wallet.api_key_id.as_deref() == Some(api_key_id),
            |wallet| {
                wallet.limit_mode = limit_mode.to_string();
                wallet.updated_at_unix_secs = current_unix_secs();
            },
        ))
    }

    async fn initialize_auth_user_wallet(
        &self,
        user_id: &str,
        initial_balance_usd: f64,
        unlimited: bool,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        initialize_wallet(
            &self.wallets_by_id,
            Some(user_id),
            None,
            initial_balance_usd,
            unlimited,
        )
    }

    async fn initialize_auth_api_key_wallet(
        &self,
        api_key_id: &str,
        initial_balance_usd: f64,
        unlimited: bool,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        initialize_wallet(
            &self.wallets_by_id,
            None,
            Some(api_key_id),
            initial_balance_usd,
            unlimited,
        )
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
        Ok(update_snapshot(
            &self.wallets_by_id,
            |wallet| wallet.user_id.as_deref() == Some(user_id),
            balance,
            limit_mode,
            currency,
            status,
            total_consumed,
            updated_at_unix_secs,
        ))
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
        Ok(update_snapshot(
            &self.wallets_by_id,
            |wallet| wallet.api_key_id.as_deref() == Some(api_key_id),
            balance,
            limit_mode,
            currency,
            status,
            total_consumed,
            updated_at_unix_secs,
        ))
    }

    async fn list_wallets_by_user_ids(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<StoredWalletSnapshot>, DataLayerError> {
        let wallets = self.wallets_by_id.read().expect("wallet repo lock");
        Ok(wallets
            .values()
            .filter(|wallet| {
                wallet
                    .user_id
                    .as_ref()
                    .is_some_and(|id| user_ids.contains(id))
            })
            .cloned()
            .collect())
    }

    async fn list_wallets_by_api_key_ids(
        &self,
        api_key_ids: &[String],
    ) -> Result<Vec<StoredWalletSnapshot>, DataLayerError> {
        let wallets = self.wallets_by_id.read().expect("wallet repo lock");
        Ok(wallets
            .values()
            .filter(|wallet| {
                wallet
                    .api_key_id
                    .as_ref()
                    .is_some_and(|id| api_key_ids.contains(id))
            })
            .cloned()
            .collect())
    }

    async fn list_admin_wallets(
        &self,
        query: &AdminWalletListQuery,
    ) -> Result<StoredAdminWalletListPage, DataLayerError> {
        let wallets = self.wallets_by_id.read().expect("wallet repo lock");
        let mut items = wallets
            .values()
            .filter(|wallet| {
                query
                    .status
                    .as_deref()
                    .is_none_or(|value| wallet.status == value)
            })
            .filter(|wallet| match query.owner_type.as_deref() {
                Some("user") => wallet.user_id.is_some(),
                Some("api_key") => wallet.api_key_id.is_some(),
                _ => true,
            })
            .map(|wallet| StoredAdminWalletListItem {
                id: wallet.id.clone(),
                user_id: wallet.user_id.clone(),
                api_key_id: wallet.api_key_id.clone(),
                balance: wallet.balance,
                limit_mode: wallet.limit_mode.clone(),
                currency: wallet.currency.clone(),
                status: wallet.status.clone(),
                total_consumed: wallet.total_consumed,
                user_name: None,
                api_key_name: None,
                created_at_unix_ms: None,
                updated_at_unix_secs: Some(wallet.updated_at_unix_secs),
            })
            .collect::<Vec<_>>();
        items.sort_by_key(|item| std::cmp::Reverse(item.updated_at_unix_secs));
        let total = items.len() as u64;
        let items = items
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .collect();
        Ok(StoredAdminWalletListPage { items, total })
    }
}

#[async_trait]
impl WalletWriteRepository for InMemoryWalletRepository {
    async fn adjust_wallet_balance(
        &self,
        input: AdjustWalletBalanceInput,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        if !input.amount_usd.is_finite() || input.amount_usd == 0.0 {
            return Err(DataLayerError::InvalidInput(
                "wallet adjustment must be a finite non-zero value".to_string(),
            ));
        }
        Ok(update_by_owner(
            &self.wallets_by_id,
            |wallet| wallet.id == input.wallet_id,
            |wallet| {
                wallet.balance += input.amount_usd;
                wallet.updated_at_unix_secs = current_unix_secs();
            },
        ))
    }
}

fn initialize_wallet(
    wallets: &RwLock<BTreeMap<String, StoredWalletSnapshot>>,
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
    let mut wallets = wallets.write().expect("wallet repo lock");
    if let Some(wallet) = wallets.values().find(|wallet| {
        wallet.user_id.as_deref() == user_id && wallet.api_key_id.as_deref() == api_key_id
    }) {
        return Ok(Some(wallet.clone()));
    }
    let wallet = StoredWalletSnapshot::new(
        uuid::Uuid::new_v4().to_string(),
        user_id.map(ToOwned::to_owned),
        api_key_id.map(ToOwned::to_owned),
        initial_balance_usd,
        if unlimited { "unlimited" } else { "finite" }.to_string(),
        "USD".to_string(),
        "active".to_string(),
        0.0,
        current_unix_secs() as i64,
    )?;
    wallets.insert(wallet.id.clone(), wallet.clone());
    Ok(Some(wallet))
}

#[allow(clippy::too_many_arguments)]
fn update_snapshot(
    wallets: &RwLock<BTreeMap<String, StoredWalletSnapshot>>,
    matches: impl Fn(&StoredWalletSnapshot) -> bool,
    balance: f64,
    limit_mode: &str,
    currency: &str,
    status: &str,
    total_consumed: f64,
    updated_at_unix_secs: Option<u64>,
) -> Option<StoredWalletSnapshot> {
    update_by_owner(wallets, matches, |wallet| {
        wallet.balance = balance;
        wallet.limit_mode = limit_mode.to_string();
        wallet.currency = currency.to_string();
        wallet.status = status.to_string();
        wallet.total_consumed = total_consumed;
        wallet.updated_at_unix_secs = updated_at_unix_secs.unwrap_or_else(current_unix_secs);
    })
}
