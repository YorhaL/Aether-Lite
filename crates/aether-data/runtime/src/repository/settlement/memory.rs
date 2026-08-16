use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;

use super::{
    settlement_billable_cost_usd, settlement_billing_status_for_usage_status,
    SettlementWriteRepository, StoredUsageSettlement, UsageSettlementInput, SETTLEMENT_EPSILON_USD,
};
use crate::repository::wallet::{InMemoryWalletRepository, StoredWalletSnapshot};
use crate::DataLayerError;

#[derive(Debug)]
enum InMemorySettlementWalletStore {
    Owned(RwLock<BTreeMap<String, StoredWalletSnapshot>>),
    Shared(Arc<InMemoryWalletRepository>),
}

impl Default for InMemorySettlementWalletStore {
    fn default() -> Self {
        Self::Owned(RwLock::new(BTreeMap::new()))
    }
}

impl InMemorySettlementWalletStore {
    fn seeded<I>(items: I) -> Self
    where
        I: IntoIterator<Item = StoredWalletSnapshot>,
    {
        Self::Owned(RwLock::new(
            items
                .into_iter()
                .map(|wallet| (wallet.id.clone(), wallet))
                .collect(),
        ))
    }

    fn with_mut<R>(&self, f: impl FnOnce(&mut BTreeMap<String, StoredWalletSnapshot>) -> R) -> R {
        match self {
            Self::Owned(wallets) => f(&mut wallets.write().expect("settlement repo lock")),
            Self::Shared(repository) => repository.with_wallets_mut(f),
        }
    }
}

#[derive(Debug, Default)]
pub struct InMemorySettlementRepository {
    wallets: InMemorySettlementWalletStore,
    settlements: RwLock<BTreeMap<String, StoredUsageSettlement>>,
}

impl InMemorySettlementRepository {
    pub fn seed<I>(items: I) -> Self
    where
        I: IntoIterator<Item = StoredWalletSnapshot>,
    {
        Self {
            wallets: InMemorySettlementWalletStore::seeded(items),
            settlements: RwLock::new(BTreeMap::new()),
        }
    }

    pub fn from_wallet_repository(wallet_repository: Arc<InMemoryWalletRepository>) -> Self {
        Self {
            wallets: InMemorySettlementWalletStore::Shared(wallet_repository),
            settlements: RwLock::new(BTreeMap::new()),
        }
    }
}

#[async_trait]
impl SettlementWriteRepository for InMemorySettlementRepository {
    async fn settle_usage(
        &self,
        input: UsageSettlementInput,
    ) -> Result<Option<StoredUsageSettlement>, DataLayerError> {
        input.validate()?;
        if let Some(existing) = self
            .settlements
            .read()
            .expect("settlement snapshot lock")
            .get(&input.request_id)
            .cloned()
        {
            return Ok(Some(existing));
        }

        let billable_cost_usd = settlement_billable_cost_usd(&input);
        let mut billing_status =
            settlement_billing_status_for_usage_status(&input.status).to_string();
        let mut settlement = self.wallets.with_mut(|wallets| {
            let wallet_id = input
                .api_key_id
                .as_deref()
                .and_then(|id| {
                    wallets
                        .values()
                        .find(|wallet| wallet.api_key_id.as_deref() == Some(id))
                        .map(|wallet| wallet.id.clone())
                })
                .or_else(|| {
                    (!input.api_key_is_standalone).then_some(())?;
                    input.user_id.as_deref().and_then(|id| {
                        wallets
                            .values()
                            .find(|wallet| wallet.user_id.as_deref() == Some(id))
                            .map(|wallet| wallet.id.clone())
                    })
                });
            let wallet = wallet_id.as_deref().and_then(|id| wallets.get_mut(id));
            let mut settlement = StoredUsageSettlement {
                request_id: input.request_id.clone(),
                wallet_id: wallet.as_ref().map(|wallet| wallet.id.clone()),
                billing_status: billing_status.clone(),
                wallet_balance_before: wallet.as_ref().map(|wallet| wallet.balance),
                wallet_balance_after: wallet.as_ref().map(|wallet| wallet.balance),
                finalized_at_unix_secs: input.finalized_at_unix_secs,
            };

            if billing_status == "settled" {
                if let Some(wallet) = wallet {
                    if !wallet.limit_mode.eq_ignore_ascii_case("unlimited") {
                        wallet.balance -= billable_cost_usd;
                    }
                    wallet.total_consumed += billable_cost_usd;
                    settlement.wallet_balance_after = Some(wallet.balance);
                } else if billable_cost_usd > SETTLEMENT_EPSILON_USD {
                    billing_status = "insufficient_quota".to_string();
                    settlement.billing_status = billing_status.clone();
                }
            }
            settlement
        });
        settlement.billing_status = billing_status;
        self.settlements
            .write()
            .expect("settlement snapshot lock")
            .insert(settlement.request_id.clone(), settlement.clone());
        Ok(Some(settlement))
    }
}
