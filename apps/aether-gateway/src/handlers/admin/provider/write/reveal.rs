use crate::handlers::admin::request::AdminAppState;
use aether_data_contracts::repository::provider_catalog::StoredProviderCatalogKey;
use serde_json::json;

pub(crate) fn build_admin_reveal_key_payload(
    state: &AdminAppState<'_>,
    key: &StoredProviderCatalogKey,
) -> Result<serde_json::Value, String> {
    let decrypted = match key.encrypted_api_key.as_deref().map(str::trim) {
        Some(ciphertext) if !ciphertext.is_empty() => state
            .decrypt_catalog_secret_with_fallbacks(ciphertext)
            .ok_or_else(|| {
                "无法解密 API Key，可能是加密密钥已更改。请重新添加该密钥。".to_string()
            })?,
        _ => String::new(),
    };
    Ok(json!({
        "auth_type": key.auth_type,
        "api_key": decrypted,
    }))
}
