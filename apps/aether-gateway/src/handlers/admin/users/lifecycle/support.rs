use super::super::format_optional_datetime_iso8601;
use crate::data::state::resolve_group_effective_rate_limit_policy;
use crate::handlers::admin::request::AdminAppState;
use crate::GatewayError;
use serde_json::json;

pub(super) async fn admin_user_password_policy(
    state: &AdminAppState<'_>,
) -> Result<String, GatewayError> {
    let config = state
        .read_system_config_json_value("password_policy_level")
        .await?;
    Ok(
        match config
            .as_ref()
            .and_then(|value| value.as_str())
            .unwrap_or("weak")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "medium" => "medium".to_string(),
            "strong" => "strong".to_string(),
            _ => "weak".to_string(),
        },
    )
}

pub(super) async fn find_admin_export_user(
    state: &AdminAppState<'_>,
    user_id: &str,
) -> Result<Option<aether_data::repository::users::StoredUserExportRow>, GatewayError> {
    state.find_export_user_by_id(user_id).await
}

pub(super) fn build_admin_user_payload(
    user: &aether_data::repository::users::StoredUserAuthRecord,
    rate_limit: Option<i32>,
    unlimited: bool,
) -> serde_json::Value {
    build_admin_user_payload_with_groups(user, rate_limit, None, unlimited, &[], &[])
}

pub(super) fn build_admin_user_payload_with_groups(
    user: &aether_data::repository::users::StoredUserAuthRecord,
    rate_limit: Option<i32>,
    rate_limit_mode: Option<&str>,
    unlimited: bool,
    groups: &[aether_data::repository::users::StoredUserGroup],
    policy_groups: &[aether_data::repository::users::StoredUserGroup],
) -> serde_json::Value {
    json!({
        "id": user.id,
        "email": user.email,
        "username": user.username,
        "role": user.role,
        "allowed_providers": user.allowed_providers,
        "allowed_providers_mode": user.allowed_providers_mode,
        "allowed_api_formats": user.allowed_api_formats,
        "allowed_api_formats_mode": user.allowed_api_formats_mode,
        "allowed_models": user.allowed_models,
        "allowed_models_mode": user.allowed_models_mode,
        "rate_limit": rate_limit,
        "rate_limit_mode": rate_limit_mode.unwrap_or("system"),
        "unlimited": unlimited,
        "is_active": user.is_active,
        "created_at": format_optional_datetime_iso8601(user.created_at),
        "updated_at": serde_json::Value::Null,
        "last_login_at": format_optional_datetime_iso8601(user.last_login_at),
        "groups": groups.iter().map(user_group_badge_payload).collect::<Vec<_>>(),
        "effective_policy": effective_policy_payload(
            user.allowed_providers.as_ref(),
            &user.allowed_providers_mode,
            user.allowed_api_formats.as_ref(),
            &user.allowed_api_formats_mode,
            user.allowed_models.as_ref(),
            &user.allowed_models_mode,
            policy_groups,
        ),
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_admin_user_export_payload(
    row: &aether_data::repository::users::StoredUserExportRow,
    unlimited: bool,
    created_at: Option<chrono::DateTime<chrono::Utc>>,
    last_login_at: Option<chrono::DateTime<chrono::Utc>>,
    request_count: u64,
    total_tokens: u64,
    groups: &[aether_data::repository::users::StoredUserGroup],
    policy_groups: &[aether_data::repository::users::StoredUserGroup],
) -> serde_json::Value {
    json!({
        "id": row.id,
        "email": row.email,
        "username": row.username,
        "role": row.role,
        "allowed_providers": row.allowed_providers,
        "allowed_providers_mode": row.allowed_providers_mode,
        "allowed_api_formats": row.allowed_api_formats,
        "allowed_api_formats_mode": row.allowed_api_formats_mode,
        "allowed_models": row.allowed_models,
        "allowed_models_mode": row.allowed_models_mode,
        "rate_limit": row.rate_limit,
        "rate_limit_mode": row.rate_limit_mode,
        "feature_settings": row.feature_settings,
        "unlimited": unlimited,
        "is_active": row.is_active,
        "created_at": format_optional_datetime_iso8601(created_at),
        "updated_at": serde_json::Value::Null,
        "last_login_at": format_optional_datetime_iso8601(last_login_at),
        "request_count": request_count,
        "total_tokens": total_tokens,
        "groups": groups.iter().map(user_group_badge_payload).collect::<Vec<_>>(),
        "effective_policy": effective_policy_payload(
            row.allowed_providers.as_ref(),
            &row.allowed_providers_mode,
            row.allowed_api_formats.as_ref(),
            &row.allowed_api_formats_mode,
            row.allowed_models.as_ref(),
            &row.allowed_models_mode,
            policy_groups,
        ),
    })
}

pub(super) fn user_group_badge_payload(
    group: &aether_data::repository::users::StoredUserGroup,
) -> serde_json::Value {
    json!({
        "id": group.id,
        "name": group.name,
    })
}

#[allow(clippy::too_many_arguments)]
fn effective_policy_payload(
    allowed_providers: Option<&Vec<String>>,
    allowed_providers_mode: &str,
    allowed_api_formats: Option<&Vec<String>>,
    allowed_api_formats_mode: &str,
    allowed_models: Option<&Vec<String>>,
    allowed_models_mode: &str,
    groups: &[aether_data::repository::users::StoredUserGroup],
) -> serde_json::Value {
    let mut sorted_groups = groups.to_vec();
    sorted_groups.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.id.cmp(&right.id))
    });
    json!({
        "allowed_providers": effective_list_policy_payload(
            allowed_providers,
            allowed_providers_mode,
            &sorted_groups,
            |group| (&group.allowed_providers_mode, group.allowed_providers.as_ref()),
        ),
        "allowed_api_formats": effective_list_policy_payload(
            allowed_api_formats,
            allowed_api_formats_mode,
            &sorted_groups,
            |group| (&group.allowed_api_formats_mode, group.allowed_api_formats.as_ref()),
        ),
        "allowed_models": effective_list_policy_payload(
            allowed_models,
            allowed_models_mode,
            &sorted_groups,
            |group| (&group.allowed_models_mode, group.allowed_models.as_ref()),
        ),
        "rate_limit": effective_rate_limit_policy_payload(&sorted_groups),
    })
}

fn effective_list_policy_payload(
    user_values: Option<&Vec<String>>,
    user_mode: &str,
    groups: &[aether_data::repository::users::StoredUserGroup],
    group_field: impl Fn(
        &aether_data::repository::users::StoredUserGroup,
    ) -> (&String, Option<&Vec<String>>),
) -> serde_json::Value {
    let mut effective = None;
    let mut group_sources = Vec::new();
    for group in groups {
        let (mode, values) = group_field(group);
        if let Some(restriction) = list_restriction_from_mode(mode, values.cloned()) {
            effective = intersect_list_policies(effective, Some(restriction));
            group_sources.push(group);
        }
    }
    let mut has_user_source = false;
    if let Some(restriction) = list_restriction_from_mode(user_mode, user_values.cloned()) {
        effective = intersect_list_policies(effective, Some(restriction));
        has_user_source = true;
    }

    let (mode, value) = match effective {
        Some(values) if values.is_empty() => ("deny_all", json!(Vec::<String>::new())),
        Some(values) => ("specific", json!(values)),
        None => ("unrestricted", serde_json::Value::Null),
    };
    let source = combined_policy_source(has_user_source, group_sources.len(), "fallback");
    policy_payload(mode, value, source, group_sources.as_slice())
}

fn effective_rate_limit_policy_payload(
    groups: &[aether_data::repository::users::StoredUserGroup],
) -> serde_json::Value {
    // Per-user policy columns are legacy-only. Reuse the runtime group resolver so the
    // admin payload reports the same grant (unlimited, otherwise the highest custom RPM).
    let group_sources = groups
        .iter()
        .filter(|group| group.rate_limit_mode == "custom")
        .collect::<Vec<_>>();
    let source = combined_policy_source(false, group_sources.len(), "fallback");
    match resolve_group_effective_rate_limit_policy(groups) {
        Some(rate_limit) => policy_payload("custom", json!(rate_limit), source, &group_sources),
        None => policy_payload("system", serde_json::Value::Null, source, &group_sources),
    }
}

fn policy_payload(
    mode: &str,
    value: serde_json::Value,
    source: &str,
    groups: &[&aether_data::repository::users::StoredUserGroup],
) -> serde_json::Value {
    let single_group = groups.first().copied().filter(|_| groups.len() == 1);
    json!({
        "mode": mode,
        "value": value,
        "source": source,
        "group_id": single_group.map(|group| group.id.as_str()),
        "group_name": single_group.map(|group| group.name.as_str()),
        "group_ids": groups.iter().map(|group| group.id.as_str()).collect::<Vec<_>>(),
        "group_names": groups.iter().map(|group| group.name.as_str()).collect::<Vec<_>>(),
    })
}

fn list_restriction_from_mode(mode: &str, values: Option<Vec<String>>) -> Option<Vec<String>> {
    match mode {
        "specific" => Some(values.unwrap_or_default()),
        "deny_all" => Some(Vec::new()),
        _ => None,
    }
}

fn intersect_list_policies(
    left: Option<Vec<String>>,
    right: Option<Vec<String>>,
) -> Option<Vec<String>> {
    match (left, right) {
        (None, None) => None,
        (Some(values), None) | (None, Some(values)) => Some(values),
        (Some(left_values), Some(right_values)) => {
            let right_values = right_values
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>();
            Some(
                left_values
                    .into_iter()
                    .filter(|value| right_values.contains(value))
                    .collect(),
            )
        }
    }
}

fn combined_policy_source(
    has_user_source: bool,
    group_source_count: usize,
    fallback_source: &'static str,
) -> &'static str {
    match (has_user_source, group_source_count) {
        (true, 0) => "user",
        (false, 1) => "group",
        (false, 0) => fallback_source,
        _ => "combined",
    }
}

pub(super) fn admin_user_id_from_detail_path(request_path: &str) -> Option<String> {
    let value = request_path
        .strip_prefix("/api/admin/users/")?
        .trim()
        .trim_matches('/')
        .to_string();
    if value.is_empty() || value.contains('/') {
        None
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_data::repository::users::{StoredUserAuthRecord, StoredUserGroup};

    fn sample_group(id: &str, name: &str, rate_limit: Option<i32>) -> StoredUserGroup {
        StoredUserGroup {
            id: id.to_string(),
            name: name.to_string(),
            normalized_name: name.to_ascii_lowercase(),
            description: None,
            priority: 0,
            allowed_providers: None,
            allowed_providers_mode: "inherit".to_string(),
            allowed_api_formats: None,
            allowed_api_formats_mode: "inherit".to_string(),
            allowed_models: None,
            allowed_models_mode: "inherit".to_string(),
            rate_limit,
            rate_limit_mode: "custom".to_string(),
            created_at: None,
            updated_at: None,
        }
    }

    fn sample_user() -> StoredUserAuthRecord {
        StoredUserAuthRecord {
            id: "user-1".to_string(),
            email: None,
            email_verified: false,
            username: "user-1".to_string(),
            password_hash: None,
            role: "user".to_string(),
            auth_source: "local".to_string(),
            allowed_providers: None,
            allowed_providers_mode: "unrestricted".to_string(),
            allowed_api_formats: None,
            allowed_api_formats_mode: "unrestricted".to_string(),
            allowed_models: None,
            allowed_models_mode: "unrestricted".to_string(),
            is_active: true,
            is_deleted: false,
            created_at: None,
            last_login_at: None,
        }
    }

    #[test]
    fn effective_rate_limit_payload_uses_highest_group_grant() {
        let payload = effective_rate_limit_policy_payload(&[
            sample_group("group-basic", "Basic", Some(30)),
            sample_group("group-pro", "Pro", Some(100)),
        ]);

        assert_eq!(payload["mode"], "custom");
        assert_eq!(payload["value"], 100);
        assert_eq!(payload["source"], "combined");
        assert_eq!(
            payload["group_ids"],
            serde_json::json!(["group-basic", "group-pro"])
        );
    }

    #[test]
    fn effective_rate_limit_payload_treats_unlimited_group_as_highest_grant() {
        let payload = effective_rate_limit_policy_payload(&[
            sample_group("group-basic", "Basic", Some(30)),
            sample_group("group-unlimited", "Unlimited", Some(0)),
        ]);

        assert_eq!(payload["mode"], "custom");
        assert_eq!(payload["value"], 0);
        assert_eq!(payload["source"], "combined");
    }

    #[test]
    fn effective_rate_limit_payload_ignores_legacy_user_policy_fields() {
        let payload = build_admin_user_payload_with_groups(
            &sample_user(),
            Some(10),
            Some("custom"),
            false,
            &[],
            &[],
        );
        let effective = &payload["effective_policy"]["rate_limit"];

        assert_eq!(payload["rate_limit"], 10);
        assert_eq!(effective["mode"], "system");
        assert!(effective["value"].is_null());
        assert_eq!(effective["source"], "fallback");
    }
}
