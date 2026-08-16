pub(in super::super) const ADMIN_WALLETS_DATA_UNAVAILABLE_DETAIL: &str =
    "Admin wallets data unavailable";

#[derive(Debug, serde::Deserialize)]
pub(in super::super) struct AdminWalletAdjustRequest {
    pub(in super::super) amount_usd: f64,
}
