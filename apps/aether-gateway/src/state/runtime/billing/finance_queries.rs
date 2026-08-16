use aether_data::repository::wallet::{AdminWalletListQuery, StoredAdminWalletListItem};

use crate::{AppState, GatewayError};

impl AppState {
    pub(crate) async fn list_admin_wallets(
        &self,
        status: Option<&str>,
        owner_type: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<StoredAdminWalletListItem>, u64), GatewayError> {
        let page = self
            .data
            .list_admin_wallets(&AdminWalletListQuery {
                status: status.map(ToOwned::to_owned),
                owner_type: owner_type.map(ToOwned::to_owned),
                limit,
                offset,
            })
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))?;
        Ok((page.items, page.total))
    }
}
