use super::{collect_workspace_rust_files, read_workspace_file, workspace_file_exists};

fn assert_manifest_excludes(manifest_path: &str, forbidden: &[&str]) {
    let manifest = read_workspace_file(manifest_path);
    let violations = forbidden
        .iter()
        .filter(|dependency| manifest.contains(**dependency))
        .copied()
        .collect::<Vec<_>>();
    assert!(
        violations.is_empty(),
        "{manifest_path} crosses its dependency tier through: {}",
        violations.join(", ")
    );
}

#[test]
fn pure_policy_crates_do_not_depend_on_runtime_adapters() {
    let pure_manifests = [
        "crates/aether-admission-core/Cargo.toml",
        "crates/aether-provider/core/Cargo.toml",
        "crates/aether-task/core/Cargo.toml",
        "crates/aether-usage/core/Cargo.toml",
    ];
    let forbidden = [
        "axum",
        "sqlx",
        "redis",
        "reqwest",
        "wreq",
        "tokio",
        "aether-data =",
        "aether-gateway",
    ];

    for manifest in pure_manifests {
        assert_manifest_excludes(manifest, &forbidden);
    }
}

#[test]
fn database_adapters_are_independent_driver_boundaries() {
    let adapters = [
        (
            "crates/aether-data/adapters/postgres/Cargo.toml",
            "features = [\"postgres\"",
            "features = [\"sqlite\"",
        ),
        (
            "crates/aether-data/adapters/sqlite/Cargo.toml",
            "features = [\"sqlite\"",
            "features = [\"postgres\"",
        ),
    ];

    for (manifest_path, expected_driver, other_driver) in adapters {
        let manifest = read_workspace_file(manifest_path);
        assert!(manifest.contains("aether-data-contracts.workspace = true"));
        assert!(manifest.contains(expected_driver));
        assert_manifest_excludes(
            manifest_path,
            &[other_driver, "aether-gateway", "axum"],
        );
    }
}

#[test]
fn gateway_runtime_components_keep_focused_dependency_surfaces() {
    assert_manifest_excludes(
        "crates/aether-gateway/frontdoor/Cargo.toml",
        &[
            "aether-data",
            "aether-provider-transport",
            "aether-gateway-workers",
            "sqlx",
            "redis",
        ],
    );
    assert_manifest_excludes(
        "crates/aether-gateway/workers/Cargo.toml",
        &[
            "axum",
            "aether-gateway-frontdoor",
            "aether-provider-transport",
        ],
    );
    assert_manifest_excludes(
        "crates/aether-gateway/execution/Cargo.toml",
        &[
            "axum",
            "sqlx",
            "redis",
            "aether-data",
            "aether-gateway-frontdoor",
            "aether-gateway-workers",
        ],
    );
    assert_manifest_excludes(
        "crates/aether-gateway/control/Cargo.toml",
        &[
            "sqlx",
            "redis",
            "reqwest",
            "aether-data",
            "aether-provider-transport",
            "aether-gateway-workers",
        ],
    );
    assert_manifest_excludes(
        "crates/aether-testing/loadtools/Cargo.toml",
        &[
            "aether-gateway",
            "aether-testkit",
            "aether-data",
            "axum",
            "redis",
        ],
    );
}

#[test]
fn data_runtime_defaults_to_postgres_and_gateway_selects_supported_drivers_explicitly() {
    let data_manifest = read_workspace_file("crates/aether-data/runtime/Cargo.toml");
    assert!(data_manifest.contains("default = [\"postgres\"]"));
    for dependency in [
        "aether-data-postgres = { workspace = true, optional = true }",
        "aether-data-sqlite = { workspace = true, optional = true }",
    ] {
        assert!(
            data_manifest.contains(dependency),
            "aether-data should keep {dependency} optional"
        );
    }
    let gateway_manifest = read_workspace_file("apps/aether-gateway/Cargo.toml");
    assert!(gateway_manifest
        .contains("aether-data = { workspace = true, features = [\"postgres\", \"sqlite\"] }"));

    let data_lib = read_workspace_file("crates/aether-data/runtime/src/lib.rs");
    for backend in ["PostgresBackend", "SqliteBackend"] {
        assert!(
            data_lib.contains(&format!("pub use backend::{backend};")),
            "aether-data should expose enabled backends symmetrically at its crate root"
        );
    }
}

#[test]
fn data_query_helpers_belong_to_adapters_not_the_runtime_layer() {
    let data_manifest = read_workspace_file("crates/aether-data/runtime/Cargo.toml");
    assert!(
        !data_manifest.contains("aether-data-query.workspace = true"),
        "aether-data should not keep a direct query-helper dependency after SQL repositories move to adapters"
    );

    for adapter_manifest in [
        "crates/aether-data/adapters/postgres/Cargo.toml",
        "crates/aether-data/adapters/sqlite/Cargo.toml",
    ] {
        let manifest = read_workspace_file(adapter_manifest);
        assert!(
            manifest.contains("aether-data-query.workspace = true"),
            "{adapter_manifest} should own its query-helper dependency"
        );
    }

    let query_helpers = read_workspace_file("crates/aether-data/query/src/lib.rs");
    for dialect in ["Postgres", "Sqlite"] {
        assert!(
            query_helpers.contains(dialect),
            "aether-data-query should render the {dialect} dialect"
        );
    }
}

#[test]
fn sql_adapters_centralize_error_mapping_boilerplate() {
    for (adapter, driver) in [("aether-data-sqlite", "sqlite")] {
        let root = format!("crates/aether-data/adapters/{driver}/src");
        let files = collect_workspace_rust_files(&root);
        let trait_owners = files
            .iter()
            .filter(|path| {
                std::fs::read_to_string(path)
                    .expect("adapter source should be readable")
                    .contains("trait SqlResultExt<T>")
            })
            .collect::<Vec<_>>();

        assert_eq!(
            trait_owners.len(),
            1,
            "{adapter} should have exactly one SqlResultExt owner, found: {trait_owners:?}"
        );
        assert_eq!(
            trait_owners[0].file_name().and_then(|name| name.to_str()),
            Some("error.rs"),
            "{adapter} should keep SQL error mapping in src/error.rs"
        );

        let lib = read_workspace_file(&format!("crates/aether-data/adapters/{driver}/src/lib.rs"));
        assert!(lib.contains("mod error;"));
    }
}

#[test]
fn frontdoor_owns_bounded_request_body_buffering() {
    let frontdoor = read_workspace_file("crates/aether-gateway/frontdoor/src/body.rs");
    assert!(frontdoor.contains("acquire_many_owned"));
    assert!(frontdoor.contains("to_bytes(body, body_limit)"));
    assert!(frontdoor.contains("BodyBufferReservation"));

    let gateway = read_workspace_file("apps/aether-gateway/src/handlers/proxy/body_buffer.rs");
    assert!(gateway.contains("FrontdoorBodyBufferPolicy"));
    assert!(!gateway.contains("acquire_many_owned"));
    assert!(!gateway.contains("request_body_collection_exceeded_limit"));
}

#[test]
fn benchmark_binaries_are_outside_the_reusable_testkit() {
    let testkit_bin = "crates/aether-testing/testkit/src/bin";
    assert!(
        !workspace_file_exists(testkit_bin) || collect_workspace_rust_files(testkit_bin).is_empty(),
        "aether-testkit must not own benchmark binaries"
    );
    assert!(
        !collect_workspace_rust_files("crates/aether-testing/loadtools/src/bin").is_empty(),
        "standalone load tools should live in aether-loadtools"
    );
    assert!(
        !collect_workspace_rust_files("crates/aether-testing/integration/src/bin").is_empty(),
        "gateway-backed scenarios should live in aether-integration-tests"
    );
}

#[test]
fn testkit_gateway_harness_is_opt_in() {
    let testkit_manifest = read_workspace_file("crates/aether-testing/testkit/Cargo.toml");
    assert!(
        testkit_manifest.contains("default = []"),
        "aether-testkit should keep the default feature set dependency-light"
    );
    assert!(
        testkit_manifest
            .contains("gateway = [\"dep:aether-gateway\", \"dep:aether-runtime-state\"]"),
        "gateway harnesses should be behind the explicit gateway feature"
    );
    assert!(
        testkit_manifest.contains("postgres = [\"dep:aether-data\", \"dep:sqlx\"]"),
        "Postgres schema helpers should be behind the explicit postgres feature"
    );
    assert!(testkit_manifest.contains(
        "aether-gateway = { workspace = true, features = [\"testkit\"], optional = true }"
    ));

    let testkit_lib =
        read_workspace_file("crates/aether-testing/testkit/src/lib.rs").replace("\r\n", "\n");
    for module in ["execution_runtime", "gateway"] {
        assert!(
            testkit_lib.contains(&format!("#[cfg(feature = \"gateway\")]\nmod {module};")),
            "aether-testkit::{module} should be feature-gated"
        );
    }
    assert!(
        testkit_lib.contains("#[cfg(feature = \"postgres\")]\nmod postgres;"),
        "the Postgres helper should be feature-gated"
    );

    let integration_manifest = read_workspace_file("crates/aether-testing/integration/Cargo.toml");
    assert!(integration_manifest
        .contains("aether-testkit = { workspace = true, features = [\"gateway\", \"postgres\"] }"));
}
