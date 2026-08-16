use axum::http;

use super::{classified, ClassifiedRoute};

pub(super) fn classify_admin_operations_family_route(
    method: &http::Method,
    normalized_path: &str,
) -> Option<ClassifiedRoute> {
    if method == http::Method::GET
        && matches!(
            normalized_path,
            "/api/admin/video-tasks" | "/api/admin/video-tasks/"
        )
    {
        Some(classified(
            "admin_proxy",
            "video_tasks_manage",
            "list_tasks",
            "admin:video_tasks",
            false,
        ))
    } else if method == http::Method::GET
        && matches!(
            normalized_path,
            "/api/admin/video-tasks/stats" | "/api/admin/video-tasks/stats/"
        )
    {
        Some(classified(
            "admin_proxy",
            "video_tasks_manage",
            "stats",
            "admin:video_tasks",
            false,
        ))
    } else if method == http::Method::GET
        && matches!(normalized_path, "/api/admin/tasks" | "/api/admin/tasks/")
    {
        Some(classified(
            "admin_proxy",
            "tasks_manage",
            "list_tasks",
            "admin:tasks",
            false,
        ))
    } else if method == http::Method::GET
        && matches!(
            normalized_path,
            "/api/admin/tasks/stats" | "/api/admin/tasks/stats/"
        )
    {
        Some(classified(
            "admin_proxy",
            "tasks_manage",
            "stats",
            "admin:tasks",
            false,
        ))
    } else if method == http::Method::GET
        && normalized_path.starts_with("/api/admin/tasks/")
        && normalized_path.ends_with("/events")
        && normalized_path.matches('/').count() == 5
    {
        Some(classified(
            "admin_proxy",
            "tasks_manage",
            "events",
            "admin:tasks",
            false,
        ))
    } else if method == http::Method::POST
        && normalized_path.starts_with("/api/admin/tasks/")
        && normalized_path.ends_with("/cancel")
        && normalized_path.matches('/').count() == 5
    {
        Some(classified(
            "admin_proxy",
            "tasks_manage",
            "cancel",
            "admin:tasks",
            false,
        ))
    } else if method == http::Method::POST
        && normalized_path.starts_with("/api/admin/tasks/")
        && normalized_path.ends_with("/trigger")
        && normalized_path.matches('/').count() == 5
    {
        Some(classified(
            "admin_proxy",
            "tasks_manage",
            "trigger",
            "admin:tasks",
            false,
        ))
    } else if method == http::Method::GET
        && normalized_path.starts_with("/api/admin/tasks/")
        && normalized_path["/api/admin/tasks/".len()..]
            .split('/')
            .count()
            == 1
        && !matches!(
            normalized_path,
            "/api/admin/tasks/stats" | "/api/admin/tasks/stats/"
        )
    {
        Some(classified(
            "admin_proxy",
            "tasks_manage",
            "detail",
            "admin:tasks",
            false,
        ))
    } else if method == http::Method::GET
        && normalized_path.starts_with("/api/admin/video-tasks/")
        && normalized_path.ends_with("/video")
        && normalized_path.matches('/').count() == 5
    {
        Some(classified(
            "admin_proxy",
            "video_tasks_manage",
            "video",
            "admin:video_tasks",
            false,
        ))
    } else if method == http::Method::POST
        && normalized_path.starts_with("/api/admin/video-tasks/")
        && normalized_path.ends_with("/cancel")
        && normalized_path.matches('/').count() == 5
    {
        Some(classified(
            "admin_proxy",
            "video_tasks_manage",
            "cancel",
            "admin:video_tasks",
            false,
        ))
    } else if method == http::Method::GET
        && normalized_path.starts_with("/api/admin/video-tasks/")
        && normalized_path["/api/admin/video-tasks/".len()..]
            .split('/')
            .count()
            == 1
        && !matches!(
            normalized_path,
            "/api/admin/video-tasks/stats" | "/api/admin/video-tasks/stats/"
        )
    {
        Some(classified(
            "admin_proxy",
            "video_tasks_manage",
            "detail",
            "admin:video_tasks",
            false,
        ))
    } else if method == http::Method::GET
        && matches!(
            normalized_path,
            "/api/admin/wallets" | "/api/admin/wallets/"
        )
    {
        Some(classified(
            "admin_proxy",
            "wallets_manage",
            "list_wallets",
            "admin:wallets",
            false,
        ))
    } else if method == http::Method::GET
        && normalized_path.starts_with("/api/admin/wallets/")
        && normalized_path.matches('/').count() == 4
    {
        Some(classified(
            "admin_proxy",
            "wallets_manage",
            "wallet_detail",
            "admin:wallets",
            false,
        ))
    } else if method == http::Method::POST
        && normalized_path.starts_with("/api/admin/wallets/")
        && normalized_path.ends_with("/adjust")
        && normalized_path.matches('/').count() == 5
    {
        Some(classified(
            "admin_proxy",
            "wallets_manage",
            "adjust_balance",
            "admin:wallets",
            false,
        ))
    } else if method == http::Method::GET
        && matches!(
            normalized_path,
            "/api/admin/user-groups" | "/api/admin/user-groups/"
        )
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "list_user_groups",
            "admin:users",
            false,
        ))
    } else if method == http::Method::POST
        && matches!(
            normalized_path,
            "/api/admin/user-groups" | "/api/admin/user-groups/"
        )
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "create_user_group",
            "admin:users",
            false,
        ))
    } else if method == http::Method::PUT
        && matches!(
            normalized_path,
            "/api/admin/user-groups/default" | "/api/admin/user-groups/default/"
        )
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "set_default_user_group",
            "admin:users",
            false,
        ))
    } else if method == http::Method::GET
        && normalized_path.starts_with("/api/admin/user-groups/")
        && normalized_path.ends_with("/members")
        && normalized_path.matches('/').count() == 5
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "list_user_group_members",
            "admin:users",
            false,
        ))
    } else if method == http::Method::PUT
        && normalized_path.starts_with("/api/admin/user-groups/")
        && normalized_path.ends_with("/members")
        && normalized_path.matches('/').count() == 5
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "replace_user_group_members",
            "admin:users",
            false,
        ))
    } else if method == http::Method::PUT
        && normalized_path.starts_with("/api/admin/user-groups/")
        && normalized_path.matches('/').count() == 4
        && !normalized_path.ends_with("/default")
        && !normalized_path.ends_with("/members")
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "update_user_group",
            "admin:users",
            false,
        ))
    } else if method == http::Method::DELETE
        && normalized_path.starts_with("/api/admin/user-groups/")
        && normalized_path.matches('/').count() == 4
        && !normalized_path.ends_with("/default")
        && !normalized_path.ends_with("/members")
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "delete_user_group",
            "admin:users",
            false,
        ))
    } else if method == http::Method::GET
        && matches!(normalized_path, "/api/admin/users" | "/api/admin/users/")
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "list_users",
            "admin:users",
            false,
        ))
    } else if method == http::Method::POST
        && matches!(normalized_path, "/api/admin/users" | "/api/admin/users/")
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "create_user",
            "admin:users",
            false,
        ))
    } else if method == http::Method::POST
        && matches!(
            normalized_path,
            "/api/admin/users/resolve-selection" | "/api/admin/users/resolve-selection/"
        )
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "resolve_user_selection",
            "admin:users",
            false,
        ))
    } else if method == http::Method::POST
        && matches!(
            normalized_path,
            "/api/admin/users/batch-action" | "/api/admin/users/batch-action/"
        )
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "batch_action_users",
            "admin:users",
            false,
        ))
    } else if method == http::Method::GET
        && normalized_path.starts_with("/api/admin/users/")
        && normalized_path.ends_with("/sessions")
        && normalized_path.matches('/').count() == 5
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "list_user_sessions",
            "admin:users",
            false,
        ))
    } else if method == http::Method::DELETE
        && normalized_path.starts_with("/api/admin/users/")
        && normalized_path.ends_with("/sessions")
        && normalized_path.matches('/').count() == 5
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "delete_user_sessions",
            "admin:users",
            false,
        ))
    } else if method == http::Method::DELETE
        && normalized_path.starts_with("/api/admin/users/")
        && normalized_path.contains("/sessions/")
        && normalized_path.matches('/').count() == 6
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "delete_user_session",
            "admin:users",
            false,
        ))
    } else if method == http::Method::GET
        && normalized_path.starts_with("/api/admin/users/")
        && normalized_path.ends_with("/api-keys")
        && normalized_path.matches('/').count() == 5
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "list_user_api_keys",
            "admin:users",
            false,
        ))
    } else if method == http::Method::POST
        && normalized_path.starts_with("/api/admin/users/")
        && normalized_path.ends_with("/api-keys")
        && normalized_path.matches('/').count() == 5
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "create_user_api_key",
            "admin:users",
            false,
        ))
    } else if method == http::Method::DELETE
        && normalized_path.starts_with("/api/admin/users/")
        && normalized_path.contains("/api-keys/")
        && !normalized_path.ends_with("/lock")
        && !normalized_path.ends_with("/full-key")
        && normalized_path.matches('/').count() == 6
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "delete_user_api_key",
            "admin:users",
            false,
        ))
    } else if method == http::Method::PUT
        && normalized_path.starts_with("/api/admin/users/")
        && normalized_path.contains("/api-keys/")
        && !normalized_path.ends_with("/lock")
        && !normalized_path.ends_with("/full-key")
        && normalized_path.matches('/').count() == 6
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "update_user_api_key",
            "admin:users",
            false,
        ))
    } else if method == http::Method::PATCH
        && normalized_path.starts_with("/api/admin/users/")
        && normalized_path.ends_with("/lock")
        && normalized_path.matches('/').count() == 7
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "lock_user_api_key",
            "admin:users",
            false,
        ))
    } else if method == http::Method::GET
        && normalized_path.starts_with("/api/admin/users/")
        && normalized_path.ends_with("/full-key")
        && normalized_path.matches('/').count() == 7
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "reveal_user_api_key",
            "admin:users",
            false,
        ))
    } else if method == http::Method::GET
        && normalized_path.starts_with("/api/admin/users/")
        && !normalized_path.ends_with("/sessions")
        && !normalized_path.contains("/sessions/")
        && !normalized_path.ends_with("/api-keys")
        && !normalized_path.contains("/api-keys/")
        && normalized_path.matches('/').count() == 4
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "get_user",
            "admin:users",
            false,
        ))
    } else if method == http::Method::PUT
        && normalized_path.starts_with("/api/admin/users/")
        && !normalized_path.ends_with("/sessions")
        && !normalized_path.contains("/sessions/")
        && !normalized_path.ends_with("/api-keys")
        && !normalized_path.contains("/api-keys/")
        && normalized_path.matches('/').count() == 4
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "update_user",
            "admin:users",
            false,
        ))
    } else if method == http::Method::DELETE
        && normalized_path.starts_with("/api/admin/users/")
        && !normalized_path.ends_with("/sessions")
        && !normalized_path.contains("/sessions/")
        && !normalized_path.ends_with("/api-keys")
        && !normalized_path.contains("/api-keys/")
        && normalized_path.matches('/').count() == 4
    {
        Some(classified(
            "admin_proxy",
            "users_manage",
            "delete_user",
            "admin:users",
            false,
        ))
    } else {
        None
    }
}
