use aether_data::repository::wallet::StoredWalletSnapshot;
use aether_wallet::{
    WalletAccessDecision, WalletAccessFailure, WalletLimitMode, WalletSnapshot, WalletStatus,
};

use crate::control::GatewayLocalAuthRejection;
use crate::data::auth::GatewayAuthApiKeySnapshot;
use crate::{AppState, GatewayError};

pub(crate) async fn resolve_wallet_auth_gate(
    state: &AppState,
    auth_snapshot: &GatewayAuthApiKeySnapshot,
) -> Result<Option<WalletAccessDecision>, GatewayError> {
    resolve_wallet_auth_gate_with_cache(state, auth_snapshot, true).await
}

pub(crate) async fn resolve_wallet_auth_gate_uncached(
    state: &AppState,
    auth_snapshot: &GatewayAuthApiKeySnapshot,
) -> Result<Option<WalletAccessDecision>, GatewayError> {
    resolve_wallet_auth_gate_with_cache(state, auth_snapshot, false).await
}

async fn resolve_wallet_auth_gate_with_cache(
    state: &AppState,
    auth_snapshot: &GatewayAuthApiKeySnapshot,
    use_cache: bool,
) -> Result<Option<WalletAccessDecision>, GatewayError> {
    if !state.has_wallet_data_reader() {
        return Ok(None);
    }

    let wallet = if use_cache {
        state
            .read_wallet_snapshot_for_auth(
                &auth_snapshot.user_id,
                &auth_snapshot.api_key_id,
                auth_snapshot.api_key_is_standalone,
            )
            .await?
    } else {
        state
            .read_wallet_snapshot_for_auth_uncached(
                &auth_snapshot.user_id,
                &auth_snapshot.api_key_id,
                auth_snapshot.api_key_is_standalone,
            )
            .await?
    };

    let decision = match wallet.as_ref() {
        Some(wallet) => map_wallet_snapshot(wallet).access_decision(false),
        None => WalletAccessDecision::wallet_unavailable(None),
    };
    Ok(Some(decision))
}

pub(crate) fn local_rejection_from_wallet_access(
    decision: &WalletAccessDecision,
) -> Option<GatewayLocalAuthRejection> {
    match decision.failure.as_ref() {
        Some(WalletAccessFailure::WalletUnavailable) => {
            Some(GatewayLocalAuthRejection::WalletUnavailable)
        }
        Some(WalletAccessFailure::BalanceDenied) => {
            Some(GatewayLocalAuthRejection::BalanceDenied {
                remaining: decision.remaining,
            })
        }
        None => None,
    }
}

fn map_wallet_snapshot(snapshot: &StoredWalletSnapshot) -> WalletSnapshot {
    WalletSnapshot {
        wallet_id: snapshot.id.clone(),
        user_id: snapshot.user_id.clone(),
        api_key_id: snapshot.api_key_id.clone(),
        balance: snapshot.balance,
        limit_mode: WalletLimitMode::parse(&snapshot.limit_mode),
        currency: snapshot.currency.clone(),
        status: WalletStatus::parse(&snapshot.status),
    }
}
