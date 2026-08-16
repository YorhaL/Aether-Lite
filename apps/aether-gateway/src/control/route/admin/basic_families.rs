use axum::http;

use super::{classified, ClassifiedRoute};

pub(super) fn classify_admin_basic_family_route(
    method: &http::Method,
    normalized_path: &str,
) -> Option<ClassifiedRoute> {
    if method == http::Method::GET
        && matches!(
            normalized_path,
            "/api/admin/management-tokens/permissions/catalog"
                | "/api/admin/management-tokens/permissions/catalog/"
        )
    {
        Some(classified(
            "admin_proxy",
            "management_tokens_manage",
            "permissions_catalog",
            "admin:management_tokens",
            false,
        ))
    } else if method == http::Method::GET
        && matches!(
            normalized_path,
            "/api/admin/management-tokens" | "/api/admin/management-tokens/"
        )
    {
        Some(classified(
            "admin_proxy",
            "management_tokens_manage",
            "list_tokens",
            "admin:management_tokens",
            false,
        ))
    } else if method == http::Method::POST
        && matches!(
            normalized_path,
            "/api/admin/management-tokens" | "/api/admin/management-tokens/"
        )
    {
        Some(classified(
            "admin_proxy",
            "management_tokens_manage",
            "create_token",
            "admin:management_tokens",
            false,
        ))
    } else if method == http::Method::GET
        && normalized_path.starts_with("/api/admin/management-tokens/")
    {
        Some(classified(
            "admin_proxy",
            "management_tokens_manage",
            "get_token",
            "admin:management_tokens",
            false,
        ))
    } else if method == http::Method::PUT
        && normalized_path.starts_with("/api/admin/management-tokens/")
    {
        Some(classified(
            "admin_proxy",
            "management_tokens_manage",
            "update_token",
            "admin:management_tokens",
            false,
        ))
    } else if method == http::Method::DELETE
        && normalized_path.starts_with("/api/admin/management-tokens/")
    {
        Some(classified(
            "admin_proxy",
            "management_tokens_manage",
            "delete_token",
            "admin:management_tokens",
            false,
        ))
    } else if method == http::Method::POST
        && normalized_path.starts_with("/api/admin/management-tokens/")
        && normalized_path.ends_with("/regenerate")
    {
        Some(classified(
            "admin_proxy",
            "management_tokens_manage",
            "regenerate_token",
            "admin:management_tokens",
            false,
        ))
    } else if method == http::Method::PATCH
        && normalized_path.starts_with("/api/admin/management-tokens/")
        && normalized_path.ends_with("/status")
    {
        Some(classified(
            "admin_proxy",
            "management_tokens_manage",
            "toggle_status",
            "admin:management_tokens",
            false,
        ))
    } else if method == http::Method::GET
        && matches!(
            normalized_path,
            "/api/admin/ldap/config" | "/api/admin/ldap/config/"
        )
    {
        Some(classified(
            "admin_proxy",
            "ldap_manage",
            "get_config",
            "admin:ldap",
            false,
        ))
    } else if method == http::Method::PUT
        && matches!(
            normalized_path,
            "/api/admin/ldap/config" | "/api/admin/ldap/config/"
        )
    {
        Some(classified(
            "admin_proxy",
            "ldap_manage",
            "set_config",
            "admin:ldap",
            false,
        ))
    } else if method == http::Method::POST
        && matches!(
            normalized_path,
            "/api/admin/ldap/test" | "/api/admin/ldap/test/"
        )
    {
        Some(classified(
            "admin_proxy",
            "ldap_manage",
            "test_connection",
            "admin:ldap",
            false,
        ))
    } else if method == http::Method::GET
        && matches!(
            normalized_path,
            "/api/admin/gemini-files/mappings" | "/api/admin/gemini-files/mappings/"
        )
    {
        Some(classified(
            "admin_proxy",
            "gemini_files_manage",
            "list_mappings",
            "admin:gemini_files",
            false,
        ))
    } else if method == http::Method::GET
        && matches!(
            normalized_path,
            "/api/admin/gemini-files/stats" | "/api/admin/gemini-files/stats/"
        )
    {
        Some(classified(
            "admin_proxy",
            "gemini_files_manage",
            "stats",
            "admin:gemini_files",
            false,
        ))
    } else if method == http::Method::DELETE
        && matches!(
            normalized_path,
            "/api/admin/gemini-files/mappings" | "/api/admin/gemini-files/mappings/"
        )
    {
        Some(classified(
            "admin_proxy",
            "gemini_files_manage",
            "cleanup_mappings",
            "admin:gemini_files",
            false,
        ))
    } else if method == http::Method::DELETE
        && normalized_path.starts_with("/api/admin/gemini-files/mappings/")
    {
        Some(classified(
            "admin_proxy",
            "gemini_files_manage",
            "delete_mapping",
            "admin:gemini_files",
            false,
        ))
    } else if method == http::Method::GET
        && matches!(
            normalized_path,
            "/api/admin/gemini-files/capable-keys" | "/api/admin/gemini-files/capable-keys/"
        )
    {
        Some(classified(
            "admin_proxy",
            "gemini_files_manage",
            "capable_keys",
            "admin:gemini_files",
            false,
        ))
    } else if method == http::Method::POST
        && matches!(
            normalized_path,
            "/api/admin/gemini-files/upload" | "/api/admin/gemini-files/upload/"
        )
    {
        Some(classified(
            "admin_proxy",
            "gemini_files_manage",
            "upload",
            "admin:gemini_files",
            false,
        ))
    } else if method == http::Method::GET && normalized_path == "/api/admin/modules/status" {
        Some(classified(
            "admin_proxy",
            "modules_manage",
            "status_list",
            "admin:modules",
            false,
        ))
    } else if method == http::Method::GET
        && normalized_path.starts_with("/api/admin/modules/status/")
    {
        Some(classified(
            "admin_proxy",
            "modules_manage",
            "status_detail",
            "admin:modules",
            false,
        ))
    } else if method == http::Method::PUT
        && normalized_path.starts_with("/api/admin/modules/status/")
        && normalized_path.ends_with("/enabled")
    {
        Some(classified(
            "admin_proxy",
            "modules_manage",
            "set_enabled",
            "admin:modules",
            false,
        ))
    } else if method == http::Method::GET
        && matches!(
            normalized_path,
            "/api/admin/adaptive/keys" | "/api/admin/adaptive/keys/"
        )
    {
        Some(classified(
            "admin_proxy",
            "adaptive_manage",
            "list_keys",
            "admin:adaptive",
            false,
        ))
    } else if method == http::Method::GET
        && matches!(
            normalized_path,
            "/api/admin/adaptive/summary" | "/api/admin/adaptive/summary/"
        )
    {
        Some(classified(
            "admin_proxy",
            "adaptive_manage",
            "summary",
            "admin:adaptive",
            false,
        ))
    } else if method == http::Method::GET
        && normalized_path.starts_with("/api/admin/adaptive/keys/")
        && normalized_path.ends_with("/stats")
        && normalized_path.matches('/').count() == 6
    {
        Some(classified(
            "admin_proxy",
            "adaptive_manage",
            "get_stats",
            "admin:adaptive",
            false,
        ))
    } else if method == http::Method::PATCH
        && normalized_path.starts_with("/api/admin/adaptive/keys/")
        && normalized_path.ends_with("/mode")
        && normalized_path.matches('/').count() == 6
    {
        Some(classified(
            "admin_proxy",
            "adaptive_manage",
            "toggle_mode",
            "admin:adaptive",
            false,
        ))
    } else if method == http::Method::PATCH
        && normalized_path.starts_with("/api/admin/adaptive/keys/")
        && normalized_path.ends_with("/limit")
        && normalized_path.matches('/').count() == 6
    {
        Some(classified(
            "admin_proxy",
            "adaptive_manage",
            "set_limit",
            "admin:adaptive",
            false,
        ))
    } else if method == http::Method::DELETE
        && normalized_path.starts_with("/api/admin/adaptive/keys/")
        && normalized_path.ends_with("/learning")
        && normalized_path.matches('/').count() == 6
    {
        Some(classified(
            "admin_proxy",
            "adaptive_manage",
            "reset_learning",
            "admin:adaptive",
            false,
        ))
    } else {
        None
    }
}
