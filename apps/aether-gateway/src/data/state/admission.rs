use std::collections::HashMap;

use aether_data::repository::admission::{
    AdmissionPolicyDocument, AdmissionPolicyScope, AdmissionScopeKind, ResolvedAdmissionPolicy,
    StoredAdmissionPolicy,
};
use aether_data::repository::auth::StoredAuthApiKeyExportRecord;
use aether_data::repository::users::StoredUserExportRow;
use aether_data::repository::users::StoredUserGroup;
use aether_data::DataLayerError;

use super::GatewayDataState;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct GatewayAuthApiKeyExportRecord {
    #[serde(flatten)]
    stored: StoredAuthApiKeyExportRecord,
    pub(crate) daily_usage_limit_usd: Option<f64>,
}

impl GatewayAuthApiKeyExportRecord {
    fn from_stored(
        stored: StoredAuthApiKeyExportRecord,
        daily_usage_limit_usd: Option<f64>,
    ) -> Self {
        Self {
            stored,
            daily_usage_limit_usd,
        }
    }

    pub(crate) fn into_stored(self) -> StoredAuthApiKeyExportRecord {
        self.stored
    }
}

impl std::ops::Deref for GatewayAuthApiKeyExportRecord {
    type Target = StoredAuthApiKeyExportRecord;

    fn deref(&self) -> &Self::Target {
        &self.stored
    }
}

impl std::ops::DerefMut for GatewayAuthApiKeyExportRecord {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.stored
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct GatewayUserGroup {
    #[serde(flatten)]
    stored: StoredUserGroup,
    pub(crate) daily_usage_limit_usd: Option<f64>,
    pub(crate) daily_usage_limit_mode: String,
    pub(crate) concurrent_limit: Option<u32>,
    pub(crate) concurrent_limit_mode: String,
}

impl GatewayUserGroup {
    pub(crate) fn from_stored(
        mut stored: StoredUserGroup,
        document: &AdmissionPolicyDocument,
    ) -> Result<Self, DataLayerError> {
        stored.rate_limit = document
            .requests_per_minute()
            .map(i32::try_from)
            .transpose()
            .map_err(|_| {
                DataLayerError::UnexpectedValue(
                    "user group request limit exceeds the supported API range".to_string(),
                )
            })?;
        stored.rate_limit_mode = if stored.rate_limit.is_some() {
            "custom".to_string()
        } else {
            "inherit".to_string()
        };
        let daily_usage_limit_usd = document.daily_usage_limit_usd();
        let concurrent_limit = document.concurrent_requests();
        Ok(Self {
            stored,
            daily_usage_limit_usd,
            daily_usage_limit_mode: if daily_usage_limit_usd.is_some() {
                "custom".to_string()
            } else {
                "inherit".to_string()
            },
            concurrent_limit,
            concurrent_limit_mode: if concurrent_limit.is_some() {
                "custom".to_string()
            } else {
                "inherit".to_string()
            },
        })
    }

    pub(crate) fn into_stored(self) -> StoredUserGroup {
        self.stored
    }
}

impl std::ops::Deref for GatewayUserGroup {
    type Target = StoredUserGroup;

    fn deref(&self) -> &Self::Target {
        &self.stored
    }
}

impl std::ops::DerefMut for GatewayUserGroup {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.stored
    }
}

impl GatewayDataState {
    pub(crate) async fn get_admission_policy(
        &self,
        scope: &AdmissionPolicyScope,
    ) -> Result<Option<StoredAdmissionPolicy>, DataLayerError> {
        let Some(repository) = self
            .backends
            .as_ref()
            .and_then(|backends| backends.read().admission_policies())
        else {
            return Ok(None);
        };
        repository.get_policy(scope).await
    }

    pub(crate) async fn list_admission_policies(
        &self,
        scopes: &[AdmissionPolicyScope],
    ) -> Result<Vec<StoredAdmissionPolicy>, DataLayerError> {
        let Some(repository) = self
            .backends
            .as_ref()
            .and_then(|backends| backends.read().admission_policies())
        else {
            return Ok(Vec::new());
        };
        repository.list_policies(scopes).await
    }

    pub(crate) async fn put_admission_policy(
        &self,
        scope: &AdmissionPolicyScope,
        document: &AdmissionPolicyDocument,
    ) -> Result<StoredAdmissionPolicy, DataLayerError> {
        let Some(repository) = self
            .backends
            .as_ref()
            .and_then(|backends| backends.write().admission_policies())
        else {
            return Err(DataLayerError::InvalidInput(
                "admission policy repository is unavailable".to_string(),
            ));
        };
        repository.put_policy(scope, document).await
    }

    pub(crate) async fn delete_admission_policy(
        &self,
        scope: &AdmissionPolicyScope,
    ) -> Result<bool, DataLayerError> {
        let Some(repository) = self
            .backends
            .as_ref()
            .and_then(|backends| backends.write().admission_policies())
        else {
            return Ok(false);
        };
        repository.delete_policy(scope).await
    }

    pub(crate) async fn delete_admission_policies(
        &self,
        scopes: &[AdmissionPolicyScope],
    ) -> Result<u64, DataLayerError> {
        let mut deleted = 0_u64;
        for scope in scopes {
            deleted += u64::from(self.delete_admission_policy(scope).await?);
        }
        Ok(deleted)
    }

    pub(crate) async fn resolve_admission_policy(
        &self,
        user_id: &str,
        api_key_id: &str,
        group_ids: &[String],
        api_key_is_standalone: bool,
    ) -> Result<ResolvedAdmissionPolicy, DataLayerError> {
        let mut scopes = Vec::with_capacity(group_ids.len() + 3);
        scopes.push(AdmissionPolicyScope::system());
        if !api_key_is_standalone {
            scopes.extend(
                group_ids
                    .iter()
                    .cloned()
                    .map(|subject_id| AdmissionPolicyScope {
                        kind: AdmissionScopeKind::UserGroup,
                        subject_id,
                    }),
            );
            scopes.push(AdmissionPolicyScope {
                kind: AdmissionScopeKind::User,
                subject_id: user_id.to_string(),
            });
        }
        scopes.push(AdmissionPolicyScope {
            kind: AdmissionScopeKind::ApiKey,
            subject_id: api_key_id.to_string(),
        });

        let policies = self
            .list_admission_policies(&scopes)
            .await?
            .into_iter()
            .map(|policy| (policy.scope, policy.document))
            .collect::<HashMap<_, _>>();

        Ok(resolve_admission_policy_documents(
            user_id,
            api_key_id,
            group_ids,
            api_key_is_standalone,
            &policies,
        ))
    }

    pub(crate) async fn resolve_principal_admission_policy(
        &self,
        user_id: &str,
        group_ids: &[String],
    ) -> Result<AdmissionPolicyDocument, DataLayerError> {
        let mut scopes = Vec::with_capacity(group_ids.len() + 2);
        scopes.push(AdmissionPolicyScope::system());
        scopes.extend(
            group_ids
                .iter()
                .cloned()
                .map(|subject_id| AdmissionPolicyScope {
                    kind: AdmissionScopeKind::UserGroup,
                    subject_id,
                }),
        );
        scopes.push(AdmissionPolicyScope {
            kind: AdmissionScopeKind::User,
            subject_id: user_id.to_string(),
        });

        let policies = self
            .list_admission_policies(&scopes)
            .await?
            .into_iter()
            .map(|policy| (policy.scope, policy.document))
            .collect::<HashMap<_, _>>();

        Ok(resolve_admission_policy_documents(user_id, "", group_ids, false, &policies).principal)
    }

    pub(crate) async fn scoped_admission_document(
        &self,
        kind: AdmissionScopeKind,
        subject_id: &str,
    ) -> Result<AdmissionPolicyDocument, DataLayerError> {
        let scope = AdmissionPolicyScope {
            kind,
            subject_id: subject_id.to_string(),
        };
        Ok(self
            .get_admission_policy(&scope)
            .await?
            .map(|policy| policy.document)
            .unwrap_or_default())
    }

    pub(crate) async fn store_scoped_admission_document(
        &self,
        kind: AdmissionScopeKind,
        subject_id: &str,
        document: &AdmissionPolicyDocument,
    ) -> Result<(), DataLayerError> {
        let scope = AdmissionPolicyScope {
            kind,
            subject_id: subject_id.to_string(),
        };
        if document.is_empty() {
            self.delete_admission_policy(&scope).await?;
        } else {
            self.put_admission_policy(&scope, document).await?;
        }
        Ok(())
    }

    pub(crate) async fn enrich_api_key_export_record(
        &self,
        record: StoredAuthApiKeyExportRecord,
    ) -> Result<GatewayAuthApiKeyExportRecord, DataLayerError> {
        let document = self
            .scoped_admission_document(AdmissionScopeKind::ApiKey, &record.api_key_id)
            .await?;
        let mut record =
            GatewayAuthApiKeyExportRecord::from_stored(record, document.daily_usage_limit_usd());
        record.rate_limit = document
            .requests_per_minute()
            .map(i32::try_from)
            .transpose()
            .map_err(|_| {
                DataLayerError::UnexpectedValue(
                    "API key request limit exceeds the supported API range".to_string(),
                )
            })?;
        record.concurrent_limit = document
            .concurrent_requests()
            .map(i32::try_from)
            .transpose()
            .map_err(|_| {
                DataLayerError::UnexpectedValue(
                    "API key concurrency limit exceeds the supported API range".to_string(),
                )
            })?;
        Ok(record)
    }

    pub(crate) async fn enrich_optional_api_key_export_record(
        &self,
        record: Option<StoredAuthApiKeyExportRecord>,
    ) -> Result<Option<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        match record {
            Some(record) => self.enrich_api_key_export_record(record).await.map(Some),
            None => Ok(None),
        }
    }

    pub(crate) async fn enrich_api_key_export_records(
        &self,
        records: Vec<StoredAuthApiKeyExportRecord>,
    ) -> Result<Vec<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        let scopes = records
            .iter()
            .map(|record| AdmissionPolicyScope {
                kind: AdmissionScopeKind::ApiKey,
                subject_id: record.api_key_id.clone(),
            })
            .collect::<Vec<_>>();
        let policies = self
            .list_admission_policies(&scopes)
            .await?
            .into_iter()
            .map(|policy| (policy.scope.subject_id, policy.document))
            .collect::<HashMap<_, _>>();
        let mut enriched = Vec::with_capacity(records.len());
        for record in records {
            let document = policies
                .get(&record.api_key_id)
                .cloned()
                .unwrap_or_default();
            let mut record = GatewayAuthApiKeyExportRecord::from_stored(
                record,
                document.daily_usage_limit_usd(),
            );
            record.rate_limit = document
                .requests_per_minute()
                .map(i32::try_from)
                .transpose()
                .map_err(|_| {
                    DataLayerError::UnexpectedValue(
                        "API key request limit exceeds the supported API range".to_string(),
                    )
                })?;
            record.concurrent_limit = document
                .concurrent_requests()
                .map(i32::try_from)
                .transpose()
                .map_err(|_| {
                    DataLayerError::UnexpectedValue(
                        "API key concurrency limit exceeds the supported API range".to_string(),
                    )
                })?;
            enriched.push(record);
        }
        Ok(enriched)
    }

    pub(crate) async fn enrich_user_groups(
        &self,
        groups: Vec<StoredUserGroup>,
    ) -> Result<Vec<GatewayUserGroup>, DataLayerError> {
        let scopes = groups
            .iter()
            .map(|group| AdmissionPolicyScope {
                kind: AdmissionScopeKind::UserGroup,
                subject_id: group.id.clone(),
            })
            .collect::<Vec<_>>();
        let policies = self
            .list_admission_policies(&scopes)
            .await?
            .into_iter()
            .map(|policy| (policy.scope.subject_id, policy.document))
            .collect::<HashMap<_, _>>();
        let mut enriched = Vec::with_capacity(groups.len());
        for group in groups {
            let document = policies.get(&group.id).cloned().unwrap_or_default();
            enriched.push(GatewayUserGroup::from_stored(group, &document)?);
        }
        Ok(enriched)
    }

    pub(crate) async fn enrich_user_export_rows(
        &self,
        mut rows: Vec<StoredUserExportRow>,
    ) -> Result<Vec<StoredUserExportRow>, DataLayerError> {
        let scopes = rows
            .iter()
            .map(|row| AdmissionPolicyScope {
                kind: AdmissionScopeKind::User,
                subject_id: row.id.clone(),
            })
            .collect::<Vec<_>>();
        let policies = self
            .list_admission_policies(&scopes)
            .await?
            .into_iter()
            .map(|policy| (policy.scope.subject_id, policy.document))
            .collect::<HashMap<_, _>>();
        for row in &mut rows {
            let document = policies.get(&row.id).cloned().unwrap_or_default();
            row.rate_limit = document
                .requests_per_minute()
                .map(i32::try_from)
                .transpose()
                .map_err(|_| {
                    DataLayerError::UnexpectedValue(
                        "user request limit exceeds the supported API range".to_string(),
                    )
                })?;
            row.rate_limit_mode = if row.rate_limit.is_some() {
                "custom".to_string()
            } else {
                "system".to_string()
            };
        }
        Ok(rows)
    }
}

fn resolve_admission_policy_documents(
    user_id: &str,
    api_key_id: &str,
    group_ids: &[String],
    api_key_is_standalone: bool,
    policies: &HashMap<AdmissionPolicyScope, AdmissionPolicyDocument>,
) -> ResolvedAdmissionPolicy {
    let mut principal = policies
        .get(&AdmissionPolicyScope::system())
        .cloned()
        .unwrap_or_default();
    if !api_key_is_standalone {
        let mut group_policy = AdmissionPolicyDocument::default();
        for group_id in group_ids {
            let scope = AdmissionPolicyScope {
                kind: AdmissionScopeKind::UserGroup,
                subject_id: group_id.clone(),
            };
            if let Some(policy) = policies.get(&scope) {
                group_policy = group_policy.union_grants(policy);
            }
        }
        principal = principal.overlay(&group_policy);
        let user_scope = AdmissionPolicyScope {
            kind: AdmissionScopeKind::User,
            subject_id: user_id.to_string(),
        };
        if let Some(policy) = policies.get(&user_scope) {
            principal = principal.overlay(policy);
        }
    }
    let api_key_scope = AdmissionPolicyScope {
        kind: AdmissionScopeKind::ApiKey,
        subject_id: api_key_id.to_string(),
    };
    let api_key = policies.get(&api_key_scope).cloned().unwrap_or_default();

    ResolvedAdmissionPolicy { principal, api_key }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(kind: AdmissionScopeKind, subject_id: &str) -> AdmissionPolicyScope {
        AdmissionPolicyScope {
            kind,
            subject_id: subject_id.to_string(),
        }
    }

    #[test]
    fn resolves_system_group_user_and_key_layers() {
        let group_ids = vec!["basic".to_string(), "extended".to_string()];
        let policies = HashMap::from([
            (
                AdmissionPolicyScope::system(),
                AdmissionPolicyDocument::default()
                    .with_requests_per_minute(Some(100))
                    .with_concurrent_requests(Some(8))
                    .with_daily_usage_limit_usd(Some(10.0)),
            ),
            (
                scope(AdmissionScopeKind::UserGroup, "basic"),
                AdmissionPolicyDocument::default()
                    .with_requests_per_minute(Some(200))
                    .with_daily_usage_limit_usd(Some(20.0)),
            ),
            (
                scope(AdmissionScopeKind::UserGroup, "extended"),
                AdmissionPolicyDocument::default()
                    .with_requests_per_minute(Some(0))
                    .with_concurrent_requests(Some(16)),
            ),
            (
                scope(AdmissionScopeKind::User, "user-1"),
                AdmissionPolicyDocument::default().with_requests_per_minute(Some(50)),
            ),
            (
                scope(AdmissionScopeKind::ApiKey, "key-1"),
                AdmissionPolicyDocument::default().with_concurrent_requests(Some(2)),
            ),
        ]);

        let resolved =
            resolve_admission_policy_documents("user-1", "key-1", &group_ids, false, &policies);

        assert_eq!(resolved.principal.requests_per_minute(), Some(50));
        assert_eq!(resolved.principal.concurrent_requests(), Some(16));
        assert_eq!(resolved.principal.daily_usage_limit_usd(), Some(20.0));
        assert_eq!(resolved.api_key.requests_per_minute(), None);
        assert_eq!(resolved.api_key.concurrent_requests(), Some(2));
    }

    #[test]
    fn standalone_key_skips_user_and_group_layers() {
        let group_ids = vec!["group-1".to_string()];
        let policies = HashMap::from([
            (
                AdmissionPolicyScope::system(),
                AdmissionPolicyDocument::default().with_requests_per_minute(Some(100)),
            ),
            (
                scope(AdmissionScopeKind::UserGroup, "group-1"),
                AdmissionPolicyDocument::default().with_requests_per_minute(Some(50)),
            ),
            (
                scope(AdmissionScopeKind::User, "user-1"),
                AdmissionPolicyDocument::default().with_requests_per_minute(Some(25)),
            ),
            (
                scope(AdmissionScopeKind::ApiKey, "key-1"),
                AdmissionPolicyDocument::default().with_requests_per_minute(Some(0)),
            ),
        ]);

        let resolved =
            resolve_admission_policy_documents("user-1", "key-1", &group_ids, true, &policies);

        assert_eq!(resolved.principal.requests_per_minute(), Some(100));
        assert_eq!(resolved.api_key.requests_per_minute(), Some(0));
    }
}
