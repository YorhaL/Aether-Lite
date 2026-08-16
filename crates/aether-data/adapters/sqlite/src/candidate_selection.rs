use std::collections::BTreeSet;

use async_trait::async_trait;
use sqlx::{sqlite::SqliteRow, QueryBuilder, Row, Sqlite};

use aether_data_contracts::repository::candidate_selection::{
    MinimalCandidateSelectionReadRepository, StoredApiFormatCandidateRowsQuery,
    StoredMinimalCandidateSelectionRow, StoredProviderModelMapping,
    StoredRequestedModelCandidateRowsQuery,
};
use aether_data_contracts::DataLayerError;

use crate::error::SqlResultExt;
use crate::SqlitePool;

const CANDIDATE_SELECTION_COLUMNS: &str = r#"
SELECT
  p.id AS provider_id,
  p.name AS provider_name,
  p.provider_priority AS provider_priority,
  p.is_active AS provider_is_active,
  pe.id AS endpoint_id,
  COALESCE(pe.api_format, '') AS endpoint_api_format,
  pe.api_family AS endpoint_api_family,
  pe.endpoint_kind AS endpoint_kind,
  pe.is_active AS endpoint_is_active,
  pak.id AS key_id,
  pak.name AS key_name,
  pak.auth_type AS key_auth_type,
  pak.is_active AS key_is_active,
  pak.api_formats AS key_api_formats,
  pak.allowed_models AS key_allowed_models,
  pak.capabilities AS key_capabilities,
  pak.internal_priority AS key_internal_priority,
  pak.global_priority_by_format AS key_global_priority_by_format,
  pak.last_used_at AS key_last_used_at_unix_secs,
  m.id AS model_id,
  m.global_model_id AS global_model_id,
  gm.name AS global_model_name,
  gm.config AS global_model_config,
  m.provider_model_name AS model_provider_model_name,
  m.provider_model_mappings AS model_provider_model_mappings,
  m.supports_streaming AS model_supports_streaming,
  m.is_active AS model_is_active,
  m.is_available AS model_is_available
FROM providers p
INNER JOIN provider_endpoints pe ON pe.provider_id = p.id
INNER JOIN provider_api_keys pak ON pak.provider_id = p.id
INNER JOIN models m ON m.provider_id = p.id
INNER JOIN global_models gm ON gm.id = m.global_model_id
WHERE p.is_active = 1
  AND pe.is_active = 1
  AND pak.is_active = 1
  AND m.is_active = 1
  AND m.is_available = 1
  AND gm.is_active = 1
"#;

const REQUESTED_MODEL_RAW_PAGE_SIZE: u32 = 256;
const REQUESTED_MODEL_RAW_SCAN_LIMIT: u32 = 2048;

#[derive(Debug, Clone)]
pub struct SqliteMinimalCandidateSelectionReadRepository {
    pool: SqlitePool,
}

#[derive(Debug, Clone)]
struct CandidateSelectionRow {
    row: StoredMinimalCandidateSelectionRow,
}

#[derive(Debug, Clone, Copy)]
enum SelectedRowsOrder {
    WithGlobalModel,
    WithoutGlobalModel,
}

#[derive(Debug, Clone, Copy)]
enum SelectedRowsFilter<'a> {
    None,
    GlobalModel(&'a str),
    RequestedModel(&'a str),
}

#[derive(Debug, Clone, Copy)]
struct SqlPage {
    limit: i64,
    offset: i64,
}

#[derive(Debug)]
struct ExactPageAccumulator<T> {
    rows: Vec<T>,
    offset: usize,
    limit: usize,
    target_len: usize,
}

impl<T> ExactPageAccumulator<T> {
    fn new(offset: u32, limit: u32) -> Self {
        let offset = usize::try_from(offset).unwrap_or(usize::MAX);
        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        Self {
            rows: Vec::new(),
            offset,
            limit,
            target_len: offset.saturating_add(limit),
        }
    }

    fn is_full(&self) -> bool {
        self.rows.len() >= self.target_len
    }

    fn push_matching<I, F>(&mut self, rows: I, mut predicate: F)
    where
        I: IntoIterator<Item = T>,
        F: FnMut(&T) -> bool,
    {
        let remaining = self.target_len.saturating_sub(self.rows.len());
        self.rows.extend(
            rows.into_iter()
                .filter(|row| predicate(row))
                .take(remaining),
        );
    }

    fn into_page(self) -> Vec<T> {
        self.rows
            .into_iter()
            .skip(self.offset)
            .take(self.limit)
            .collect()
    }
}

#[derive(Debug)]
struct RequestedModelRawPage {
    rows: Vec<CandidateSelectionRow>,
    raw_len: u32,
}

impl SqliteMinimalCandidateSelectionReadRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    async fn selected_rows_for_api_format(
        &self,
        api_format: &str,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        self.load_selected_rows_for_api_format(
            api_format,
            SelectedRowsFilter::None,
            SelectedRowsOrder::WithGlobalModel,
            None,
        )
        .await
    }

    async fn load_selected_rows_for_api_format(
        &self,
        api_format: &str,
        filter: SelectedRowsFilter<'_>,
        order: SelectedRowsOrder,
        page: Option<SqlPage>,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        let canonical_api_format = normalize_api_format(api_format);
        let storage_aliases = api_format_aliases(&canonical_api_format);
        let match_aliases =
            sql_match_aliases(&api_format_permission_aliases(&canonical_api_format));
        let mut rows = Vec::new();

        for storage_api_format in storage_aliases {
            let mut builder = QueryBuilder::<Sqlite>::new("WITH candidate_rows AS (");
            builder.push(CANDIDATE_SELECTION_COLUMNS);
            push_candidate_sql_filters(&mut builder, &storage_api_format, &match_aliases);
            match filter {
                SelectedRowsFilter::None => {}
                SelectedRowsFilter::GlobalModel(global_model_name) => {
                    builder.push(" AND gm.name = ");
                    builder.push_bind(global_model_name);
                }
                SelectedRowsFilter::RequestedModel(requested_model_name) => {
                    push_requested_model_sql_filter(
                        &mut builder,
                        requested_model_name,
                        &match_aliases,
                    );
                }
            }
            push_selected_rows_query_tail(&mut builder, order, page);

            let query_rows = builder.build().fetch_all(&self.pool).await.map_sql_err()?;
            let mut items = query_rows
                .iter()
                .map(map_candidate_selection_row)
                .collect::<Result<Vec<_>, _>>()?;
            items.retain(|item| {
                api_format_matches(&item.row.endpoint_api_format, &canonical_api_format)
                    && item.row.key_supports_api_format(&canonical_api_format)
            });
            rows.extend(items.into_iter().map(|item| item.row));
        }

        let rows = match filter {
            SelectedRowsFilter::RequestedModel(requested_model_name) => rows
                .into_iter()
                .filter(|row| {
                    row_matches_requested_model(row, requested_model_name, &canonical_api_format)
                })
                .collect(),
            _ => rows,
        };
        Ok(dedupe_candidate_selection_rows(rows))
    }

    async fn load_requested_model_raw_page(
        &self,
        api_format: &str,
        requested_model_name: &str,
        page: SqlPage,
    ) -> Result<RequestedModelRawPage, DataLayerError> {
        let canonical_api_format = normalize_api_format(api_format);
        let storage_aliases = api_format_aliases(&canonical_api_format);
        let match_aliases =
            sql_match_aliases(&api_format_permission_aliases(&canonical_api_format));
        let mut builder = QueryBuilder::<Sqlite>::new("WITH candidate_rows AS (");
        builder.push(CANDIDATE_SELECTION_COLUMNS);
        push_candidate_sql_filters_for_aliases(&mut builder, &storage_aliases, &match_aliases);
        push_requested_model_sql_filter(&mut builder, requested_model_name, &match_aliases);
        push_selected_rows_query_tail(&mut builder, SelectedRowsOrder::WithGlobalModel, Some(page));

        let query_rows = builder.build().fetch_all(&self.pool).await.map_sql_err()?;
        let raw_len = u32::try_from(query_rows.len()).unwrap_or(u32::MAX);
        let mut rows = query_rows
            .iter()
            .map(map_candidate_selection_row)
            .collect::<Result<Vec<_>, _>>()?;
        rows.retain(|item| {
            api_format_matches(&item.row.endpoint_api_format, &canonical_api_format)
                && item.row.key_supports_api_format(&canonical_api_format)
        });

        Ok(RequestedModelRawPage { rows, raw_len })
    }
}

#[async_trait]
impl MinimalCandidateSelectionReadRepository for SqliteMinimalCandidateSelectionReadRepository {
    async fn list_for_exact_api_format(
        &self,
        api_format: &str,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        self.selected_rows_for_api_format(api_format).await
    }

    async fn list_for_exact_api_format_page(
        &self,
        query: &StoredApiFormatCandidateRowsQuery,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        if query.limit == 0 {
            return Ok(Vec::new());
        }
        let fetch_limit = query.offset.saturating_add(query.limit);
        let mut rows = self
            .load_selected_rows_for_api_format(
                &query.api_format,
                SelectedRowsFilter::None,
                SelectedRowsOrder::WithGlobalModel,
                Some(SqlPage {
                    limit: i64::from(fetch_limit),
                    offset: 0,
                }),
            )
            .await?;
        sort_candidate_selection_rows(&mut rows, true);
        Ok(rows
            .into_iter()
            .skip(query.offset as usize)
            .take(query.limit as usize)
            .collect())
    }

    async fn list_for_exact_api_format_and_global_model(
        &self,
        api_format: &str,
        global_model_name: &str,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        self.load_selected_rows_for_api_format(
            api_format,
            SelectedRowsFilter::GlobalModel(global_model_name),
            SelectedRowsOrder::WithoutGlobalModel,
            None,
        )
        .await
    }

    async fn list_for_exact_api_format_and_requested_model(
        &self,
        api_format: &str,
        requested_model_name: &str,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        self.load_selected_rows_for_api_format(
            api_format,
            SelectedRowsFilter::RequestedModel(requested_model_name),
            SelectedRowsOrder::WithGlobalModel,
            None,
        )
        .await
    }

    async fn list_for_exact_api_format_and_requested_model_page(
        &self,
        query: &StoredRequestedModelCandidateRowsQuery,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        if query.limit == 0 {
            return Ok(Vec::new());
        }

        let mut exact_page = ExactPageAccumulator::new(query.offset, query.limit);
        let mut raw_offset = 0_u32;
        while !exact_page.is_full() && raw_offset < REQUESTED_MODEL_RAW_SCAN_LIMIT {
            let raw_limit = REQUESTED_MODEL_RAW_PAGE_SIZE
                .min(REQUESTED_MODEL_RAW_SCAN_LIMIT.saturating_sub(raw_offset));
            let raw_page = self
                .load_requested_model_raw_page(
                    &query.api_format,
                    &query.requested_model_name,
                    SqlPage {
                        limit: i64::from(raw_limit),
                        offset: i64::from(raw_offset),
                    },
                )
                .await?;
            exact_page.push_matching(raw_page.rows, |item| {
                row_matches_requested_model(
                    &item.row,
                    &query.requested_model_name,
                    &query.api_format,
                )
            });
            raw_offset = raw_offset.saturating_add(raw_page.raw_len);
            if raw_page.raw_len < raw_limit || raw_page.raw_len == 0 {
                break;
            }
        }

        let rows = exact_page
            .into_page()
            .into_iter()
            .map(|item| item.row)
            .collect();
        Ok(dedupe_candidate_selection_rows(rows))
    }
}

fn push_candidate_sql_filters(
    builder: &mut QueryBuilder<'_, Sqlite>,
    storage_api_format: &str,
    match_aliases: &[String],
) {
    builder.push(" AND LOWER(COALESCE(pe.api_format, '')) = ");
    builder.push_bind(storage_api_format.trim().to_ascii_lowercase());
    push_key_api_format_sql_filter(builder, match_aliases);
}

fn push_candidate_sql_filters_for_aliases(
    builder: &mut QueryBuilder<'_, Sqlite>,
    storage_api_formats: &[String],
    match_aliases: &[String],
) {
    builder.push(" AND LOWER(COALESCE(pe.api_format, '')) IN (");
    push_bind_list(builder, storage_api_formats);
    builder.push(")");
    push_key_api_format_sql_filter(builder, match_aliases);
}

fn push_key_api_format_sql_filter(
    builder: &mut QueryBuilder<'_, Sqlite>,
    match_aliases: &[String],
) {
    builder.push(
        r#"
  AND (
    pak.api_formats IS NULL
    OR TRIM(pak.api_formats) = ''
    OR CASE
      WHEN json_valid(pak.api_formats) THEN
      (
        (
          json_type(pak.api_formats) = 'array'
          AND EXISTS (
            SELECT 1
            FROM json_each(pak.api_formats) AS fmt
            WHERE LOWER(TRIM(CAST(fmt.value AS TEXT))) IN (
"#,
    );
    push_bind_list(builder, match_aliases);
    builder.push(
        r#"
            )
          )
        )
        OR (
          json_type(pak.api_formats) = 'text'
          AND LOWER(TRIM(CAST(json_extract(pak.api_formats, '$') AS TEXT))) IN (
"#,
    );
    push_bind_list(builder, match_aliases);
    builder.push(
        r#"
          )
        )
        OR (
          json_type(pak.api_formats) = 'text'
          AND EXISTS (
            SELECT 1
            FROM json_each(
              CASE
                WHEN json_valid(CAST(json_extract(pak.api_formats, '$') AS TEXT))
                  THEN CAST(json_extract(pak.api_formats, '$') AS TEXT)
                ELSE '[]'
              END
            ) AS fmt
            WHERE LOWER(TRIM(CAST(fmt.value AS TEXT))) IN (
"#,
    );
    push_bind_list(builder, match_aliases);
    builder.push(
        r#"
            )
          )
        )
      )
      ELSE 0
    END
    OR LOWER(TRIM(pak.api_formats)) IN (
"#,
    );
    push_bind_list(builder, match_aliases);
    builder.push(
        r#"
    )
  )
"#,
    );
}

fn push_requested_model_sql_filter(
    builder: &mut QueryBuilder<'_, Sqlite>,
    requested_model_name: &str,
    _match_aliases: &[String],
) {
    builder.push(
        r#"
  AND (
    gm.name = "#,
    );
    builder.push_bind(requested_model_name.to_string());
    builder.push(
        r#"
    OR m.provider_model_name = "#,
    );
    builder.push_bind(requested_model_name.to_string());
    builder.push(
        r#"
    OR (
      m.provider_model_mappings IS NOT NULL
      AND m.provider_model_mappings LIKE "#,
    );
    builder.push_bind(format!(
        "%{}%",
        requested_model_name
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    ));
    builder.push(
        r#"
      ESCAPE '\'
    )
  )
"#,
    );
}

fn push_selected_rows_query_tail(
    builder: &mut QueryBuilder<'_, Sqlite>,
    order: SelectedRowsOrder,
    page: Option<SqlPage>,
) {
    builder.push(
        r#"
)
SELECT * FROM candidate_rows
"#,
    );
    push_selected_rows_order(builder, order);
    if let Some(page) = page {
        builder.push(" LIMIT ");
        builder.push_bind(page.limit);
        builder.push(" OFFSET ");
        builder.push_bind(page.offset);
    }
}

fn push_selected_rows_order(builder: &mut QueryBuilder<'_, Sqlite>, order: SelectedRowsOrder) {
    builder.push(" ORDER BY ");
    if matches!(order, SelectedRowsOrder::WithGlobalModel) {
        builder.push("global_model_name ASC, ");
    }
    builder.push(
        "provider_priority ASC, key_internal_priority ASC, provider_id ASC, endpoint_id ASC, key_id ASC, model_id ASC",
    );
}

fn push_bind_list(builder: &mut QueryBuilder<'_, Sqlite>, values: &[String]) {
    let mut separated = builder.separated(", ");
    for value in values {
        separated.push_bind(value.clone());
    }
}

fn row_matches_requested_model(
    row: &StoredMinimalCandidateSelectionRow,
    requested_model_name: &str,
    api_format: &str,
) -> bool {
    (row_has_available_provider_model(row, api_format)
        && row.global_model_name == requested_model_name)
        || (row_default_provider_model_name_available(row, api_format)
            && row.model_provider_model_name == requested_model_name)
        || row
            .model_provider_model_mappings
            .as_ref()
            .is_some_and(|mappings| {
                mappings.iter().any(|mapping| {
                    mapping_scope_matches(mapping, row, api_format)
                        && mapping.name == requested_model_name
                })
            })
}

fn row_has_available_provider_model(
    row: &StoredMinimalCandidateSelectionRow,
    api_format: &str,
) -> bool {
    row_mapping_matches_scope(row, api_format)
        || row_default_provider_model_name_available(row, api_format)
}

fn row_default_provider_model_name_available(
    row: &StoredMinimalCandidateSelectionRow,
    api_format: &str,
) -> bool {
    let Some(mappings) = row.model_provider_model_mappings.as_ref() else {
        return true;
    };
    let mut has_explicit_default_mapping = false;
    for mapping in mappings {
        if mapping.name != row.model_provider_model_name {
            continue;
        }
        has_explicit_default_mapping = true;
        if mapping_scope_matches(mapping, row, api_format) {
            return true;
        }
    }
    !has_explicit_default_mapping
}

fn row_mapping_matches_scope(row: &StoredMinimalCandidateSelectionRow, api_format: &str) -> bool {
    row.model_provider_model_mappings
        .as_ref()
        .is_some_and(|mappings| {
            mappings
                .iter()
                .any(|mapping| mapping_scope_matches(mapping, row, api_format))
        })
}

fn mapping_scope_matches(
    mapping: &StoredProviderModelMapping,
    row: &StoredMinimalCandidateSelectionRow,
    api_format: &str,
) -> bool {
    mapping.api_formats.as_ref().is_none_or(|formats| {
        formats
            .iter()
            .any(|value| api_format_scope_covers(value, api_format))
    }) && mapping.endpoint_ids.as_ref().is_none_or(|endpoint_ids| {
        endpoint_ids
            .iter()
            .any(|endpoint_id| endpoint_id == &row.endpoint_id)
    })
}

fn dedupe_candidate_selection_rows(
    rows: Vec<StoredMinimalCandidateSelectionRow>,
) -> Vec<StoredMinimalCandidateSelectionRow> {
    let mut seen = BTreeSet::new();
    rows.into_iter()
        .filter(|row| {
            seen.insert((
                row.endpoint_id.clone(),
                row.key_id.clone(),
                row.model_id.clone(),
            ))
        })
        .collect()
}

fn sort_candidate_selection_rows(
    rows: &mut [StoredMinimalCandidateSelectionRow],
    include_global_model: bool,
) {
    rows.sort_by(|left, right| {
        let global_model_order = if include_global_model {
            left.global_model_name.cmp(&right.global_model_name)
        } else {
            std::cmp::Ordering::Equal
        };
        global_model_order
            .then(left.provider_priority.cmp(&right.provider_priority))
            .then(left.key_internal_priority.cmp(&right.key_internal_priority))
            .then(left.provider_id.cmp(&right.provider_id))
            .then(left.endpoint_id.cmp(&right.endpoint_id))
            .then(left.key_id.cmp(&right.key_id))
            .then(left.model_id.cmp(&right.model_id))
    });
}

fn map_candidate_selection_row(row: &SqliteRow) -> Result<CandidateSelectionRow, DataLayerError> {
    let global_model_config = parse_json(row.try_get("global_model_config").ok().flatten())?;
    let global_model_mappings = global_model_config
        .as_ref()
        .and_then(|value| value.get("model_mappings").cloned());
    let global_model_supports_streaming = global_model_config
        .as_ref()
        .and_then(|value| value.get("streaming"))
        .and_then(json_bool);
    Ok(CandidateSelectionRow {
        row: StoredMinimalCandidateSelectionRow {
            provider_id: row.try_get("provider_id").map_sql_err()?,
            provider_name: row.try_get("provider_name").map_sql_err()?,
            provider_priority: row.try_get("provider_priority").map_sql_err()?,
            provider_is_active: row.try_get("provider_is_active").map_sql_err()?,
            endpoint_id: row.try_get("endpoint_id").map_sql_err()?,
            endpoint_api_format: row.try_get("endpoint_api_format").map_sql_err()?,
            endpoint_api_family: row.try_get("endpoint_api_family").map_sql_err()?,
            endpoint_kind: row.try_get("endpoint_kind").map_sql_err()?,
            endpoint_is_active: row.try_get("endpoint_is_active").map_sql_err()?,
            key_id: row.try_get("key_id").map_sql_err()?,
            key_name: row.try_get("key_name").map_sql_err()?,
            key_auth_type: row.try_get("key_auth_type").map_sql_err()?,
            key_is_active: row.try_get("key_is_active").map_sql_err()?,
            key_api_formats: parse_string_list(
                parse_json(row.try_get("key_api_formats").ok().flatten())?,
                "provider_api_keys.api_formats",
            )?,
            key_allowed_models: parse_string_list(
                parse_json(row.try_get("key_allowed_models").ok().flatten())?,
                "provider_api_keys.allowed_models",
            )?,
            key_capabilities: parse_json(row.try_get("key_capabilities").ok().flatten())?,
            key_internal_priority: row.try_get("key_internal_priority").map_sql_err()?,
            key_global_priority_by_format: parse_json(
                row.try_get("key_global_priority_by_format").ok().flatten(),
            )?,
            model_id: row.try_get("model_id").map_sql_err()?,
            global_model_id: row.try_get("global_model_id").map_sql_err()?,
            global_model_name: row.try_get("global_model_name").map_sql_err()?,
            global_model_mappings: parse_string_list(
                global_model_mappings,
                "global_models.config.model_mappings",
            )?,
            global_model_supports_streaming,
            model_provider_model_name: row.try_get("model_provider_model_name").map_sql_err()?,
            model_provider_model_mappings: parse_provider_model_mappings(parse_json(
                row.try_get("model_provider_model_mappings").ok().flatten(),
            )?)?,
            model_supports_streaming: row.try_get("model_supports_streaming").map_sql_err()?,
            model_is_active: row.try_get("model_is_active").map_sql_err()?,
            model_is_available: row.try_get("model_is_available").map_sql_err()?,
        },
    })
}

fn parse_json(value: Option<String>) -> Result<Option<serde_json::Value>, DataLayerError> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            serde_json::from_str(&value).map_err(|err| {
                DataLayerError::UnexpectedValue(format!(
                    "candidate selection JSON field is invalid: {err}"
                ))
            })
        })
        .transpose()
}

fn json_bool(value: &serde_json::Value) -> Option<bool> {
    value.as_bool().or_else(|| {
        value
            .as_str()
            .and_then(|value| value.trim().parse::<bool>().ok())
    })
}

fn parse_string_list(
    value: Option<serde_json::Value>,
    field_name: &str,
) -> Result<Option<Vec<String>>, DataLayerError> {
    let Some(value) = value else {
        return Ok(None);
    };
    parse_string_list_value(&value, field_name)
}

fn parse_string_list_value(
    value: &serde_json::Value,
    field_name: &str,
) -> Result<Option<Vec<String>>, DataLayerError> {
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Array(array) => parse_string_list_array(array, field_name).map(Some),
        serde_json::Value::String(raw) => parse_embedded_string_list(raw, field_name),
        _ => Err(DataLayerError::UnexpectedValue(format!(
            "{field_name} is not a JSON array"
        ))),
    }
}

fn parse_embedded_string_list(
    raw: &str,
    field_name: &str,
) -> Result<Option<Vec<String>>, DataLayerError> {
    let raw = raw.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("null") {
        return Ok(None);
    }

    if let Ok(decoded) = serde_json::from_str::<serde_json::Value>(raw) {
        return parse_string_list_value(&decoded, field_name);
    }

    Ok(Some(vec![raw.to_string()]))
}

fn parse_string_list_array(
    array: &[serde_json::Value],
    field_name: &str,
) -> Result<Vec<String>, DataLayerError> {
    let mut items = Vec::with_capacity(array.len());
    for item in array {
        let Some(item) = item.as_str() else {
            return Err(DataLayerError::UnexpectedValue(format!(
                "{field_name} contains a non-string item"
            )));
        };
        let item = item.trim();
        if !item.is_empty() {
            items.push(item.to_string());
        }
    }
    Ok(items)
}

fn parse_provider_model_mappings(
    value: Option<serde_json::Value>,
) -> Result<Option<Vec<StoredProviderModelMapping>>, DataLayerError> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Array(array) => parse_provider_model_mappings_array(&array),
        serde_json::Value::Object(object) => parse_provider_model_mapping_object_lenient(&object)
            .map(|mapping| mapping.map(|value| vec![value])),
        serde_json::Value::String(raw) => parse_embedded_provider_model_mappings(&raw),
        _ => Err(DataLayerError::UnexpectedValue(
            "models.provider_model_mappings is not a JSON array".to_string(),
        )),
    }
}

fn parse_embedded_provider_model_mappings(
    raw: &str,
) -> Result<Option<Vec<StoredProviderModelMapping>>, DataLayerError> {
    let raw = raw.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("null") {
        return Ok(None);
    }

    if let Ok(decoded) = serde_json::from_str::<serde_json::Value>(raw) {
        return parse_provider_model_mappings(Some(decoded));
    }

    Ok(Some(vec![StoredProviderModelMapping {
        name: raw.to_string(),
        priority: 1,
        api_formats: None,
        endpoint_ids: None,
        operations: None,
    }]))
}

fn parse_provider_model_mappings_array(
    array: &[serde_json::Value],
) -> Result<Option<Vec<StoredProviderModelMapping>>, DataLayerError> {
    let mut mappings = Vec::with_capacity(array.len());
    for raw in array {
        match raw {
            serde_json::Value::Object(object) => {
                if let Some(mapping) = parse_provider_model_mapping_object_lenient(object)? {
                    mappings.push(mapping);
                }
            }
            serde_json::Value::String(raw) if !raw.trim().is_empty() => {
                mappings.push(StoredProviderModelMapping {
                    name: raw.trim().to_string(),
                    priority: 1,
                    api_formats: None,
                    endpoint_ids: None,
                    operations: None,
                });
            }
            _ => {}
        }
    }

    if mappings.is_empty() {
        Ok(None)
    } else {
        Ok(Some(mappings))
    }
}

fn parse_provider_model_mapping_object_lenient(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<Option<StoredProviderModelMapping>, DataLayerError> {
    let Some(name) = object
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    let priority = object
        .get("priority")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(1)
        .max(1);
    let api_formats = parse_string_list(
        object.get("api_formats").cloned(),
        "models.provider_model_mappings.api_formats",
    )?
    .map(|formats| {
        formats
            .into_iter()
            .map(|value| normalize_api_format(&value))
            .collect()
    });
    let endpoint_ids = parse_string_list(
        object.get("endpoint_ids").cloned(),
        "models.provider_model_mappings.endpoint_ids",
    )?;
    let operations = parse_string_list(
        object.get("operations").cloned(),
        "models.provider_model_mappings.operations",
    )?
    .and_then(normalize_request_operations);

    Ok(Some(StoredProviderModelMapping {
        name: name.to_string(),
        priority: i32::try_from(priority).map_err(|_| {
            DataLayerError::UnexpectedValue(format!(
                "invalid models.provider_model_mappings.priority: {priority}"
            ))
        })?,
        api_formats,
        endpoint_ids,
        operations,
    }))
}

fn normalize_request_operations(values: Vec<String>) -> Option<Vec<String>> {
    let operations = values
        .into_iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    (!operations.is_empty()).then_some(operations)
}

fn api_format_aliases(api_format: &str) -> Vec<String> {
    aether_ai_formats::api_format_storage_aliases(api_format)
}

fn api_format_permission_aliases(api_format: &str) -> Vec<String> {
    aether_ai_formats::api_format_permission_storage_aliases(api_format)
}

fn normalize_api_format(api_format: &str) -> String {
    aether_ai_formats::normalize_api_format_alias(api_format)
}

fn api_format_matches(left: &str, right: &str) -> bool {
    aether_ai_formats::api_format_alias_matches(left, right)
}

fn api_format_scope_covers(allowed: &str, requested: &str) -> bool {
    aether_ai_formats::api_format_permission_covers(allowed, requested)
}

fn sql_match_aliases(api_formats: &[String]) -> Vec<String> {
    api_formats
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .collect()
}
