use super::*;

#[test]
fn admin_billing_wallets_boundaries_are_split() {
    let wallets_mod =
        read_workspace_file("apps/aether-gateway/src/handlers/admin/billing/wallets/mod.rs");
    for pattern in ["mod mutations;", "mod reads;", "mod routes;", "mod shared;"] {
        assert!(
            wallets_mod.contains(pattern),
            "handlers/admin/billing/wallets/mod.rs should register {pattern}"
        );
    }

    let shared_mod =
        read_workspace_file("apps/aether-gateway/src/handlers/admin/billing/wallets/shared/mod.rs");
    for pattern in [
        "mod normalizers;",
        "mod payloads;",
        "mod requests;",
        "mod responses;",
        "mod support;",
    ] {
        assert!(
            shared_mod.contains(pattern),
            "handlers/admin/billing/wallets/shared/mod.rs should register {pattern}"
        );
    }

    let mutations_mod = read_workspace_file(
        "apps/aether-gateway/src/handlers/admin/billing/wallets/mutations/mod.rs",
    );
    for pattern in ["mod adjust;"] {
        assert!(
            mutations_mod.contains(pattern),
            "handlers/admin/billing/wallets/mutations/mod.rs should register {pattern}"
        );
    }

    let reads_mod =
        read_workspace_file("apps/aether-gateway/src/handlers/admin/billing/wallets/reads/mod.rs");
    for pattern in ["mod detail;", "mod list;"] {
        assert!(
            reads_mod.contains(pattern),
            "handlers/admin/billing/wallets/reads/mod.rs should register {pattern}"
        );
    }

    for path in [
        "apps/aether-gateway/src/handlers/admin/billing/wallets/shared/core.rs",
        "apps/aether-gateway/src/handlers/admin/billing/wallets/mutations/core.rs",
        "apps/aether-gateway/src/handlers/admin/billing/wallets/reads.rs",
    ] {
        assert!(
            !workspace_file_exists(path),
            "{path} should be removed after wallets boundaries are split"
        );
    }
}

#[test]
fn admin_billing_wallets_support_uses_wrapped_request_context() {
    let wallets_support = read_workspace_file(
        "apps/aether-gateway/src/handlers/admin/billing/wallets/shared/support.rs",
    );
    assert!(
        wallets_support.contains("use crate::handlers::admin::request::AdminRequestContext;"),
        "handlers/admin/billing/wallets/shared/support.rs should consume wrapped AdminRequestContext"
    );
    assert!(
        !wallets_support.contains("GatewayPublicRequestContext"),
        "handlers/admin/billing/wallets/shared/support.rs should not keep raw GatewayPublicRequestContext seam"
    );
}
