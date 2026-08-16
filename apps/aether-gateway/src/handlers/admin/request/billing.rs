use super::AdminAppState;
use crate::GatewayError;

impl<'a> AdminAppState<'a> {
    pub(crate) async fn find_wallet(
        &self,
        lookup: aether_data::repository::wallet::WalletLookupKey<'_>,
    ) -> Result<Option<aether_data::repository::wallet::StoredWalletSnapshot>, GatewayError> {
        self.app.find_wallet(lookup).await
    }

    pub(crate) async fn list_admin_wallets(
        &self,
        status: Option<&str>,
        owner_type: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<
        (
            Vec<aether_data::repository::wallet::StoredAdminWalletListItem>,
            u64,
        ),
        GatewayError,
    > {
        self.app
            .list_admin_wallets(status, owner_type, limit, offset)
            .await
    }

    pub(crate) async fn admin_adjust_wallet_balance(
        &self,
        wallet_id: &str,
        amount_usd: f64,
    ) -> Result<Option<aether_data::repository::wallet::StoredWalletSnapshot>, GatewayError> {
        self.app
            .admin_adjust_wallet_balance(wallet_id, amount_usd)
            .await
    }
}
