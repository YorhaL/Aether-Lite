#[cfg(feature = "sqlite")]
use std::time::{SystemTime, UNIX_EPOCH};

use crate::DataLayerError;

use super::DataBackends;

#[derive(Debug, Clone, Copy)]
pub struct PrivacyDataState<'a> {
    backends: Option<&'a DataBackends>,
}

impl<'a> PrivacyDataState<'a> {
    pub fn new(backends: Option<&'a DataBackends>) -> Self {
        Self { backends }
    }

    pub async fn record_user_privacy_policy_acceptance(
        &self,
        user_id: &str,
        version: &str,
    ) -> Result<bool, DataLayerError> {
        let Some(backends) = self.backends else {
            return Ok(false);
        };
        #[cfg(feature = "postgres")]
        if let Some(backend) = backends.postgres() {
            let affected = sqlx::query(
                r#"
UPDATE users
SET privacy_policy_accepted_version = $2,
    privacy_policy_accepted_at = NOW()
WHERE id = $1
"#,
            )
            .bind(user_id)
            .bind(version)
            .execute(&backend.pool_clone())
            .await
            .map_err(DataLayerError::postgres)?
            .rows_affected();
            return Ok(affected > 0);
        }
        #[cfg(feature = "sqlite")]
        if let Some(backend) = backends.sqlite() {
            let affected = sqlx::query(
                r#"
UPDATE users
SET privacy_policy_accepted_version = ?,
    privacy_policy_accepted_at = ?
WHERE id = ?
"#,
            )
            .bind(version)
            .bind(now_unix_secs() as i64)
            .bind(user_id)
            .execute(&backend.pool_clone())
            .await
            .map_err(DataLayerError::sql)?
            .rows_affected();
            return Ok(affected > 0);
        }
        Ok(false)
    }
}

#[cfg(feature = "sqlite")]
fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}
