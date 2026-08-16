use crate::{AppState, GatewayError};

impl AppState {
    pub(crate) async fn find_wallet(
        &self,
        lookup: aether_data::repository::wallet::WalletLookupKey<'_>,
    ) -> Result<Option<aether_data::repository::wallet::StoredWalletSnapshot>, GatewayError> {
        #[cfg(test)]
        if let Some(store) = self.auth_wallet_store.as_ref() {
            let wallet = {
                let wallets = store.lock().expect("auth wallet store should lock");
                match lookup {
                    aether_data::repository::wallet::WalletLookupKey::WalletId(wallet_id) => {
                        wallets.get(wallet_id).cloned()
                    }
                    aether_data::repository::wallet::WalletLookupKey::UserId(user_id) => wallets
                        .values()
                        .find(|wallet| wallet.user_id.as_deref() == Some(user_id))
                        .cloned(),
                    aether_data::repository::wallet::WalletLookupKey::ApiKeyId(api_key_id) => {
                        wallets
                            .values()
                            .find(|wallet| wallet.api_key_id.as_deref() == Some(api_key_id))
                            .cloned()
                    }
                }
            };
            if wallet.is_some() {
                return Ok(wallet);
            }
        }

        self.data
            .find_wallet(lookup)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn read_wallet_snapshot_for_auth(
        &self,
        user_id: &str,
        api_key_id: &str,
        api_key_is_standalone: bool,
    ) -> Result<Option<aether_data::repository::wallet::StoredWalletSnapshot>, GatewayError> {
        let user_id = user_id.trim();
        let api_key_id = api_key_id.trim();
        let lookup = if api_key_is_standalone {
            (!api_key_id.is_empty()).then(|| {
                (
                    format!("api_key:{api_key_id}"),
                    aether_data::repository::wallet::WalletLookupKey::ApiKeyId(api_key_id),
                )
            })
        } else if !user_id.is_empty() {
            Some((
                format!("user:{user_id}"),
                aether_data::repository::wallet::WalletLookupKey::UserId(user_id),
            ))
        } else {
            (!api_key_id.is_empty()).then(|| {
                (
                    format!("api_key:{api_key_id}"),
                    aether_data::repository::wallet::WalletLookupKey::ApiKeyId(api_key_id),
                )
            })
        };

        let Some((cache_key, lookup)) = lookup else {
            return Ok(None);
        };

        let ttl = self.frontdoor_runtime_guards.auth_capacity_cache_ttl;
        if ttl.is_zero() {
            return self
                .read_wallet_snapshot_for_auth_uncached(user_id, api_key_id, api_key_is_standalone)
                .await;
        }

        self.auth_wallet_snapshot_cache
            .get_or_load(cache_key, ttl, || async move {
                let _permit = self.acquire_auth_snapshot_load_gate().await?;
                self.find_wallet(lookup).await
            })
            .await
    }

    pub(crate) async fn read_wallet_snapshot_for_auth_uncached(
        &self,
        user_id: &str,
        api_key_id: &str,
        api_key_is_standalone: bool,
    ) -> Result<Option<aether_data::repository::wallet::StoredWalletSnapshot>, GatewayError> {
        let user_id = user_id.trim();
        let api_key_id = api_key_id.trim();
        let lookup = if api_key_is_standalone {
            (!api_key_id.is_empty())
                .then(|| aether_data::repository::wallet::WalletLookupKey::ApiKeyId(api_key_id))
        } else if !user_id.is_empty() {
            Some(aether_data::repository::wallet::WalletLookupKey::UserId(
                user_id,
            ))
        } else {
            (!api_key_id.is_empty())
                .then(|| aether_data::repository::wallet::WalletLookupKey::ApiKeyId(api_key_id))
        };

        let Some(lookup) = lookup else {
            return Ok(None);
        };

        let _permit = self.acquire_auth_snapshot_load_gate().await?;
        self.find_wallet(lookup).await
    }

    pub(crate) async fn list_wallet_snapshots_by_user_ids(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<aether_data::repository::wallet::StoredWalletSnapshot>, GatewayError> {
        self.data
            .list_wallets_by_user_ids(user_ids)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    pub(crate) async fn list_wallet_snapshots_by_api_key_ids(
        &self,
        api_key_ids: &[String],
    ) -> Result<Vec<aether_data::repository::wallet::StoredWalletSnapshot>, GatewayError> {
        self.data
            .list_wallets_by_api_key_ids(api_key_ids)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }
}
