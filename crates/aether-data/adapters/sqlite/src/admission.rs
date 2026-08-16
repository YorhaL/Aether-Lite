use aether_data_contracts::repository::admission::{
    AdmissionPolicyDocument, AdmissionPolicyReadRepository, AdmissionPolicyScope,
    AdmissionPolicyWriteRepository, AdmissionScopeKind, StoredAdmissionPolicy,
};
use aether_data_contracts::DataLayerError;
use async_trait::async_trait;
use sqlx::{QueryBuilder, Row, Sqlite};

use crate::error::SqlResultExt;
use crate::SqlitePool;

#[derive(Debug, Clone)]
pub struct SqliteAdmissionPolicyRepository {
    pool: SqlitePool,
}

impl SqliteAdmissionPolicyRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AdmissionPolicyReadRepository for SqliteAdmissionPolicyRepository {
    async fn get_policy(
        &self,
        scope: &AdmissionPolicyScope,
    ) -> Result<Option<StoredAdmissionPolicy>, DataLayerError> {
        let row = sqlx::query(
            r#"
SELECT p.schema_version, p.document, p.scope_kind, p.subject_id
FROM lite_admission_policies AS p
WHERE p.scope_kind = ? AND p.subject_id = ?
"#,
        )
        .bind(scope.kind.as_str())
        .bind(&scope.subject_id)
        .fetch_optional(&self.pool)
        .await
        .map_sql_err()?;
        row.as_ref().map(map_row).transpose()
    }

    async fn list_policies(
        &self,
        scopes: &[AdmissionPolicyScope],
    ) -> Result<Vec<StoredAdmissionPolicy>, DataLayerError> {
        if scopes.is_empty() {
            return Ok(Vec::new());
        }
        let mut query = QueryBuilder::<Sqlite>::new(
            r#"
SELECT p.schema_version, p.document, p.scope_kind, p.subject_id
FROM lite_admission_policies AS p
WHERE (p.scope_kind, p.subject_id) IN
"#,
        );
        query.push_tuples(scopes, |mut tuple, scope| {
            tuple
                .push_bind(scope.kind.as_str())
                .push_bind(&scope.subject_id);
        });
        query.push(" ORDER BY p.scope_kind, p.subject_id");
        query
            .build()
            .fetch_all(&self.pool)
            .await
            .map_sql_err()?
            .iter()
            .map(map_row)
            .collect()
    }
}

#[async_trait]
impl AdmissionPolicyWriteRepository for SqliteAdmissionPolicyRepository {
    async fn put_policy(
        &self,
        scope: &AdmissionPolicyScope,
        document: &AdmissionPolicyDocument,
    ) -> Result<StoredAdmissionPolicy, DataLayerError> {
        document.validate()?;
        let document_json = serde_json::to_string(document).map_err(|error| {
            DataLayerError::UnexpectedValue(format!(
                "failed to serialize admission policy document: {error}"
            ))
        })?;
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            r#"
INSERT INTO lite_admission_policies (
    scope_kind, subject_id, schema_version, document, created_at, updated_at
)
VALUES (?, ?, ?, ?, ?, ?)
ON CONFLICT(scope_kind, subject_id) DO UPDATE
SET schema_version = excluded.schema_version,
    document = excluded.document,
    updated_at = excluded.updated_at
"#,
        )
        .bind(scope.kind.as_str())
        .bind(&scope.subject_id)
        .bind(i64::from(document.schema_version))
        .bind(document_json)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_sql_err()?;
        Ok(StoredAdmissionPolicy {
            scope: scope.clone(),
            document: document.clone(),
        })
    }

    async fn delete_policy(&self, scope: &AdmissionPolicyScope) -> Result<bool, DataLayerError> {
        let result = sqlx::query(
            "DELETE FROM lite_admission_policies WHERE scope_kind = ? AND subject_id = ?",
        )
        .bind(scope.kind.as_str())
        .bind(&scope.subject_id)
        .execute(&self.pool)
        .await
        .map_sql_err()?;
        Ok(result.rows_affected() > 0)
    }
}

fn map_row(row: &sqlx::sqlite::SqliteRow) -> Result<StoredAdmissionPolicy, DataLayerError> {
    let scope_kind: String = row.try_get("scope_kind").map_sql_err()?;
    let schema_version: i64 = row.try_get("schema_version").map_sql_err()?;
    let document: AdmissionPolicyDocument = serde_json::from_str(
        &row.try_get::<String, _>("document").map_sql_err()?,
    )
    .map_err(|error| {
        DataLayerError::UnexpectedValue(format!("invalid admission policy document JSON: {error}"))
    })?;
    let stored_schema_version = u16::try_from(schema_version).map_err(|_| {
        DataLayerError::UnexpectedValue(format!(
            "invalid admission policy schema version: {schema_version}"
        ))
    })?;
    if document.schema_version != stored_schema_version {
        return Err(DataLayerError::UnexpectedValue(format!(
            "admission policy schema version mismatch: column={stored_schema_version}, document={}",
            document.schema_version
        )));
    }
    document.validate()?;
    Ok(StoredAdmissionPolicy {
        scope: AdmissionPolicyScope::new(
            AdmissionScopeKind::parse(&scope_kind)?,
            row.try_get::<String, _>("subject_id").map_sql_err()?,
        )?,
        document,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn admission_policy_round_trips_all_scopes_and_rules() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("SQLite pool should build");
        sqlx::raw_sql(include_str!(
            "../../../runtime/migrations/lite/sqlite/20260803000000_admission_policies.sql"
        ))
        .execute(&pool)
        .await
        .expect("Lite admission migration should run");
        let repository = SqliteAdmissionPolicyRepository::new(pool);
        let system = AdmissionPolicyScope::system();
        let user = AdmissionPolicyScope::new(AdmissionScopeKind::User, "user-1")
            .expect("user scope should build");
        let document = AdmissionPolicyDocument::default()
            .with_requests_per_minute(Some(120))
            .with_concurrent_requests(Some(8))
            .with_daily_usage_limit_usd(Some(25.0));

        repository
            .put_policy(&system, &document)
            .await
            .expect("system policy should write");
        repository
            .put_policy(
                &user,
                &AdmissionPolicyDocument::default().with_requests_per_minute(Some(30)),
            )
            .await
            .expect("user policy should write");

        let stored = repository
            .get_policy(&system)
            .await
            .expect("system policy should read")
            .expect("system policy should exist");
        assert_eq!(stored.document, document);
        assert_eq!(
            repository
                .list_policies(&[system.clone(), user.clone()])
                .await
                .expect("policies should list")
                .len(),
            2
        );

        assert!(repository
            .delete_policy(&user)
            .await
            .expect("user policy should delete"));
        assert!(repository
            .get_policy(&user)
            .await
            .expect("deleted user policy lookup should work")
            .is_none());
    }
}
