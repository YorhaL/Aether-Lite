use crate::handlers::admin::request::AdminAppState;
use crate::handlers::admin::shared::unix_secs_to_rfc3339;
use crate::GatewayError;
use serde_json::json;

#[derive(Clone)]
pub(in super::super) struct AdminWalletOwnerSummary {
    pub(in super::super) owner_type: &'static str,
    pub(in super::super) owner_name: Option<String>,
}

pub(in super::super) fn wallet_owner_summary_from_fields(
    user_id: Option<&str>,
    user_name: Option<String>,
    api_key_id: Option<&str>,
    api_key_name: Option<String>,
) -> AdminWalletOwnerSummary {
    if user_id.is_some() {
        return AdminWalletOwnerSummary {
            owner_type: "user",
            owner_name: user_name,
        };
    }
    if let Some(api_key_id) = api_key_id {
        return AdminWalletOwnerSummary {
            owner_type: "api_key",
            owner_name: api_key_name
                .filter(|value| !value.trim().is_empty())
                .or_else(|| Some(format!("Key-{}", &api_key_id[..api_key_id.len().min(8)]))),
        };
    }
    AdminWalletOwnerSummary {
        owner_type: "orphaned",
        owner_name: None,
    }
}

pub(in super::super) async fn resolve_admin_wallet_owner_summary(
    state: &AdminAppState<'_>,
    wallet: &aether_data::repository::wallet::StoredWalletSnapshot,
) -> Result<AdminWalletOwnerSummary, GatewayError> {
    if let Some(user_id) = wallet.user_id.as_deref() {
        let user = state.find_user_auth_by_id(user_id).await?;
        return Ok(AdminWalletOwnerSummary {
            owner_type: "user",
            owner_name: user.map(|record| record.username),
        });
    }
    if let Some(api_key_id) = wallet.api_key_id.as_deref() {
        let snapshots = state
            .list_auth_api_key_snapshots_by_ids(&[api_key_id.to_string()])
            .await?;
        let owner_name = snapshots
            .into_iter()
            .find(|snapshot| snapshot.api_key_id == api_key_id)
            .and_then(|snapshot| snapshot.api_key_name)
            .filter(|value| !value.trim().is_empty())
            .or_else(|| Some(format!("Key-{}", &api_key_id[..api_key_id.len().min(8)])));
        return Ok(AdminWalletOwnerSummary {
            owner_type: "api_key",
            owner_name,
        });
    }
    Ok(AdminWalletOwnerSummary {
        owner_type: "orphaned",
        owner_name: None,
    })
}

pub(in super::super) fn build_admin_wallet_summary_payload(
    wallet: &aether_data::repository::wallet::StoredWalletSnapshot,
    owner: &AdminWalletOwnerSummary,
) -> serde_json::Value {
    json!({
        "id": wallet.id.clone(),
        "user_id": wallet.user_id.clone(),
        "api_key_id": wallet.api_key_id.clone(),
        "owner_type": owner.owner_type,
        "owner_name": owner.owner_name.clone(),
        "balance": wallet.balance,
        "currency": wallet.currency.clone(),
        "status": wallet.status.clone(),
        "limit_mode": wallet.limit_mode.clone(),
        "unlimited": wallet.limit_mode.eq_ignore_ascii_case("unlimited"),
        "total_consumed": wallet.total_consumed,
        "created_at": serde_json::Value::Null,
        "updated_at": unix_secs_to_rfc3339(wallet.updated_at_unix_secs),
    })
}
