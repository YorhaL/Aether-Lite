use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use super::{
    build_import_plan, decode_jsonl, encode_jsonl, export_postgres_core_jsonl,
    export_sqlite_core_jsonl, filter_import_payload, import_postgres_jsonl, import_sqlite_jsonl,
    normalize_imported_binary, normalize_imported_integer_timestamp,
    normalize_postgres_import_payload, postgres_bytea_json_value, postgres_core_export_domains,
    sqlite_core_export_domains, sqlite_schema_copy_insert_sql, DataExportManifest,
    DataExportRecord, DataImportPlan, ExportDomain, ExportRow, PostgresImportColumn,
    SchemaCopyColumn, SchemaCopyTable, SqliteCopyColumn, AUXILIARY_TABLES,
};
use crate::lifecycle::migrate::{run_migrations as run_postgres_migrations, run_sqlite_migrations};
use crate::DatabaseDriver;
use aether_data_postgres::{PostgresPoolConfig, PostgresPoolFactory};

#[test]
fn jsonl_round_trips_manifest_and_domain_rows() {
    let records = vec![
        DataExportRecord::manifest(DataExportManifest::new(
            1_700_000_000,
            Some(DatabaseDriver::Postgres),
            vec![ExportDomain::Users, ExportDomain::ApiKeys],
        )),
        DataExportRecord::row(
            ExportDomain::Users,
            "user-1",
            json!({
                "id": "user-1",
                "email": "owner@example.com"
            }),
        ),
        DataExportRecord::row(
            ExportDomain::ApiKeys,
            "api-key-1",
            json!({
                "id": "api-key-1",
                "key_hash": "ciphertext-preserved"
            }),
        ),
    ];

    let encoded = encode_jsonl(&records).expect("records should encode");
    assert_eq!(encoded.lines().count(), 3);

    let decoded = decode_jsonl(&encoded).expect("records should decode");
    assert_eq!(decoded, records);

    let import_plan = build_import_plan(&encoded).expect("import plan should build");
    assert_eq!(
        import_plan.manifest.source_driver,
        Some(DatabaseDriver::Postgres)
    );
    assert_eq!(import_plan.rows(ExportDomain::Users).len(), 1);
    assert_eq!(
        import_plan.rows(ExportDomain::ApiKeys)[0].payload["key_hash"],
        "ciphertext-preserved"
    );
}

#[test]
fn core_export_domains_match_across_sql_drivers() {
    assert_eq!(sqlite_core_export_domains(), postgres_core_export_domains());
    assert!(sqlite_core_export_domains().contains(&ExportDomain::Auxiliary));
}

#[tokio::test]
async fn sqlite_core_export_tables_exist_in_the_migrated_schema() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("sqlite pool should connect");
    run_sqlite_migrations(&pool)
        .await
        .expect("sqlite migrations should run");

    let schema_tables = sqlx::query_scalar::<_, String>(
        r#"
SELECT name
FROM sqlite_master
WHERE type = 'table'
  AND name NOT LIKE 'sqlite_%'
  AND name NOT IN ('_sqlx_migrations', 'schema_backfills')
ORDER BY name
"#,
    )
    .fetch_all(&pool)
    .await
    .expect("sqlite schema tables should load")
    .into_iter()
    .collect::<BTreeSet<_>>();

    let mut exported_tables = [
        "users",
        "api_keys",
        "providers",
        "provider_api_keys",
        "provider_endpoints",
        "global_models",
        "models",
        "auth_modules",
        "oauth_providers",
        "user_oauth_links",
        "user_groups",
        "user_group_members",
        "proxy_nodes",
        "system_configs",
        "usage",
        "wallets",
        "wallet_transactions",
        "wallet_daily_usage_ledgers",
        "payment_orders",
        "payment_callbacks",
        "refund_requests",
        "redeem_code_batches",
        "redeem_codes",
        "billing_rules",
        "dimension_collectors",
        "usage_settlement_snapshots",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<BTreeSet<_>>();
    exported_tables.extend(AUXILIARY_TABLES.iter().map(|table| table.name.to_string()));

    assert!(exported_tables.is_subset(&schema_tables));
}

#[test]
fn version_one_exports_remain_importable_after_full_export_expansion() {
    let records = decode_jsonl(
        r#"{"record_type":"manifest","manifest":{"format_version":1,"created_at_unix_secs":1,"source_driver":null,"domains":["users"]}}
{"record_type":"row","domain":"users","id":"user-1","payload":{"id":"user-1"}}"#,
    )
    .expect("version one exports should remain supported");

    assert_eq!(records.len(), 2);
}

#[test]
fn jsonl_rejects_missing_manifest() {
    let err = decode_jsonl(r#"{"record_type":"row","domain":"users","id":"user-1","payload":{}}"#)
        .expect_err("missing manifest should fail");

    assert!(err.to_string().contains("must start with a manifest"));
}

#[test]
fn jsonl_rejects_rows_outside_manifest_domains() {
    let records = vec![
        DataExportRecord::manifest(DataExportManifest::new(
            1_700_000_000,
            Some(DatabaseDriver::Sqlite),
            vec![ExportDomain::Users],
        )),
        DataExportRecord::row(
            ExportDomain::Wallets,
            "wallet-1",
            json!({ "id": "wallet-1" }),
        ),
    ];

    let err = encode_jsonl(&records).expect_err("undeclared domain should fail");
    assert!(err.to_string().contains("not declared in manifest"));
}

#[test]
fn jsonl_rejects_bad_json_with_line_number() {
    let err = decode_jsonl(
            r#"{"record_type":"manifest","manifest":{"format_version":1,"created_at_unix_secs":1,"source_driver":null,"domains":["users"]}}
not-json"#,
        )
        .expect_err("bad json should fail");

    assert!(err.to_string().contains("line 2"));
}

#[test]
fn jsonl_rejects_duplicate_domain_ids() {
    let records = vec![
        DataExportRecord::manifest(DataExportManifest::new(
            1_700_000_000,
            None,
            vec![ExportDomain::Users],
        )),
        DataExportRecord::row(ExportDomain::Users, "user-1", json!({ "id": "user-1" })),
        DataExportRecord::row(ExportDomain::Users, "user-1", json!({ "id": "user-1" })),
    ];

    let err = encode_jsonl(&records).expect_err("duplicate id should fail");
    assert!(err.to_string().contains("duplicate"));
}

#[test]
fn postgres_import_payload_normalizes_sqlite_values_for_target_columns() {
    let target_columns = BTreeMap::from([
        (
            "id".to_string(),
            postgres_column("character varying", "varchar"),
        ),
        (
            "email_verified".to_string(),
            postgres_column("boolean", "bool"),
        ),
        (
            "created_at".to_string(),
            postgres_column("timestamp with time zone", "timestamptz"),
        ),
        (
            "allowed_models".to_string(),
            postgres_column("json", "json"),
        ),
        (
            "role".to_string(),
            postgres_not_null_default_column("USER-DEFINED", "userrole"),
        ),
    ]);
    let row = ExportRow {
        id: "user-1".to_string(),
        payload: json!({
            "id": "user-1",
            "email_verified": 1,
            "created_at": 1,
            "allowed_models": "[\"gpt-test\"]",
            "role": null,
            "legacy_nullable": null
        }),
    };

    let normalized = normalize_postgres_import_payload(
        "public.users",
        ExportDomain::Users,
        &row,
        &target_columns,
    )
    .expect("postgres payload should normalize");

    assert_eq!(normalized["email_verified"], json!(true));
    assert_eq!(normalized["created_at"], json!("1970-01-01T00:00:01+00:00"));
    assert_eq!(normalized["allowed_models"], json!(["gpt-test"]));
    assert!(!normalized.contains_key("role"));
    assert!(!normalized.contains_key("legacy_nullable"));
}

#[test]
fn cross_driver_timestamp_normalization_preserves_usage_second_contract() {
    assert_eq!(
        normalize_imported_integer_timestamp(
            "sqlite",
            r#""usage""#,
            "created_at_unix_ms",
            &json!("1970-01-01T00:00:01.234900Z"),
        )
        .expect("usage timestamp should normalize"),
        Some(1),
    );
    let target_columns = BTreeMap::from([(
        "created_at_unix_ms".to_string(),
        postgres_column("timestamp with time zone", "timestamptz"),
    )]);
    let row = ExportRow {
        id: "usage-1".to_string(),
        payload: json!({ "created_at_unix_ms": 1_700_000_000 }),
    };
    let normalized = normalize_postgres_import_payload(
        "public.usage",
        ExportDomain::Usage,
        &row,
        &target_columns,
    )
    .expect("postgres usage timestamp should normalize");
    assert_eq!(
        normalized["created_at_unix_ms"],
        json!("2023-11-14T22:13:20+00:00")
    );

    let target_columns = BTreeMap::from([(
        "created_at_unix_ms".to_string(),
        postgres_column("bigint", "int8"),
    )]);
    let row = ExportRow {
        id: "usage-1".to_string(),
        payload: json!({ "created_at_unix_ms": "1970-01-01T00:00:01.234900Z" }),
    };
    let normalized = normalize_postgres_import_payload(
        "public.usage",
        ExportDomain::Usage,
        &row,
        &target_columns,
    )
    .expect("postgres integer usage timestamp should normalize");
    assert_eq!(normalized["created_at_unix_ms"], json!(1));
}

#[test]
fn cross_driver_binary_normalization_preserves_raw_bytes() {
    assert_eq!(
        normalize_imported_binary("sqlite", "payload_gzip", &json!([0, 1, 127, 255]))
            .expect("byte array should normalize"),
        Some(vec![0, 1, 127, 255]),
    );
    assert_eq!(
        normalize_imported_binary("sqlite", "payload_gzip", &json!("\\x00017fff"))
            .expect("postgres hex should normalize"),
        Some(vec![0, 1, 127, 255]),
    );
    assert!(normalize_imported_binary("sqlite", "payload_gzip", &json!([256])).is_err());
    assert_eq!(
        postgres_bytea_json_value("payload_gzip", &json!([0, 1, 127, 255]))
            .expect("postgres bytea should normalize"),
        json!("\\x00017fff"),
    );
}

#[test]
fn postgres_import_payload_rejects_non_null_unknown_columns() {
    let target_columns = BTreeMap::from([(
        "id".to_string(),
        postgres_column("character varying", "varchar"),
    )]);
    let row = ExportRow {
        id: "user-1".to_string(),
        payload: json!({
            "id": "user-1",
            "unexpected_column": "value"
        }),
    };

    let err = normalize_postgres_import_payload(
        "public.users",
        ExportDomain::Users,
        &row,
        &target_columns,
    )
    .expect_err("non-null unknown columns should fail");

    assert!(err.to_string().contains("unexpected_column"));
    assert!(err.to_string().contains("does not exist"));
}

#[test]
fn sqlite_import_payloads_reject_non_null_unknown_columns() {
    let target_columns = BTreeSet::from(["id".to_string()]);
    let row = ExportRow {
        id: "user-1".to_string(),
        payload: json!({
            "id": "user-1",
            "legacy_nullable": null,
            "unexpected_column": "value"
        }),
    };

    let err = filter_import_payload(
        "sqlite",
        "users",
        ExportDomain::Users,
        &row,
        &target_columns,
    )
    .expect_err("non-null unknown columns should fail");

    assert!(err.to_string().contains("unexpected_column"));
    assert!(err.to_string().contains("does not exist"));
    assert!(err.to_string().contains("sqlite"));
}

#[test]
fn sqlite_import_payloads_ignore_unknown_null_columns() {
    let target_columns = BTreeSet::from(["id".to_string()]);
    let row = ExportRow {
        id: "user-1".to_string(),
        payload: json!({
            "id": "user-1",
            "legacy_nullable": null
        }),
    };

    let filtered = filter_import_payload(
        "sqlite",
        "users",
        ExportDomain::Users,
        &row,
        &target_columns,
    )
    .expect("unknown null columns should remain backward compatible");

    assert_eq!(
        filtered,
        serde_json::Map::from_iter([("id".to_string(), json!("user-1"))])
    );
}

#[test]
fn postgres_to_sqlite_copy_uses_primary_key_upsert_instead_of_replace() {
    let table = SchemaCopyTable {
        table_name: "usage".to_string(),
        columns: vec![
            SchemaCopyColumn {
                sqlite: SqliteCopyColumn {
                    name: "request_id".to_string(),
                    declared_type: "TEXT".to_string(),
                    not_null: true,
                    has_default: false,
                    primary_key_position: 1,
                },
                postgres: postgres_column("character varying", "varchar"),
            },
            SchemaCopyColumn {
                sqlite: SqliteCopyColumn {
                    name: "status".to_string(),
                    declared_type: "TEXT".to_string(),
                    not_null: true,
                    has_default: false,
                    primary_key_position: 0,
                },
                postgres: postgres_column("character varying", "varchar"),
            },
        ],
    };

    let sql = sqlite_schema_copy_insert_sql(&table).expect("copy SQL should build");

    assert!(!sql.contains("OR REPLACE"));
    assert!(sql.contains("ON CONFLICT (\"request_id\") DO UPDATE SET"));
    assert!(sql.contains("\"status\" = excluded.\"status\""));
}

fn postgres_column(data_type: &str, udt_name: &str) -> PostgresImportColumn {
    PostgresImportColumn {
        data_type: data_type.to_ascii_lowercase(),
        udt_name: udt_name.to_ascii_lowercase(),
        is_nullable: true,
        has_default: false,
    }
}

fn postgres_not_null_default_column(data_type: &str, udt_name: &str) -> PostgresImportColumn {
    PostgresImportColumn {
        data_type: data_type.to_ascii_lowercase(),
        udt_name: udt_name.to_ascii_lowercase(),
        is_nullable: false,
        has_default: true,
    }
}

#[tokio::test]
async fn sqlite_import_rejects_non_integer_timestamp_values() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("sqlite pool should connect");
    run_sqlite_migrations(&pool)
        .await
        .expect("sqlite migrations should run");

    for invalid_value in [
        json!("not-a-timestamp"),
        json!(1.5),
        json!(true),
        json!({"unexpected": "object"}),
    ] {
        let encoded = encode_jsonl(&[
            DataExportRecord::manifest(DataExportManifest::new(
                1_700_000_000,
                Some(DatabaseDriver::Postgres),
                vec![ExportDomain::GlobalModels],
            )),
            DataExportRecord::row(
                ExportDomain::GlobalModels,
                "invalid-timestamp",
                json!({
                    "id": "invalid-timestamp",
                    "name": "invalid-timestamp",
                    "created_at": invalid_value,
                    "updated_at": 1
                }),
            ),
        ])
        .expect("invalid timestamp fixture should encode");

        let err = import_sqlite_jsonl(&pool, &encoded)
            .await
            .expect_err("non-integer timestamp should be rejected");
        assert!(err.to_string().contains(
            "timestamp column 'created_at' must contain an integer or supported datetime"
        ));
    }
}

#[tokio::test]
async fn sqlite_import_updates_parent_without_cascading_child_rows() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("sqlite pool should connect");
    run_sqlite_migrations(&pool)
        .await
        .expect("sqlite migrations should run");
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .expect("foreign keys should be enabled");
    sqlx::raw_sql(
        r#"
INSERT INTO users (id, email, username, created_at, updated_at)
VALUES ('import-user', 'import@example.test', 'import-user', 1, 1);
INSERT INTO user_groups (
  id, name, normalized_name, description, priority,
  allowed_providers_mode, allowed_api_formats_mode, allowed_models_mode, rate_limit_mode,
  created_at, updated_at
)
VALUES (
  'import-group', 'Before', 'import-group', 'preserve-me', 0,
  'inherit', 'inherit', 'inherit', 'inherit', 1, 1
);
INSERT INTO user_group_members (group_id, user_id, created_at)
VALUES ('import-group', 'import-user', 1);
"#,
    )
    .execute(&pool)
    .await
    .expect("parent and child fixtures should insert");

    let encoded = encode_jsonl(&[
        DataExportRecord::manifest(DataExportManifest::new(
            1_700_000_000,
            Some(DatabaseDriver::Postgres),
            vec![ExportDomain::UserGroups],
        )),
        DataExportRecord::row(
            ExportDomain::UserGroups,
            "import-group",
            json!({
                "id": "import-group",
                "name": "After",
                "normalized_name": "import-group",
                "priority": 10,
                "allowed_providers_mode": "inherit",
                "allowed_api_formats_mode": "inherit",
                "allowed_models_mode": "inherit",
                "rate_limit_mode": "inherit",
                "created_at": 1,
                "updated_at": 2
            }),
        ),
    ])
    .expect("group export should encode");

    assert_eq!(
        import_sqlite_jsonl(&pool, &encoded)
            .await
            .expect("group import should update in place"),
        1
    );
    let group = sqlx::query_as::<_, (String, String)>(
        "SELECT name, description FROM user_groups WHERE id = 'import-group'",
    )
    .fetch_one(&pool)
    .await
    .expect("updated group should load");
    assert_eq!(group, ("After".to_string(), "preserve-me".to_string()));
    let member_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_group_members WHERE group_id = 'import-group'",
    )
    .fetch_one(&pool)
    .await
    .expect("group member count should load");
    assert_eq!(member_count, 1);
}

#[tokio::test]
async fn sqlite_import_rolls_back_rows_after_late_failure() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("sqlite pool should connect");
    run_sqlite_migrations(&pool)
        .await
        .expect("sqlite migrations should run");
    let encoded = encode_jsonl(&[
        DataExportRecord::manifest(DataExportManifest::new(
            1_700_000_000,
            Some(DatabaseDriver::Postgres),
            vec![ExportDomain::GlobalModels],
        )),
        DataExportRecord::row(
            ExportDomain::GlobalModels,
            "rollback-valid",
            json!({
                "id": "rollback-valid",
                "name": "rollback-valid",
                "created_at": 1,
                "updated_at": 1
            }),
        ),
        DataExportRecord::row(
            ExportDomain::GlobalModels,
            "rollback-invalid",
            json!({
                "id": "rollback-invalid",
                "name": "rollback-invalid",
                "created_at": "invalid-timestamp",
                "updated_at": 1
            }),
        ),
    ])
    .expect("rollback fixture should encode");

    import_sqlite_jsonl(&pool, &encoded)
        .await
        .expect_err("late invalid row should fail the import");
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM global_models WHERE id LIKE 'rollback-%'")
            .fetch_one(&pool)
            .await
            .expect("rolled back row count should load");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn sqlite_core_export_reads_migrated_database_rows() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("sqlite pool should connect");
    run_sqlite_migrations(&pool)
        .await
        .expect("sqlite migrations should run");

    sqlx::query(
            r#"
INSERT INTO users (id, email, username, auth_source, created_at, updated_at)
VALUES ('user-1', 'owner@example.com', 'owner', 'local', '1970-01-01T00:00:01Z', '1970-01-01T00:00:02Z');
INSERT INTO user_groups (id, name, normalized_name, description, priority, allowed_models, allowed_models_mode, created_at, updated_at)
VALUES ('group-1', 'Export Group', 'export group', 'Exported group', 10, '["gpt-test"]', 'specific', '1970-01-01T00:00:01Z', '1970-01-01T00:00:02Z');
INSERT INTO user_group_members (group_id, user_id, created_at)
VALUES ('group-1', 'user-1', '1970-01-01T00:00:01Z');
INSERT INTO api_keys (id, user_id, key_hash, key_encrypted, name, created_at, updated_at)
VALUES ('api-key-1', 'user-1', 'hash-1', 'ciphertext-1', 'Default', '1970-01-01T00:00:01Z', '1970-01-01T00:00:02Z');
INSERT INTO providers (id, name, provider_type, created_at, updated_at)
VALUES ('provider-1', 'Provider One', 'openai', '1970-01-01T00:00:01Z', '1970-01-01T00:00:02Z');
INSERT INTO provider_api_keys (id, provider_id, name, encrypted_key, created_at, updated_at)
VALUES ('provider-key-1', 'provider-1', 'Provider Key', 'ciphertext-provider', '1970-01-01T00:00:01Z', '1970-01-01T00:00:02Z');
INSERT INTO provider_endpoints (id, provider_id, name, base_url, created_at, updated_at)
VALUES ('endpoint-1', 'provider-1', 'Primary', 'https://example.test', '1970-01-01T00:00:01Z', '1970-01-01T00:00:02Z');
INSERT INTO global_models (id, name, created_at, updated_at)
VALUES ('global-model-1', 'gpt-test', '1970-01-01T00:00:01Z', '1970-01-01 00:00:02.123456');
INSERT INTO models (id, provider_id, global_model_id, provider_model_name, created_at, updated_at)
VALUES ('model-1', 'provider-1', 'global-model-1', 'gpt-test', '1970-01-01T00:00:01Z', '1970-01-01T00:00:02Z');
INSERT INTO billing_rules (id, global_model_id, name, task_type, expression, variables, dimension_mappings, is_enabled, created_at, updated_at)
VALUES ('billing-rule-1', 'global-model-1', 'Rule One', 'chat', 'input_tokens * 0.01', '{}', '{"input":"input_tokens"}', 1, '1970-01-01T00:00:01Z', '1970-01-01T00:00:02Z');
INSERT INTO dimension_collectors (id, api_format, task_type, dimension_name, source_type, value_type, transform_expression, priority, is_enabled, created_at, updated_at)
VALUES ('collector-1', 'openai', 'chat', 'input_tokens', 'computed', 'float', 'usage.input_tokens', 10, 1, '1970-01-01T00:00:01Z', '1970-01-01T00:00:02Z');
INSERT INTO system_configs (id, key, value, created_at, updated_at)
VALUES ('config-1', 'billing.enabled', 'true', '1970-01-01T00:00:01Z', '1970-01-01T00:00:02Z');
INSERT INTO wallets (id, user_id, created_at, updated_at)
VALUES ('wallet-1', 'user-1', '1970-01-01T00:00:01Z', '1970-01-01T00:00:02Z');
INSERT INTO "usage" (request_id, id, user_id, provider_name, model, status, billing_status, created_at_unix_ms, updated_at_unix_secs)
VALUES ('request-1', 'request-1', 'user-1', 'Provider One', 'gpt-test', 'completed', 'settled', '1970-01-01T00:00:01.234900Z', 2);
INSERT INTO audit_logs (id, event_type, description, request_id, created_at)
VALUES ('audit-1', 'request.completed', 'Exported audit', 'request-1', '1970-01-01T00:00:02Z');
INSERT INTO usage_body_blobs (body_ref, request_id, body_field, payload_gzip, created_at, updated_at)
VALUES ('body-ref-1', 'request-1', 'request', X'00117FFF', '1970-01-01T00:00:01Z', '1970-01-01T00:00:02Z');
INSERT INTO usage_http_audits (request_id, request_body_ref, request_body_state, body_capture_mode, created_at, updated_at)
VALUES ('request-1', 'body-ref-1', 'captured', 'full', '1970-01-01T00:00:01Z', '1970-01-01T00:00:02Z');
INSERT INTO usage_routing_snapshots (
  request_id, candidate_id, candidate_index, selected_provider_id,
  selected_endpoint_id, selected_provider_api_key_id, created_at, updated_at
)
VALUES (
  'request-1', 'candidate-1', 2, 'provider-1',
  'endpoint-1', 'provider-key-1', '1970-01-01T00:00:01Z', '1970-01-01T00:00:02Z'
);
"#,
        )
        .execute(&pool)
        .await
        .expect("sqlite export rows should seed");

    let encoded = export_sqlite_core_jsonl(&pool, 1_700_000_000)
        .await
        .expect("sqlite export should encode");
    let import_plan = build_import_plan(&encoded).expect("sqlite export should decode");

    assert_eq!(
        import_plan.manifest.source_driver,
        Some(DatabaseDriver::Sqlite)
    );
    assert_eq!(import_plan.manifest.domains, sqlite_core_export_domains());
    assert_eq!(
        import_plan.rows(ExportDomain::Users)[0].payload["email"],
        "owner@example.com"
    );
    assert!(import_plan
        .rows(ExportDomain::UserGroups)
        .iter()
        .any(|row| row.id == "group-1" && row.payload["name"] == "Export Group"));
    assert!(import_plan
        .rows(ExportDomain::UserGroupMembers)
        .iter()
        .any(|row| row.id == "group-1:user-1"
            && row.payload["group_id"] == "group-1"
            && row.payload["user_id"] == "user-1"));
    assert_eq!(
        import_plan.rows(ExportDomain::ApiKeys)[0].payload["key_encrypted"],
        "ciphertext-1"
    );
    assert_eq!(
        import_plan.rows(ExportDomain::ProviderKeys)[0].payload["encrypted_key"],
        "ciphertext-provider"
    );
    assert_eq!(import_plan.rows(ExportDomain::Usage)[0].id, "request-1");
    assert_eq!(import_plan.rows(ExportDomain::Billing).len(), 2);
    assert_eq!(
        import_plan.rows(ExportDomain::Billing)[0].payload["__table"],
        "billing_rules"
    );
    assert_eq!(
        import_plan.rows(ExportDomain::Billing)[0].payload["dimension_mappings"]["input"],
        "input_tokens"
    );
    assert!(import_plan
        .rows(ExportDomain::Auxiliary)
        .iter()
        .any(|row| row.payload["__table"] == "audit_logs" && row.payload["id"] == "audit-1"));
    assert!(import_plan
        .rows(ExportDomain::Auxiliary)
        .iter()
        .any(|row| row.payload["__table"] == "usage_body_blobs"
            && row.payload["payload_gzip"] == json!([0, 17, 127, 255])));
    assert!(import_plan
        .rows(ExportDomain::Auxiliary)
        .iter()
        .any(|row| row.payload["__table"] == "usage_routing_snapshots"
            && row.payload["candidate_id"] == "candidate-1"
            && row.payload["selected_provider_id"] == "provider-1"));

    let target_pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("target sqlite pool should connect");
    run_sqlite_migrations(&target_pool)
        .await
        .expect("target sqlite migrations should run");
    let imported = import_sqlite_jsonl(&target_pool, &encoded)
        .await
        .expect("sqlite import should load exported rows");
    assert_eq!(imported, 20);

    let imported_api_key =
        sqlx::query_as::<_, (String,)>("SELECT key_encrypted FROM api_keys WHERE id = 'api-key-1'")
            .fetch_one(&target_pool)
            .await
            .expect("imported api key should load");
    assert_eq!(imported_api_key.0, "ciphertext-1");

    let imported_usage = sqlx::query_as::<_, (String, i64, String)>(
        "SELECT request_id, created_at_unix_ms, typeof(created_at_unix_ms) FROM \"usage\" WHERE request_id = 'request-1'",
    )
    .fetch_one(&target_pool)
    .await
    .expect("imported usage should load");
    assert_eq!(
        imported_usage,
        ("request-1".to_string(), 1, "integer".to_string())
    );

    let imported_global_model_timestamps = sqlx::query_as::<_, (i64, i64, String, String)>(
        r#"
SELECT created_at, updated_at, typeof(created_at), typeof(updated_at)
FROM global_models
WHERE id = 'global-model-1'
"#,
    )
    .fetch_one(&target_pool)
    .await
    .expect("imported global model timestamps should decode as integers");
    assert_eq!(
        imported_global_model_timestamps,
        (1, 2, "integer".to_string(), "integer".to_string())
    );

    let imported_group_member = sqlx::query_as::<_, (String, String)>(
            "SELECT group_id, user_id FROM user_group_members WHERE group_id = 'group-1' AND user_id = 'user-1'",
        )
        .fetch_one(&target_pool)
        .await
        .expect("imported user group member should load");
    assert_eq!(imported_group_member.0, "group-1");
    assert_eq!(imported_group_member.1, "user-1");

    let imported_billing_rule = sqlx::query_as::<_, (String,)>(
        "SELECT expression FROM billing_rules WHERE id = 'billing-rule-1'",
    )
    .fetch_one(&target_pool)
    .await
    .expect("imported billing rule should load");
    assert_eq!(imported_billing_rule.0, "input_tokens * 0.01");

    let imported_body: Vec<u8> = sqlx::query_scalar(
        "SELECT payload_gzip FROM usage_body_blobs WHERE body_ref = 'body-ref-1'",
    )
    .fetch_one(&target_pool)
    .await
    .expect("imported body blob should load");
    assert_eq!(imported_body, vec![0, 17, 127, 255]);

    let imported_routing = sqlx::query_as::<_, (String, i64, String)>(
        r#"
SELECT candidate_id, candidate_index, selected_provider_id
FROM usage_routing_snapshots
WHERE request_id = 'request-1'
"#,
    )
    .fetch_one(&target_pool)
    .await
    .expect("imported routing snapshot should load");
    assert_eq!(
        imported_routing,
        ("candidate-1".to_string(), 2, "provider-1".to_string())
    );

    if let Some(database_url) = std::env::var("AETHER_TEST_POSTGRES_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        let config = PostgresPoolConfig {
            database_url,
            min_connections: 1,
            max_connections: 1,
            acquire_timeout_ms: 1_000,
            idle_timeout_ms: 5_000,
            max_lifetime_ms: 30_000,
            statement_cache_capacity: 64,
            require_ssl: false,
        };
        let postgres_pool = PostgresPoolFactory::new(config)
            .expect("postgres factory should build")
            .connect_lazy()
            .expect("postgres pool should build");
        run_postgres_migrations(&postgres_pool)
            .await
            .expect("postgres migrations should run");

        let imported = import_postgres_jsonl(&postgres_pool, &encoded)
            .await
            .expect("postgres import should load exported rows");
        assert_eq!(imported, 20);

        let imported_api_key = sqlx::query_as::<_, (String,)>(
            "SELECT key_encrypted FROM api_keys WHERE id = 'api-key-1'",
        )
        .fetch_one(&postgres_pool)
        .await
        .expect("imported postgres api key should load");
        assert_eq!(imported_api_key.0, "ciphertext-1");
    }
}

#[tokio::test]
async fn postgres_core_export_reads_migrated_database_rows_when_url_is_set() {
    let Some(database_url) = std::env::var("AETHER_TEST_POSTGRES_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        eprintln!(
            "skipping postgres core export smoke test because AETHER_TEST_POSTGRES_URL is unset"
        );
        return;
    };

    let config = PostgresPoolConfig {
        database_url,
        min_connections: 1,
        max_connections: 1,
        acquire_timeout_ms: 1_000,
        idle_timeout_ms: 5_000,
        max_lifetime_ms: 30_000,
        statement_cache_capacity: 64,
        require_ssl: false,
    };
    let pool = PostgresPoolFactory::new(config)
        .expect("postgres factory should build")
        .connect_lazy()
        .expect("postgres pool should build");
    run_postgres_migrations(&pool)
        .await
        .expect("postgres migrations should run");

    let suffix = unique_suffix();
    let user_id = format!("export-user-{suffix}");
    let api_key_id = format!("export-api-key-{suffix}");
    let provider_id = format!("export-provider-{suffix}");
    let provider_key_id = format!("export-provider-key-{suffix}");
    let endpoint_id = format!("export-endpoint-{suffix}");
    let global_model_id = format!("export-global-model-{suffix}");
    let model_id = format!("export-model-{suffix}");
    let billing_rule_id = format!("export-billing-rule-{suffix}");
    let collector_id = format!("export-collector-{suffix}");
    let config_id = format!("export-config-{suffix}");
    let config_key = format!("export.config.{suffix}");
    let wallet_id = format!("export-wallet-{suffix}");
    let request_id = format!("export-request-{suffix}");
    let group_id = format!("export-group-{suffix}");

    sqlx::query(
            "INSERT INTO users (id, email, username, auth_source, email_verified, created_at, updated_at) VALUES ($1, $2, $3, 'local', TRUE, to_timestamp(1), to_timestamp(2))",
        )
        .bind(&user_id)
        .bind(format!("{user_id}@example.com"))
        .bind(format!("owner-{suffix}"))
        .execute(&pool)
        .await
        .expect("user should seed");
    sqlx::query(
            "INSERT INTO user_groups (id, name, normalized_name, priority, allowed_models, allowed_models_mode, created_at, updated_at) VALUES ($1, $2, $3, 10, '[\"provider-model\"]', 'specific', to_timestamp(1), to_timestamp(2))",
        )
        .bind(&group_id)
        .bind(format!("Export Group {suffix}"))
        .bind(format!("export group {suffix}"))
        .execute(&pool)
        .await
        .expect("user group should seed");
    sqlx::query(
            "INSERT INTO user_group_members (group_id, user_id, created_at) VALUES ($1, $2, to_timestamp(1))",
        )
        .bind(&group_id)
        .bind(&user_id)
        .execute(&pool)
        .await
        .expect("user group member should seed");
    sqlx::query(
            "INSERT INTO api_keys (id, user_id, key_hash, key_encrypted, name, created_at, updated_at) VALUES ($1, $2, $3, 'ciphertext-1', 'Default', to_timestamp(1), to_timestamp(2))",
        )
        .bind(&api_key_id)
        .bind(&user_id)
        .bind(format!("hash-{api_key_id}"))
        .execute(&pool)
        .await
        .expect("api key should seed");
    sqlx::query(
            "INSERT INTO providers (id, name, provider_type, created_at, updated_at) VALUES ($1, $2, 'openai', to_timestamp(1), to_timestamp(2))",
        )
        .bind(&provider_id)
        .bind(format!("Provider {suffix}"))
        .execute(&pool)
        .await
        .expect("provider should seed");
    sqlx::query(
            "INSERT INTO provider_api_keys (id, provider_id, name, encrypted_key, total_tokens, total_cost_usd, created_at, updated_at) VALUES ($1, $2, 'Provider Key', 'ciphertext-provider', 0, 0, to_timestamp(1), to_timestamp(2))",
        )
        .bind(&provider_key_id)
        .bind(&provider_id)
        .execute(&pool)
        .await
        .expect("provider key should seed");
    sqlx::query(
            "INSERT INTO provider_endpoints (id, provider_id, name, base_url, created_at, updated_at) VALUES ($1, $2, 'Primary', 'https://example.test', to_timestamp(1), to_timestamp(2))",
        )
        .bind(&endpoint_id)
        .bind(&provider_id)
        .execute(&pool)
        .await
        .expect("endpoint should seed");
    sqlx::query(
            "INSERT INTO global_models (id, name, created_at, updated_at) VALUES ($1, $2, to_timestamp(1), to_timestamp(2))",
        )
        .bind(&global_model_id)
        .bind(format!("global-model-{suffix}"))
        .execute(&pool)
        .await
        .expect("global model should seed");
    sqlx::query(
            "INSERT INTO models (id, provider_id, global_model_id, provider_model_name, created_at, updated_at) VALUES ($1, $2, $3, 'provider-model', to_timestamp(1), to_timestamp(2))",
        )
        .bind(&model_id)
        .bind(&provider_id)
        .bind(&global_model_id)
        .execute(&pool)
        .await
        .expect("model should seed");
    sqlx::query(
            "INSERT INTO billing_rules (id, global_model_id, name, task_type, expression, variables, dimension_mappings, is_enabled, created_at, updated_at) VALUES ($1, $2, 'Rule One', 'chat', 'input_tokens * 0.01', '{}', '{\"input\":\"input_tokens\"}', TRUE, to_timestamp(1), to_timestamp(2))",
        )
        .bind(&billing_rule_id)
        .bind(&global_model_id)
        .execute(&pool)
        .await
        .expect("billing rule should seed");
    sqlx::query(
            "INSERT INTO dimension_collectors (id, api_format, task_type, dimension_name, source_type, value_type, transform_expression, priority, is_enabled, created_at, updated_at) VALUES ($1, 'openai', 'chat', $2, 'computed', 'float', 'usage.input_tokens', 10, TRUE, to_timestamp(1), to_timestamp(2))",
        )
        .bind(&collector_id)
        .bind(format!("input_tokens_{suffix}"))
        .execute(&pool)
        .await
        .expect("dimension collector should seed");
    sqlx::query(
            "INSERT INTO system_configs (id, key, value, created_at, updated_at) VALUES ($1, $2, 'true', to_timestamp(1), to_timestamp(2))",
        )
        .bind(&config_id)
        .bind(&config_key)
        .execute(&pool)
        .await
        .expect("system config should seed");
    sqlx::query(
            "INSERT INTO wallets (id, user_id, created_at, updated_at) VALUES ($1, $2, to_timestamp(1), to_timestamp(2))",
        )
        .bind(&wallet_id)
        .bind(&user_id)
        .execute(&pool)
        .await
        .expect("wallet should seed");
    sqlx::query(
            "INSERT INTO \"usage\" (request_id, id, user_id, provider_name, model, status, billing_status, created_at_unix_ms, updated_at_unix_secs) VALUES ($1, $2, $3, 'Provider One', 'provider-model', 'completed', 'settled', 1, 2)",
        )
        .bind(&request_id)
        .bind(&request_id)
        .bind(&user_id)
        .execute(&pool)
        .await
        .expect("usage should seed");

    let encoded = export_postgres_core_jsonl(&pool, 1_700_000_000)
        .await
        .expect("postgres export should encode");
    let import_plan = build_import_plan(&encoded).expect("postgres export should decode");

    assert_eq!(
        import_plan.manifest.source_driver,
        Some(DatabaseDriver::Postgres)
    );
    assert_eq!(import_plan.manifest.domains, postgres_core_export_domains());
    assert!(import_plan
        .rows(ExportDomain::Users)
        .iter()
        .any(|row| row.id == user_id));
    assert!(import_plan
        .rows(ExportDomain::UserGroups)
        .iter()
        .any(|row| row.id == group_id));
    assert!(import_plan
        .rows(ExportDomain::UserGroupMembers)
        .iter()
        .any(|row| row.id == format!("{group_id}:{user_id}")));
    assert!(import_plan
        .rows(ExportDomain::ApiKeys)
        .iter()
        .any(|row| row.id == api_key_id && row.payload["key_encrypted"] == "ciphertext-1"));
    assert!(import_plan
        .rows(ExportDomain::ProviderKeys)
        .iter()
        .any(|row| {
            row.id == provider_key_id && row.payload["encrypted_key"] == "ciphertext-provider"
        }));
    assert!(import_plan
        .rows(ExportDomain::GlobalModels)
        .iter()
        .any(|row| row.id == global_model_id));
    assert!(import_plan
        .rows(ExportDomain::Models)
        .iter()
        .any(|row| row.id == model_id));
    assert!(import_plan
        .rows(ExportDomain::Usage)
        .iter()
        .any(|row| row.id == request_id));

    let target_pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("target sqlite pool should connect");
    run_sqlite_migrations(&target_pool)
        .await
        .expect("target sqlite migrations should run");
    let imported = import_sqlite_jsonl(&target_pool, &encoded)
        .await
        .expect("sqlite import should load postgres exported rows");
    assert_eq!(imported, import_plan_row_count(&import_plan));

    let imported_api_key =
        sqlx::query_as::<_, (String,)>("SELECT key_encrypted FROM api_keys WHERE id = $1")
            .bind(&api_key_id)
            .fetch_one(&target_pool)
            .await
            .expect("imported sqlite api key should load");
    assert_eq!(imported_api_key.0, "ciphertext-1");
    let imported_global_model_timestamps = sqlx::query_as::<_, (i64, i64, String, String)>(
        "SELECT created_at, updated_at, typeof(created_at), typeof(updated_at) FROM global_models WHERE id = ?",
    )
    .bind(&global_model_id)
    .fetch_one(&target_pool)
    .await
    .expect("imported sqlite global model timestamps should decode as integers");
    assert_eq!(
        imported_global_model_timestamps,
        (1, 2, "integer".to_string(), "integer".to_string())
    );
    let imported_group_member = sqlx::query_as::<_, (String, String)>(
        "SELECT group_id, user_id FROM user_group_members WHERE group_id = ? AND user_id = ?",
    )
    .bind(&group_id)
    .bind(&user_id)
    .fetch_one(&target_pool)
    .await
    .expect("imported sqlite user group member should load");
    assert_eq!(imported_group_member.0, group_id);
    assert_eq!(imported_group_member.1, user_id);
}

fn unique_suffix() -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{:016x}", nanos ^ counter.rotate_left(17))
}

fn import_plan_row_count(plan: &DataImportPlan) -> usize {
    plan.manifest
        .domains
        .iter()
        .map(|domain| plan.rows(*domain).len())
        .sum()
}
