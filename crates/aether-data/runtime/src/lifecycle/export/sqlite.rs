use super::*;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SqliteImportColumns {
    names: ImportColumnNames,
    declared_types: BTreeMap<String, String>,
    primary_key: Vec<String>,
}

pub async fn export_sqlite_core_jsonl(
    pool: &aether_data_sqlite::SqlitePool,
    created_at_unix_secs: u64,
) -> Result<String, DataLayerError> {
    export_sqlite_jsonl(pool, sqlite_core_export_domains(), created_at_unix_secs).await
}

pub async fn export_sqlite_jsonl(
    pool: &aether_data_sqlite::SqlitePool,
    domains: Vec<ExportDomain>,
    created_at_unix_secs: u64,
) -> Result<String, DataLayerError> {
    let mut tx = pool.begin().await.map_sql_err()?;
    let manifest = DataExportManifest::new(
        created_at_unix_secs,
        Some(DatabaseDriver::Sqlite),
        domains.clone(),
    );
    let mut records = vec![DataExportRecord::manifest(manifest)];

    for domain in domains {
        if domain == ExportDomain::Auxiliary {
            export_sqlite_auxiliary_records(&mut tx, &mut records).await?;
            continue;
        }
        if domain == ExportDomain::Billing {
            export_sqlite_billing_records(&mut tx, &mut records).await?;
            continue;
        }
        if domain == ExportDomain::Wallets {
            export_sqlite_wallet_records(&mut tx, &mut records).await?;
            continue;
        }
        let (table_name, id_column) = sqlite_domain_table(domain)?;
        let order_by = export_order_by(domain, id_column);
        let sql = format!("SELECT * FROM {table_name} ORDER BY {order_by}");
        let rows = sqlx::query(&sql).fetch_all(&mut *tx).await.map_sql_err()?;
        for row in rows {
            let id = sqlite_export_row_id(domain, &row, id_column)?;
            records.push(DataExportRecord::row(domain, id, sqlite_row_payload(&row)?));
        }
    }

    tx.commit().await.map_sql_err()?;
    encode_jsonl(&records)
}

pub async fn import_sqlite_jsonl(
    pool: &aether_data_sqlite::SqlitePool,
    input: &str,
) -> Result<usize, DataLayerError> {
    let plan = build_import_plan(input)?;
    import_sqlite_plan(pool, &plan).await
}

pub async fn import_sqlite_plan(
    pool: &aether_data_sqlite::SqlitePool,
    plan: &DataImportPlan,
) -> Result<usize, DataLayerError> {
    let mut tx = pool.begin().await.map_sql_err()?;
    let mut imported = 0usize;
    let mut column_cache = BTreeMap::<String, SqliteImportColumns>::new();
    for domain in &plan.manifest.domains {
        if *domain == ExportDomain::Auxiliary {
            for row in plan.rows(*domain) {
                import_sqlite_auxiliary_row(&mut tx, row, &mut column_cache).await?;
                imported = imported.saturating_add(1);
            }
            continue;
        }
        if *domain == ExportDomain::Billing {
            for row in plan.rows(*domain) {
                import_sqlite_billing_row(&mut tx, row, &mut column_cache).await?;
                imported = imported.saturating_add(1);
            }
            continue;
        }
        if *domain == ExportDomain::Wallets {
            for row in plan.rows(*domain) {
                import_sqlite_wallet_row(&mut tx, row, &mut column_cache).await?;
                imported = imported.saturating_add(1);
            }
            continue;
        }
        let (table_name, _id_column) = sqlite_domain_table(*domain)?;
        let target_columns =
            sqlite_import_columns_cached(&mut tx, &mut column_cache, table_name).await?;
        for row in plan.rows(*domain) {
            import_sqlite_row(&mut tx, table_name, *domain, row, &target_columns).await?;
            imported = imported.saturating_add(1);
        }
    }
    tx.commit().await.map_sql_err()?;
    Ok(imported)
}

fn sqlite_domain_table(
    domain: ExportDomain,
) -> Result<(&'static str, &'static str), DataLayerError> {
    match domain {
        ExportDomain::Users => Ok(("users", "id")),
        ExportDomain::ApiKeys => Ok(("api_keys", "id")),
        ExportDomain::Providers => Ok(("providers", "id")),
        ExportDomain::ProviderKeys => Ok(("provider_api_keys", "id")),
        ExportDomain::Endpoints => Ok(("provider_endpoints", "id")),
        ExportDomain::Models => Ok(("models", "id")),
        ExportDomain::GlobalModels => Ok(("global_models", "id")),
        ExportDomain::AuthModules => Ok(("auth_modules", "id")),
        ExportDomain::OAuthProviders => Ok(("oauth_providers", "provider_type")),
        ExportDomain::UserOAuthLinks => Ok(("user_oauth_links", "id")),
        ExportDomain::UserGroups => Ok(("user_groups", "id")),
        ExportDomain::UserGroupMembers => Ok(("user_group_members", "group_id")),
        ExportDomain::SystemConfigs => Ok(("system_configs", "id")),
        ExportDomain::Wallets => Err(DataLayerError::InvalidInput(
            "sqlite wallet export uses multiple tables and must be handled as a domain".to_string(),
        )),
        ExportDomain::Usage => Ok((r#""usage""#, "request_id")),
        ExportDomain::Billing => Err(DataLayerError::InvalidInput(
            "sqlite billing export uses multiple tables and must be handled as a domain"
                .to_string(),
        )),
        ExportDomain::Auxiliary => Err(DataLayerError::InvalidInput(
            "sqlite auxiliary export uses multiple tables and must be handled as a domain"
                .to_string(),
        )),
    }
}

async fn export_sqlite_auxiliary_records(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    records: &mut Vec<DataExportRecord>,
) -> Result<(), DataLayerError> {
    for table in AUXILIARY_TABLES {
        let table_sql = sqlite_quote_identifier(table.name)?;
        let order_sql = table
            .primary_key
            .iter()
            .map(|column| sqlite_quote_identifier(column).map(|column| format!("{column} ASC")))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let rows = sqlx::query(&format!("SELECT * FROM {table_sql} ORDER BY {order_sql}"))
            .fetch_all(&mut **tx)
            .await
            .map_sql_err()?;
        for row in rows {
            let payload = sqlite_row_payload(&row)?;
            let id = auxiliary_row_id(*table, &payload)?;
            records.push(DataExportRecord::row(
                ExportDomain::Auxiliary,
                id,
                payload_with_table(payload, table.name)?,
            ));
        }
    }
    Ok(())
}

fn sqlite_export_row_id(
    domain: ExportDomain,
    row: &sqlx::sqlite::SqliteRow,
    id_column: &str,
) -> Result<String, DataLayerError> {
    if domain == ExportDomain::UserGroupMembers {
        let group_id = sqlite_required_export_text(row, "group_id", domain)?;
        let user_id = sqlite_required_export_text(row, "user_id", domain)?;
        return Ok(format!("{group_id}:{user_id}"));
    }
    sqlite_required_export_text(row, id_column, domain)
}

fn sqlite_required_export_text(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
    domain: ExportDomain,
) -> Result<String, DataLayerError> {
    row.try_get::<Option<String>, _>(column)
        .map_sql_err()?
        .ok_or_else(|| {
            DataLayerError::UnexpectedValue(format!(
                "{} export row has null id column '{}'",
                domain.as_str(),
                column
            ))
        })
}

async fn export_sqlite_billing_records(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    records: &mut Vec<DataExportRecord>,
) -> Result<(), DataLayerError> {
    for table_name in [
        "billing_rules",
        "dimension_collectors",
        "usage_settlement_snapshots",
    ] {
        let id_column = if table_name == "usage_settlement_snapshots" {
            "request_id"
        } else {
            "id"
        };
        let sql = format!("SELECT * FROM {table_name} ORDER BY {id_column} ASC");
        let rows = sqlx::query(&sql).fetch_all(&mut **tx).await.map_sql_err()?;
        for row in rows {
            let id = row
                .try_get::<Option<String>, _>(id_column)
                .map_sql_err()?
                .ok_or_else(|| {
                    DataLayerError::UnexpectedValue(format!(
                        "billing export row in table '{table_name}' has null id"
                    ))
                })?;
            records.push(DataExportRecord::row(
                ExportDomain::Billing,
                format!("{table_name}:{id}"),
                payload_with_table(sqlite_row_payload(&row)?, table_name)?,
            ));
        }
    }
    Ok(())
}

async fn export_sqlite_wallet_records(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    records: &mut Vec<DataExportRecord>,
) -> Result<(), DataLayerError> {
    for (table_name, id_column) in sqlite_wallet_tables() {
        let sql = format!("SELECT * FROM {table_name} ORDER BY {id_column} ASC");
        let rows = sqlx::query(&sql).fetch_all(&mut **tx).await.map_sql_err()?;
        for row in rows {
            let id = row
                .try_get::<Option<String>, _>(id_column)
                .map_sql_err()?
                .ok_or_else(|| {
                    DataLayerError::UnexpectedValue(format!(
                        "wallet export row in table '{table_name}' has null id"
                    ))
                })?;
            records.push(DataExportRecord::row(
                ExportDomain::Wallets,
                format!("{table_name}:{id}"),
                payload_with_table(sqlite_row_payload(&row)?, table_name)?,
            ));
        }
    }
    Ok(())
}

async fn import_sqlite_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    table_name: &str,
    domain: ExportDomain,
    row: &ExportRow,
    target_columns: &SqliteImportColumns,
) -> Result<(), DataLayerError> {
    let object = filter_import_payload("sqlite", table_name, domain, row, &target_columns.names)?;

    let columns = object.keys().map(String::as_str).collect::<Vec<_>>();
    let column_sql = columns
        .iter()
        .map(|column| sqlite_quote_identifier(column))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let placeholder_sql = vec!["?"; columns.len()].join(", ");
    let conflict_columns = target_columns
        .primary_key
        .iter()
        .map(|column| sqlite_quote_identifier(column))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let update_sql = columns
        .iter()
        .filter(|column| !target_columns.primary_key.iter().any(|key| key == *column))
        .map(|column| {
            let quoted = sqlite_quote_identifier(column)?;
            Ok(format!("{quoted} = excluded.{quoted}"))
        })
        .collect::<Result<Vec<_>, DataLayerError>>()?
        .join(", ");
    let conflict_sql = if update_sql.is_empty() {
        format!("ON CONFLICT ({conflict_columns}) DO NOTHING")
    } else {
        format!("ON CONFLICT ({conflict_columns}) DO UPDATE SET {update_sql}")
    };
    let sql = format!(
        "INSERT INTO {table_name} ({column_sql}) VALUES ({placeholder_sql}) {conflict_sql}"
    );
    let mut query = sqlx::query(&sql);
    for column in columns {
        let value = object
            .get(column)
            .expect("column name came from payload object keys");
        let declared_type = target_columns
            .declared_types
            .get(column)
            .map(String::as_str)
            .unwrap_or_default();
        query = bind_sqlite_import_value(query, value, table_name, column, declared_type)?;
    }
    query.execute(&mut **tx).await.map_sql_err()?;
    Ok(())
}

async fn import_sqlite_billing_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    row: &ExportRow,
    column_cache: &mut BTreeMap<String, SqliteImportColumns>,
) -> Result<(), DataLayerError> {
    let (table_name, payload) = billing_payload_table(row)?;
    let table_name = sqlite_billing_table_name(&table_name)?;
    let target_columns = sqlite_import_columns_cached(tx, column_cache, table_name).await?;
    import_sqlite_row(
        tx,
        table_name,
        ExportDomain::Billing,
        &ExportRow {
            id: row.id.clone(),
            payload,
        },
        &target_columns,
    )
    .await
}

async fn import_sqlite_auxiliary_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    row: &ExportRow,
    column_cache: &mut BTreeMap<String, SqliteImportColumns>,
) -> Result<(), DataLayerError> {
    let (table_name, payload) = domain_payload_table(row, "auxiliary", None)?;
    let table = auxiliary_table(&table_name)?;
    let target_columns = sqlite_import_columns_cached(tx, column_cache, table.name).await?;
    import_sqlite_row(
        tx,
        table.name,
        ExportDomain::Auxiliary,
        &ExportRow {
            id: row.id.clone(),
            payload,
        },
        &target_columns,
    )
    .await
}

fn sqlite_billing_table_name(table_name: &str) -> Result<&'static str, DataLayerError> {
    match table_name {
        "billing_rules" => Ok("billing_rules"),
        "dimension_collectors" => Ok("dimension_collectors"),
        "usage_settlement_snapshots" => Ok("usage_settlement_snapshots"),
        other => Err(DataLayerError::InvalidInput(format!(
            "unsupported sqlite billing export table '{other}'"
        ))),
    }
}

async fn import_sqlite_wallet_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    row: &ExportRow,
    column_cache: &mut BTreeMap<String, SqliteImportColumns>,
) -> Result<(), DataLayerError> {
    let (table_name, payload) = domain_payload_table(row, "wallet", Some("wallets"))?;
    let table_name = sqlite_wallet_table_name(&table_name)?;
    let target_columns = sqlite_import_columns_cached(tx, column_cache, table_name).await?;
    import_sqlite_row(
        tx,
        table_name,
        ExportDomain::Wallets,
        &ExportRow {
            id: row.id.clone(),
            payload,
        },
        &target_columns,
    )
    .await
}

fn sqlite_wallet_tables() -> &'static [(&'static str, &'static str)] {
    &[("wallets", "id")]
}

fn sqlite_wallet_table_name(table_name: &str) -> Result<&'static str, DataLayerError> {
    sqlite_wallet_tables()
        .iter()
        .find(|(candidate, _)| *candidate == table_name)
        .map(|(table, _)| *table)
        .ok_or_else(|| {
            DataLayerError::InvalidInput(format!(
                "unsupported sqlite wallet export table '{table_name}'"
            ))
        })
}

async fn sqlite_import_columns_cached(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    cache: &mut BTreeMap<String, SqliteImportColumns>,
    table_name: &str,
) -> Result<SqliteImportColumns, DataLayerError> {
    if let Some(columns) = cache.get(table_name) {
        return Ok(columns.clone());
    }

    let columns = load_sqlite_import_columns(tx, table_name).await?;
    cache.insert(table_name.to_string(), columns.clone());
    Ok(columns)
}

async fn load_sqlite_import_columns(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    table_name: &str,
) -> Result<SqliteImportColumns, DataLayerError> {
    let sql = format!("PRAGMA table_info({table_name})");
    let rows = sqlx::query(&sql).fetch_all(&mut **tx).await.map_sql_err()?;
    let mut columns = SqliteImportColumns::default();
    let mut primary_key = BTreeMap::new();
    for row in rows {
        let name = row.try_get::<String, _>("name").map_sql_err()?;
        let declared_type = row
            .try_get::<Option<String>, _>("type")
            .map_sql_err()?
            .unwrap_or_default();
        columns.names.insert(name.clone());
        columns.declared_types.insert(name.clone(), declared_type);
        let primary_key_position = row.try_get::<i64, _>("pk").map_sql_err()?;
        if primary_key_position > 0 {
            primary_key.insert(primary_key_position, name);
        }
    }

    if columns.names.is_empty() {
        return Err(DataLayerError::UnexpectedValue(format!(
            "sqlite import target table '{table_name}' has no visible columns"
        )));
    }
    if primary_key.is_empty() {
        return Err(DataLayerError::UnexpectedValue(format!(
            "sqlite import target table '{table_name}' has no primary key"
        )));
    }
    columns.primary_key = primary_key.into_values().collect();

    Ok(columns)
}

fn bind_sqlite_import_value<'q>(
    query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    json_value: &'q Value,
    table_name: &str,
    column_name: &str,
    declared_type: &str,
) -> Result<sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>, DataLayerError>
{
    if declared_type.to_ascii_uppercase().contains("BLOB") {
        return match normalize_imported_binary("sqlite", column_name, json_value)? {
            Some(bytes) => Ok(query.bind(bytes)),
            None => Ok(query.bind(Option::<Vec<u8>>::None)),
        };
    }
    let has_integer_affinity = declared_type.to_ascii_uppercase().contains("INT");
    if !has_integer_affinity || !import_column_stores_timestamp(column_name) {
        return bind_sqlite_json_value(query, json_value);
    }

    match normalize_imported_integer_timestamp("sqlite", table_name, column_name, json_value)? {
        Some(timestamp) => Ok(query.bind(timestamp)),
        None => Ok(query.bind(Option::<i64>::None)),
    }
}

fn sqlite_row_payload(row: &sqlx::sqlite::SqliteRow) -> Result<Value, DataLayerError> {
    let mut object = serde_json::Map::new();
    for (index, column) in row.columns().iter().enumerate() {
        object.insert(
            column.name().to_string(),
            sqlite_value_to_json(row, index, column.name())?,
        );
    }
    Ok(Value::Object(object))
}

fn sqlite_value_to_json(
    row: &sqlx::sqlite::SqliteRow,
    index: usize,
    column_name: &str,
) -> Result<Value, DataLayerError> {
    let raw = row.try_get_raw(index).map_sql_err()?;
    if raw.is_null() {
        return Ok(Value::Null);
    }

    match raw.type_info().name().to_ascii_uppercase().as_str() {
        "INTEGER" => {
            let value = row.try_get::<i64, _>(index).map_sql_err()?;
            if sqlite_integer_column_is_boolean(column_name) {
                match value {
                    0 => return Ok(Value::Bool(false)),
                    1 => return Ok(Value::Bool(true)),
                    _ => {}
                }
            }
            Ok(Value::from(value))
        }
        "REAL" | "FLOAT" | "DOUBLE" => {
            let value = row.try_get::<f64, _>(index).map_sql_err()?;
            serde_json::Number::from_f64(value)
                .map(Value::Number)
                .ok_or_else(|| {
                    DataLayerError::UnexpectedValue(format!(
                        "sqlite export column {} contains non-finite float",
                        index
                    ))
                })
        }
        "TEXT" => Ok(Value::String(
            row.try_get::<String, _>(index).map_sql_err()?,
        )),
        "BLOB" => {
            let bytes = row.try_get::<Vec<u8>, _>(index).map_sql_err()?;
            Ok(Value::Array(bytes.into_iter().map(Value::from).collect()))
        }
        other => Err(DataLayerError::UnexpectedValue(format!(
            "unsupported sqlite export column type '{other}' at index {index}"
        ))),
    }
}

fn sqlite_integer_column_is_boolean(column_name: &str) -> bool {
    column_name.starts_with("is_")
        || column_name.starts_with("has_")
        || column_name.starts_with("supports_")
        || column_name.starts_with("enable_")
        || column_name.starts_with("use_")
        || matches!(
            column_name,
            "announcement_notifications"
                | "auto_delete_on_expiry"
                | "auto_fetch_models"
                | "email_notifications"
                | "email_verified"
                | "signature_valid"
                | "usage_alerts"
                | "webhook_sent"
        )
}
