use crate::handlers::admin::shared::query_param_value;

pub(in super::super) fn admin_wallet_id_from_detail_path(request_path: &str) -> Option<String> {
    let value = request_path
        .strip_prefix("/api/admin/wallets/")?
        .trim_matches('/');
    (!value.is_empty() && !value.contains('/')).then(|| value.to_string())
}

pub(in super::super) fn admin_wallet_id_from_suffix_path(
    request_path: &str,
    suffix: &str,
) -> Option<String> {
    request_path
        .strip_prefix("/api/admin/wallets/")?
        .strip_suffix(suffix)
        .map(|value| value.trim_matches('/').to_string())
        .filter(|value| !value.is_empty() && !value.contains('/'))
}

pub(in super::super) fn parse_admin_wallets_limit(query: Option<&str>) -> Result<usize, String> {
    match query_param_value(query, "limit") {
        Some(value) => {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| "limit must be an integer between 1 and 200".to_string())?;
            if (1..=200).contains(&parsed) {
                Ok(parsed)
            } else {
                Err("limit must be an integer between 1 and 200".to_string())
            }
        }
        None => Ok(50),
    }
}

pub(in super::super) fn parse_admin_wallets_offset(query: Option<&str>) -> Result<usize, String> {
    match query_param_value(query, "offset") {
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| "offset must be a non-negative integer".to_string()),
        None => Ok(0),
    }
}

pub(in super::super) fn parse_admin_wallets_owner_type_filter(
    query: Option<&str>,
) -> Option<String> {
    match query_param_value(query, "owner_type") {
        Some(value) if value.eq_ignore_ascii_case("user") => Some("user".to_string()),
        Some(value) if value.eq_ignore_ascii_case("api_key") => Some("api_key".to_string()),
        _ => None,
    }
}
