use std::sync::RwLock;

use async_trait::async_trait;

use super::{
    MinimalCandidateSelectionReadRepository, StoredApiFormatCandidateRowsQuery,
    StoredMinimalCandidateSelectionRow, StoredRequestedModelCandidateRowsQuery,
};
use crate::DataLayerError;

#[derive(Debug, Default)]
pub struct InMemoryMinimalCandidateSelectionReadRepository {
    rows: RwLock<Vec<StoredMinimalCandidateSelectionRow>>,
}

impl InMemoryMinimalCandidateSelectionReadRepository {
    pub fn seed<I>(rows: I) -> Self
    where
        I: IntoIterator<Item = StoredMinimalCandidateSelectionRow>,
    {
        Self {
            rows: RwLock::new(rows.into_iter().collect()),
        }
    }
}

#[async_trait]
impl MinimalCandidateSelectionReadRepository for InMemoryMinimalCandidateSelectionReadRepository {
    async fn list_for_exact_api_format(
        &self,
        api_format: &str,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        let api_format = api_format.trim();
        let mut rows = self
            .rows
            .read()
            .expect("candidate selection repository lock")
            .iter()
            .filter(|row| {
                row.provider_is_active
                    && row.endpoint_is_active
                    && row.key_is_active
                    && row.model_is_active
                    && row.model_is_available
                    && api_format_matches(&row.endpoint_api_format, api_format)
                    && row.key_supports_api_format(api_format)
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            left.provider_priority
                .cmp(&right.provider_priority)
                .then(left.key_internal_priority.cmp(&right.key_internal_priority))
                .then(left.provider_id.cmp(&right.provider_id))
                .then(left.endpoint_id.cmp(&right.endpoint_id))
                .then(left.key_id.cmp(&right.key_id))
                .then(left.model_id.cmp(&right.model_id))
        });
        Ok(rows)
    }

    async fn list_for_exact_api_format_page(
        &self,
        query: &StoredApiFormatCandidateRowsQuery,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        Ok(self
            .list_for_exact_api_format(&query.api_format)
            .await?
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
        let rows = self.list_for_exact_api_format(api_format).await?;
        Ok(rows
            .into_iter()
            .filter(|row| row.global_model_name == global_model_name)
            .collect())
    }

    async fn list_for_exact_api_format_and_requested_model(
        &self,
        api_format: &str,
        requested_model_name: &str,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        self.list_for_exact_api_format_and_requested_model_page(
            &StoredRequestedModelCandidateRowsQuery {
                api_format: api_format.to_string(),
                requested_model_name: requested_model_name.to_string(),
                offset: 0,
                limit: u32::MAX,
            },
        )
        .await
    }

    async fn list_for_exact_api_format_and_requested_model_page(
        &self,
        query: &StoredRequestedModelCandidateRowsQuery,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        let rows = self.list_for_exact_api_format(&query.api_format).await?;
        let mut rows = rows
            .into_iter()
            .filter(|row| {
                row_matches_requested_model(row, &query.requested_model_name, &query.api_format)
            })
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            left.global_model_name
                .cmp(&right.global_model_name)
                .then(left.provider_priority.cmp(&right.provider_priority))
                .then(left.key_internal_priority.cmp(&right.key_internal_priority))
                .then(left.provider_id.cmp(&right.provider_id))
                .then(left.endpoint_id.cmp(&right.endpoint_id))
                .then(left.key_id.cmp(&right.key_id))
                .then(left.model_id.cmp(&right.model_id))
        });
        Ok(rows
            .into_iter()
            .skip(query.offset as usize)
            .take(query.limit as usize)
            .collect())
    }
}

fn api_format_matches(left: &str, right: &str) -> bool {
    aether_ai_formats::api_format_alias_matches(left, right)
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
                    mapping.api_formats.as_ref().is_none_or(|formats| {
                        formats
                            .iter()
                            .any(|value| api_format_scope_covers(value, api_format))
                    }) && mapping.endpoint_ids.as_ref().is_none_or(|endpoint_ids| {
                        endpoint_ids
                            .iter()
                            .any(|endpoint_id| endpoint_id == &row.endpoint_id)
                    }) && mapping.name == requested_model_name
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
    mapping: &super::StoredProviderModelMapping,
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

fn api_format_scope_covers(allowed: &str, requested: &str) -> bool {
    aether_ai_formats::api_format_permission_covers(allowed, requested)
}
