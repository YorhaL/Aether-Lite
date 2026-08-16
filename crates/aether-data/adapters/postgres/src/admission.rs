use aether_data_contracts::repository::admission::{
    AdmissionPolicyDocument, AdmissionPolicyReadRepository, AdmissionPolicyScope,
    AdmissionPolicyWriteRepository, AdmissionScopeKind, StoredAdmissionPolicy,
};
use aether_data_contracts::DataLayerError;
use async_trait::async_trait;
use sqlx::{PgPool, Postgres, QueryBuilder, Row};

use crate::error::SqlxResultExt;

#[derive(Debug, Clone)]
pub struct PostgresAdmissionPolicyRepository {
    pool: PgPool,
}

impl PostgresAdmissionPolicyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AdmissionPolicyReadRepository for PostgresAdmissionPolicyRepository {
    async fn get_policy(
        &self,
        scope: &AdmissionPolicyScope,
    ) -> Result<Option<StoredAdmissionPolicy>, DataLayerError> {
        let row = sqlx::query(
            r#"
SELECT p.schema_version, p.document, p.scope_kind, p.subject_id
FROM aether_lite.admission_policies AS p
WHERE p.scope_kind = $1 AND p.subject_id = $2
"#,
        )
        .bind(scope.kind.as_str())
        .bind(&scope.subject_id)
        .fetch_optional(&self.pool)
        .await
        .map_postgres_err()?;
        row.as_ref().map(map_row).transpose()
    }

    async fn list_policies(
        &self,
        scopes: &[AdmissionPolicyScope],
    ) -> Result<Vec<StoredAdmissionPolicy>, DataLayerError> {
        if scopes.is_empty() {
            return Ok(Vec::new());
        }
        let mut query = QueryBuilder::<Postgres>::new(
            r#"
SELECT p.schema_version, p.document, p.scope_kind, p.subject_id
FROM aether_lite.admission_policies AS p
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
            .map_postgres_err()?
            .iter()
            .map(map_row)
            .collect()
    }
}

#[async_trait]
impl AdmissionPolicyWriteRepository for PostgresAdmissionPolicyRepository {
    async fn put_policy(
        &self,
        scope: &AdmissionPolicyScope,
        document: &AdmissionPolicyDocument,
    ) -> Result<StoredAdmissionPolicy, DataLayerError> {
        document.validate()?;
        let document_value = serde_json::to_value(document).map_err(|error| {
            DataLayerError::UnexpectedValue(format!(
                "failed to serialize admission policy document: {error}"
            ))
        })?;
        sqlx::query(
            r#"
INSERT INTO aether_lite.admission_policies (
    scope_kind, subject_id, schema_version, document, created_at, updated_at
)
VALUES ($1, $2, $3, $4, now(), now())
ON CONFLICT (scope_kind, subject_id) DO UPDATE
SET schema_version = EXCLUDED.schema_version,
    document = EXCLUDED.document,
    updated_at = now()
"#,
        )
        .bind(scope.kind.as_str())
        .bind(&scope.subject_id)
        .bind(i32::from(document.schema_version))
        .bind(document_value)
        .execute(&self.pool)
        .await
        .map_postgres_err()?;
        Ok(StoredAdmissionPolicy {
            scope: scope.clone(),
            document: document.clone(),
        })
    }

    async fn delete_policy(&self, scope: &AdmissionPolicyScope) -> Result<bool, DataLayerError> {
        let result = sqlx::query(
            r#"
DELETE FROM aether_lite.admission_policies
WHERE scope_kind = $1 AND subject_id = $2
"#,
        )
        .bind(scope.kind.as_str())
        .bind(&scope.subject_id)
        .execute(&self.pool)
        .await
        .map_postgres_err()?;
        Ok(result.rows_affected() > 0)
    }
}

fn map_row(row: &sqlx::postgres::PgRow) -> Result<StoredAdmissionPolicy, DataLayerError> {
    let scope_kind: String = row.try_get("scope_kind").map_postgres_err()?;
    let schema_version: i32 = row.try_get("schema_version").map_postgres_err()?;
    let document: AdmissionPolicyDocument = serde_json::from_value(
        row.try_get::<serde_json::Value, _>("document")
            .map_postgres_err()?,
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
            row.try_get::<String, _>("subject_id").map_postgres_err()?,
        )?,
        document,
    })
}
