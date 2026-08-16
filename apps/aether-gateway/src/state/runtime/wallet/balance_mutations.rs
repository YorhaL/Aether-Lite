use crate::{AppState, GatewayError};

impl AppState {
    pub(crate) async fn admin_adjust_wallet_balance(
        &self,
        wallet_id: &str,
        amount_usd: f64,
    ) -> Result<Option<aether_data::repository::wallet::StoredWalletSnapshot>, GatewayError> {
        #[cfg(test)]
        if let Some(store) = self.auth_wallet_store.as_ref() {
            let mut guard = store.lock().expect("auth wallet store should lock");
            let Some(wallet) = guard.get_mut(wallet_id) else {
                return Ok(None);
            };
            wallet.balance += amount_usd;
            wallet.updated_at_unix_secs = chrono::Utc::now().timestamp().max(0) as u64;
            let wallet = wallet.clone();
            drop(guard);
            self.invalidate_auth_context_cache();
            return Ok(Some(wallet));
        }

        let result = self
            .data
            .adjust_wallet_balance(aether_data::repository::wallet::AdjustWalletBalanceInput {
                wallet_id: wallet_id.to_string(),
                amount_usd,
            })
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))?;
        if result.is_some() {
            self.invalidate_auth_context_cache();
        }
        Ok(result)
    }
}
