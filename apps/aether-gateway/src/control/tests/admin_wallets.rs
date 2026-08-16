use super::{classify_control_route, headers};
use http::Uri;

#[test]
fn classifies_admin_wallet_list_as_admin_proxy_route() {
    let uri: Uri = "/api/admin/wallets?page=1&page_size=20"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::GET, &uri, &headers(&[]));

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("wallets_manage"));
    assert_eq!(decision.route_kind.as_deref(), Some("list_wallets"));
    assert_eq!(decision.auth_signature, "admin:wallets");
}

#[test]
fn classifies_admin_wallet_detail_as_admin_proxy_route() {
    let uri: Uri = "/api/admin/wallets/wallet-123"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::GET, &uri, &headers(&[]));

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("wallets_manage"));
    assert_eq!(decision.route_kind.as_deref(), Some("wallet_detail"));
}

#[test]
fn classifies_admin_wallet_adjust_as_buffered_admin_proxy_route() {
    let uri: Uri = "/api/admin/wallets/wallet-123/adjust"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::POST, &uri, &headers(&[]));

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("wallets_manage"));
    assert_eq!(decision.route_kind.as_deref(), Some("adjust_balance"));
}
