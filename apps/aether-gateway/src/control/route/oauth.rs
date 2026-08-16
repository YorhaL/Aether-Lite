use super::{classified, ClassifiedRoute};

pub(super) fn classify_oauth_route(
    method: &http::Method,
    normalized_path: &str,
) -> Option<ClassifiedRoute> {
    if method == http::Method::GET && normalized_path == "/api/oauth/providers" {
        Some(classified(
            "public_support",
            "oauth",
            "list_providers",
            "user:oauth",
            false,
        ))
    } else if method == http::Method::GET
        && normalized_path.starts_with("/api/oauth/")
        && normalized_path.ends_with("/authorize")
    {
        Some(classified(
            "public_support",
            "oauth",
            "authorize",
            "user:oauth",
            false,
        ))
    } else if method == http::Method::GET
        && normalized_path.starts_with("/api/oauth/")
        && normalized_path.ends_with("/callback")
    {
        Some(classified(
            "public_support",
            "oauth",
            "callback",
            "user:oauth",
            false,
        ))
    } else if method == http::Method::GET && normalized_path == "/api/user/oauth/bindable-providers"
    {
        Some(classified(
            "public_support",
            "oauth",
            "bindable_providers",
            "user:oauth",
            false,
        ))
    } else if method == http::Method::GET && normalized_path == "/api/user/oauth/links" {
        Some(classified(
            "public_support",
            "oauth",
            "links",
            "user:oauth",
            false,
        ))
    } else if method == http::Method::POST
        && normalized_path.starts_with("/api/user/oauth/")
        && normalized_path.ends_with("/bind-token")
    {
        Some(classified(
            "public_support",
            "oauth",
            "bind_token",
            "user:oauth",
            false,
        ))
    } else if method == http::Method::GET
        && normalized_path.starts_with("/api/user/oauth/")
        && normalized_path.ends_with("/bind")
    {
        Some(classified(
            "public_support",
            "oauth",
            "bind",
            "user:oauth",
            false,
        ))
    } else if method == http::Method::DELETE
        && normalized_path.starts_with("/api/user/oauth/")
        && !normalized_path.contains("/bind")
    {
        Some(classified(
            "public_support",
            "oauth",
            "unbind",
            "user:oauth",
            false,
        ))
    } else if method == http::Method::GET && normalized_path == "/api/admin/oauth/supported-types" {
        Some(classified(
            "admin_proxy",
            "oauth_manage",
            "supported_types",
            "admin:oauth",
            false,
        ))
    } else if method == http::Method::GET
        && matches!(
            normalized_path,
            "/api/admin/oauth/providers" | "/api/admin/oauth/providers/"
        )
    {
        Some(classified(
            "admin_proxy",
            "oauth_manage",
            "list_providers",
            "admin:oauth",
            false,
        ))
    } else if method == http::Method::POST
        && normalized_path.starts_with("/api/admin/oauth/providers/")
        && normalized_path.ends_with("/test")
    {
        Some(classified(
            "admin_proxy",
            "oauth_manage",
            "test_provider",
            "admin:oauth",
            false,
        ))
    } else if method == http::Method::GET
        && normalized_path.starts_with("/api/admin/oauth/providers/")
    {
        Some(classified(
            "admin_proxy",
            "oauth_manage",
            "get_provider",
            "admin:oauth",
            false,
        ))
    } else if method == http::Method::PUT
        && normalized_path.starts_with("/api/admin/oauth/providers/")
    {
        Some(classified(
            "admin_proxy",
            "oauth_manage",
            "upsert_provider",
            "admin:oauth",
            false,
        ))
    } else if method == http::Method::DELETE
        && normalized_path.starts_with("/api/admin/oauth/providers/")
    {
        Some(classified(
            "admin_proxy",
            "oauth_manage",
            "delete_provider",
            "admin:oauth",
            false,
        ))
    } else {
        None
    }
}
