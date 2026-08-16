pub(crate) fn admin_provider_id_for_keys(request_path: &str) -> Option<String> {
    request_path
        .strip_prefix("/api/admin/endpoints/providers/")?
        .strip_suffix("/keys")
        .map(ToOwned::to_owned)
}

pub(crate) fn admin_reveal_key_id(request_path: &str) -> Option<String> {
    request_path
        .strip_prefix("/api/admin/endpoints/keys/")?
        .strip_suffix("/reveal")
        .map(ToOwned::to_owned)
}

pub(crate) fn admin_update_key_id(request_path: &str) -> Option<String> {
    let key_id = request_path.strip_prefix("/api/admin/endpoints/keys/")?;
    (!key_id.is_empty() && !key_id.contains('/')).then_some(key_id.to_string())
}
