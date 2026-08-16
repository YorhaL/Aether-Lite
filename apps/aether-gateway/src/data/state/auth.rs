use super::{
    AuthApiKeyLookupKey, CreateManagementTokenRecord, DataLayerError,
    GatewayAuthApiKeyExportRecord, GatewayAuthApiKeySnapshot, GatewayDataState, GatewayUserGroup,
    ManagementTokenCounterDelta, ManagementTokenListQuery, RegenerateManagementTokenSecret,
    StoredAuthApiKeySnapshot, StoredLdapModuleConfig, StoredManagementToken,
    StoredManagementTokenListPage, StoredManagementTokenWithUser, StoredOAuthProviderConfig,
    StoredOAuthProviderModuleConfig, StoredUserAuthRecord, StoredUserOAuthLinkSummary,
    StoredUserPreferenceRecord, StoredUserSessionRecord, StoredWalletSnapshot,
    UpdateManagementTokenRecord, UpsertOAuthProviderConfigRecord,
};
use crate::LocalMutationOutcome;
use aether_data::backend::PrivacyDataState;
use aether_data::repository::auth::ResolvedAuthApiKeySnapshotReader;

#[derive(Debug, Clone, Default)]
pub(crate) struct GatewayUserEffectiveListPolicies {
    pub(crate) allowed_providers: Option<Vec<String>>,
    pub(crate) allowed_api_formats: Option<Vec<String>>,
    pub(crate) allowed_models: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GatewayUserGroupPolicySets {
    pub(crate) assigned_groups: Vec<GatewayUserGroup>,
    pub(crate) effective_groups: Vec<GatewayUserGroup>,
}

impl GatewayDataState {
    pub(crate) async fn record_user_privacy_policy_acceptance(
        &self,
        user_id: &str,
        version: &str,
    ) -> Result<bool, DataLayerError> {
        PrivacyDataState::new(self.backends.as_ref())
            .record_user_privacy_policy_acceptance(user_id, version)
            .await
    }

    pub(crate) async fn is_other_user_auth_email_taken(
        &self,
        email: &str,
        user_id: &str,
    ) -> Result<bool, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(false);
        };
        Ok(repository
            .find_user_auth_by_email(email)
            .await?
            .is_some_and(|user| user.id != user_id))
    }

    pub(crate) async fn is_other_user_auth_username_taken(
        &self,
        username: &str,
        user_id: &str,
    ) -> Result<bool, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(false);
        };
        Ok(repository
            .find_user_auth_by_username(username)
            .await?
            .is_some_and(|user| user.id != user_id))
    }

    pub(crate) async fn find_active_user_auth_by_email_ci(
        &self,
        email: &str,
    ) -> Result<Option<StoredUserAuthRecord>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(None);
        };
        repository.find_active_user_auth_by_email_ci(email).await
    }

    pub(crate) async fn find_user_auth_by_username(
        &self,
        username: &str,
    ) -> Result<Option<StoredUserAuthRecord>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(None);
        };
        repository.find_user_auth_by_username(username).await
    }

    pub(crate) async fn find_user_auth_by_id(
        &self,
        user_id: &str,
    ) -> Result<Option<StoredUserAuthRecord>, DataLayerError> {
        match &self.user_reader {
            Some(repository) => repository.find_user_auth_by_id(user_id).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn find_user_auth_by_identifier(
        &self,
        identifier: &str,
    ) -> Result<Option<StoredUserAuthRecord>, DataLayerError> {
        match &self.user_reader {
            Some(repository) => repository.find_user_auth_by_identifier(identifier).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn list_user_groups(&self) -> Result<Vec<GatewayUserGroup>, DataLayerError> {
        let groups = match &self.user_reader {
            Some(repository) => repository.list_user_groups().await,
            None => Ok(Vec::new()),
        }?;
        self.enrich_user_groups(groups).await
    }

    pub(crate) async fn find_user_group_by_id(
        &self,
        group_id: &str,
    ) -> Result<Option<GatewayUserGroup>, DataLayerError> {
        let group = match &self.user_reader {
            Some(repository) => repository.find_user_group_by_id(group_id).await,
            None => Ok(None),
        }?;
        Ok(self
            .enrich_user_groups(group.into_iter().collect())
            .await?
            .into_iter()
            .next())
    }

    pub(crate) async fn list_user_groups_by_ids(
        &self,
        group_ids: &[String],
    ) -> Result<Vec<GatewayUserGroup>, DataLayerError> {
        let groups = match &self.user_reader {
            Some(repository) => repository.list_user_groups_by_ids(group_ids).await,
            None => Ok(Vec::new()),
        }?;
        self.enrich_user_groups(groups).await
    }

    pub(crate) async fn create_user_group(
        &self,
        record: aether_data::repository::users::UpsertUserGroupRecord,
        admission_policy: aether_data::repository::admission::AdmissionPolicyDocument,
    ) -> Result<Option<GatewayUserGroup>, DataLayerError> {
        let created = match &self.user_reader {
            Some(repository) => repository.create_user_group(record).await,
            None => Ok(None),
        }?;
        let Some(created) = created else {
            return Ok(None);
        };
        if let Err(error) = self
            .store_scoped_admission_document(
                aether_data::repository::admission::AdmissionScopeKind::UserGroup,
                &created.id,
                &admission_policy,
            )
            .await
        {
            if let Some(repository) = &self.user_reader {
                let _ = repository.delete_user_group(&created.id).await;
            }
            return Err(error);
        }
        Ok(self
            .enrich_user_groups(vec![created])
            .await?
            .into_iter()
            .next())
    }

    pub(crate) async fn update_user_group(
        &self,
        group_id: &str,
        record: aether_data::repository::users::UpsertUserGroupRecord,
        admission_policy: aether_data::repository::admission::AdmissionPolicyDocument,
    ) -> Result<Option<GatewayUserGroup>, DataLayerError> {
        let updated = match &self.user_reader {
            Some(repository) => repository.update_user_group(group_id, record).await,
            None => Ok(None),
        }?;
        let Some(updated) = updated else {
            return Ok(None);
        };
        self.store_scoped_admission_document(
            aether_data::repository::admission::AdmissionScopeKind::UserGroup,
            group_id,
            &admission_policy,
        )
        .await?;
        Ok(self
            .enrich_user_groups(vec![updated])
            .await?
            .into_iter()
            .next())
    }

    pub(crate) async fn delete_user_group(&self, group_id: &str) -> Result<bool, DataLayerError> {
        let deleted = match &self.user_reader {
            Some(repository) => repository.delete_user_group(group_id).await,
            None => Ok(false),
        }?;
        if deleted {
            self.delete_admission_policy(
                &aether_data::repository::admission::AdmissionPolicyScope {
                    kind: aether_data::repository::admission::AdmissionScopeKind::UserGroup,
                    subject_id: group_id.to_string(),
                },
            )
            .await?;
        }
        Ok(deleted)
    }

    pub(crate) async fn list_user_group_members(
        &self,
        group_id: &str,
    ) -> Result<Vec<aether_data::repository::users::StoredUserGroupMember>, DataLayerError> {
        match &self.user_reader {
            Some(repository) => repository.list_user_group_members(group_id).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn replace_user_group_members(
        &self,
        group_id: &str,
        user_ids: &[String],
    ) -> Result<Vec<aether_data::repository::users::StoredUserGroupMember>, DataLayerError> {
        match &self.user_reader {
            Some(repository) => {
                repository
                    .replace_user_group_members(group_id, user_ids)
                    .await
            }
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn list_user_groups_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<GatewayUserGroup>, DataLayerError> {
        let groups = match &self.user_reader {
            Some(repository) => repository.list_user_groups_for_user(user_id).await,
            None => Ok(Vec::new()),
        }?;
        self.enrich_user_groups(groups).await
    }

    pub(crate) async fn list_user_group_memberships_by_user_ids(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<aether_data::repository::users::StoredUserGroupMembership>, DataLayerError>
    {
        match &self.user_reader {
            Some(repository) => {
                repository
                    .list_user_group_memberships_by_user_ids(user_ids)
                    .await
            }
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn replace_user_groups_for_user(
        &self,
        user_id: &str,
        group_ids: &[String],
    ) -> Result<Vec<GatewayUserGroup>, DataLayerError> {
        let groups = match &self.user_reader {
            Some(repository) => {
                repository
                    .replace_user_groups_for_user(user_id, group_ids)
                    .await
            }
            None => Ok(Vec::new()),
        }?;
        self.enrich_user_groups(groups).await
    }

    pub(crate) async fn add_user_to_group(
        &self,
        group_id: &str,
        user_id: &str,
    ) -> Result<bool, DataLayerError> {
        match &self.user_reader {
            Some(repository) => repository.add_user_to_group(group_id, user_id).await,
            None => Ok(false),
        }
    }

    pub(crate) async fn list_user_oauth_links(
        &self,
        user_id: &str,
    ) -> Result<Vec<StoredUserOAuthLinkSummary>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(Vec::new());
        };
        repository.list_user_oauth_links(user_id).await
    }

    pub(crate) async fn find_oauth_linked_user(
        &self,
        provider_type: &str,
        provider_user_id: &str,
    ) -> Result<Option<StoredUserAuthRecord>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(None);
        };
        repository
            .find_oauth_linked_user(provider_type, provider_user_id)
            .await
    }

    pub(crate) async fn touch_oauth_link(
        &self,
        provider_type: &str,
        provider_user_id: &str,
        provider_username: Option<&str>,
        provider_email: Option<&str>,
        extra_data: Option<serde_json::Value>,
        touched_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(false);
        };
        repository
            .touch_oauth_link(
                provider_type,
                provider_user_id,
                provider_username,
                provider_email,
                extra_data,
                touched_at,
            )
            .await
    }

    pub(crate) async fn create_oauth_auth_user(
        &self,
        email: Option<String>,
        username: String,
        created_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<StoredUserAuthRecord>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(None);
        };
        repository
            .create_oauth_auth_user(email, username, created_at)
            .await
    }

    pub(crate) async fn find_oauth_link_owner(
        &self,
        provider_type: &str,
        provider_user_id: &str,
    ) -> Result<Option<String>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(None);
        };
        repository
            .find_oauth_link_owner(provider_type, provider_user_id)
            .await
    }

    pub(crate) async fn has_user_oauth_provider_link(
        &self,
        user_id: &str,
        provider_type: &str,
    ) -> Result<bool, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(false);
        };
        repository
            .has_user_oauth_provider_link(user_id, provider_type)
            .await
    }

    pub(crate) async fn count_user_oauth_links(
        &self,
        user_id: &str,
    ) -> Result<u64, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(0);
        };
        repository.count_user_oauth_links(user_id).await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn upsert_user_oauth_link(
        &self,
        user_id: &str,
        provider_type: &str,
        provider_user_id: &str,
        provider_username: Option<&str>,
        provider_email: Option<&str>,
        extra_data: Option<serde_json::Value>,
        linked_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(());
        };
        repository
            .upsert_user_oauth_link(
                user_id,
                provider_type,
                provider_user_id,
                provider_username,
                provider_email,
                extra_data,
                linked_at,
            )
            .await
    }

    pub(crate) async fn delete_user_oauth_link(
        &self,
        user_id: &str,
        provider_type: &str,
    ) -> Result<bool, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(false);
        };
        repository
            .delete_user_oauth_link(user_id, provider_type)
            .await
    }

    pub(crate) async fn read_user_preferences(
        &self,
        user_id: &str,
    ) -> Result<Option<StoredUserPreferenceRecord>, DataLayerError> {
        if let Some(store) = &self.user_preferences {
            return Ok(store
                .read()
                .expect("user preference store should lock")
                .get(user_id)
                .cloned());
        }

        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(None);
        };
        repository.read_user_preferences(user_id).await
    }

    pub(crate) async fn write_user_preferences(
        &self,
        preferences: &StoredUserPreferenceRecord,
    ) -> Result<Option<StoredUserPreferenceRecord>, DataLayerError> {
        if let Some(store) = &self.user_preferences {
            store
                .write()
                .expect("user preference store should lock")
                .insert(preferences.user_id.clone(), preferences.clone());
            return Ok(Some(preferences.clone()));
        }

        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(None);
        };
        repository.write_user_preferences(preferences).await
    }

    pub(crate) async fn find_active_provider_name(
        &self,
        provider_id: &str,
    ) -> Result<Option<String>, DataLayerError> {
        let providers = self.list_provider_catalog_providers(true).await?;
        Ok(providers
            .into_iter()
            .find(|provider| provider.id == provider_id)
            .map(|provider| provider.name))
    }

    pub(crate) async fn find_user_session(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<StoredUserSessionRecord>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(None);
        };
        repository.find_user_session(user_id, session_id).await
    }

    pub(crate) async fn list_user_sessions(
        &self,
        user_id: &str,
    ) -> Result<Vec<StoredUserSessionRecord>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(Vec::new());
        };
        repository.list_user_sessions(user_id).await
    }

    pub(crate) async fn create_user_session(
        &self,
        session: &StoredUserSessionRecord,
    ) -> Result<Option<StoredUserSessionRecord>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(None);
        };
        repository.create_user_session(session).await
    }

    pub(crate) async fn update_user_model_capability_settings(
        &self,
        user_id: &str,
        settings: Option<serde_json::Value>,
    ) -> Result<Option<serde_json::Value>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(None);
        };
        repository
            .update_user_model_capability_settings(user_id, settings)
            .await
    }

    pub(crate) async fn update_user_feature_settings(
        &self,
        user_id: &str,
        settings: Option<serde_json::Value>,
    ) -> Result<Option<serde_json::Value>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(None);
        };
        repository
            .update_user_feature_settings(user_id, settings)
            .await
    }

    pub(crate) async fn update_local_auth_user_profile(
        &self,
        user_id: &str,
        email: Option<String>,
        username: Option<String>,
    ) -> Result<Option<StoredUserAuthRecord>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(None);
        };
        repository
            .update_local_auth_user_profile(user_id, email, username)
            .await
    }

    pub(crate) async fn update_local_auth_user_password_hash(
        &self,
        user_id: &str,
        password_hash: String,
        updated_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<StoredUserAuthRecord>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(None);
        };
        repository
            .update_local_auth_user_password_hash(user_id, password_hash, updated_at)
            .await
    }

    #[allow(dead_code)]
    pub(crate) async fn create_local_auth_user(
        &self,
        email: Option<String>,
        email_verified: bool,
        username: String,
        password_hash: String,
    ) -> Result<Option<StoredUserAuthRecord>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(None);
        };
        repository
            .create_local_auth_user(email, email_verified, username, password_hash)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn create_local_auth_user_with_settings(
        &self,
        email: Option<String>,
        email_verified: bool,
        username: String,
        password_hash: String,
        role: String,
        allowed_providers: Option<Vec<String>>,
        allowed_api_formats: Option<Vec<String>>,
        allowed_models: Option<Vec<String>>,
        rate_limit: Option<i32>,
    ) -> Result<Option<StoredUserAuthRecord>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(None);
        };
        let user = repository
            .create_local_auth_user_with_settings(
                email,
                email_verified,
                username,
                password_hash,
                role,
                allowed_providers,
                allowed_api_formats,
                allowed_models,
                None,
            )
            .await?;
        let Some(user) = user else {
            return Ok(None);
        };
        if let Some(rate_limit) = rate_limit {
            let document = aether_data::repository::admission::AdmissionPolicyDocument::default()
                .with_requests_per_minute(Some(rate_limit.max(0) as u32));
            if let Err(error) = self
                .store_scoped_admission_document(
                    aether_data::repository::admission::AdmissionScopeKind::User,
                    &user.id,
                    &document,
                )
                .await
            {
                let _ = repository.delete_local_auth_user(&user.id).await;
                return Err(error);
            }
        }
        Ok(Some(user))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn update_local_auth_user_admin_fields(
        &self,
        user_id: &str,
        role: Option<String>,
        allowed_providers_present: bool,
        allowed_providers: Option<Vec<String>>,
        allowed_api_formats_present: bool,
        allowed_api_formats: Option<Vec<String>>,
        allowed_models_present: bool,
        allowed_models: Option<Vec<String>>,
        rate_limit_present: bool,
        rate_limit: Option<i32>,
        is_active: Option<bool>,
    ) -> Result<Option<StoredUserAuthRecord>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(None);
        };
        let updated = repository
            .update_local_auth_user_admin_fields(
                user_id,
                role,
                allowed_providers_present,
                allowed_providers,
                allowed_api_formats_present,
                allowed_api_formats,
                allowed_models_present,
                allowed_models,
                false,
                None,
                is_active,
            )
            .await?;
        if updated.is_some() && rate_limit_present {
            let mut document = self
                .scoped_admission_document(
                    aether_data::repository::admission::AdmissionScopeKind::User,
                    user_id,
                )
                .await?;
            document =
                document.with_requests_per_minute(rate_limit.map(|value| value.max(0) as u32));
            self.store_scoped_admission_document(
                aether_data::repository::admission::AdmissionScopeKind::User,
                user_id,
                &document,
            )
            .await?;
        }
        Ok(updated)
    }

    pub(crate) async fn update_local_auth_user_policy_modes(
        &self,
        user_id: &str,
        allowed_providers_mode: Option<String>,
        allowed_api_formats_mode: Option<String>,
        allowed_models_mode: Option<String>,
        rate_limit_mode: Option<String>,
    ) -> Result<Option<StoredUserAuthRecord>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(None);
        };
        let updated = repository
            .update_local_auth_user_policy_modes(
                user_id,
                allowed_providers_mode,
                allowed_api_formats_mode,
                allowed_models_mode,
                None,
            )
            .await?;
        if updated.is_some()
            && rate_limit_mode
                .as_deref()
                .is_some_and(|mode| mode != "custom")
        {
            let document = self
                .scoped_admission_document(
                    aether_data::repository::admission::AdmissionScopeKind::User,
                    user_id,
                )
                .await?
                .with_requests_per_minute(None);
            self.store_scoped_admission_document(
                aether_data::repository::admission::AdmissionScopeKind::User,
                user_id,
                &document,
            )
            .await?;
        }
        Ok(updated)
    }

    pub(crate) async fn touch_auth_user_last_login(
        &self,
        user_id: &str,
        logged_in_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(false);
        };
        repository
            .touch_auth_user_last_login(user_id, logged_in_at)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn get_or_create_ldap_auth_user(
        &self,
        email: String,
        username: String,
        ldap_dn: Option<String>,
        ldap_username: Option<String>,
        logged_in_at: chrono::DateTime<chrono::Utc>,
        initial_balance_usd: f64,
        unlimited: bool,
    ) -> Result<Option<StoredUserAuthRecord>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(None);
        };
        let Some(outcome) = repository
            .get_or_create_ldap_auth_user(email, username, ldap_dn, ldap_username, logged_in_at)
            .await?
        else {
            return Ok(None);
        };
        if outcome.created {
            match self
                .initialize_auth_user_wallet(&outcome.user.id, initial_balance_usd, unlimited)
                .await
            {
                Ok(Some(_wallet)) => {}
                Ok(None) => {
                    let _ = self.delete_local_auth_user(&outcome.user.id).await;
                    return Ok(None);
                }
                Err(err) => {
                    let _ = self.delete_local_auth_user(&outcome.user.id).await;
                    return Err(err);
                }
            }
        }
        Ok(Some(outcome.user))
    }

    #[allow(dead_code)]
    pub(crate) async fn initialize_auth_user_wallet(
        &self,
        user_id: &str,
        initial_balance_usd: f64,
        unlimited: bool,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        let Some(repository) = self.wallet_reader.as_ref() else {
            return Ok(None);
        };
        repository
            .initialize_auth_user_wallet(user_id, initial_balance_usd, unlimited)
            .await
    }

    pub(crate) async fn initialize_auth_api_key_wallet(
        &self,
        api_key_id: &str,
        initial_balance_usd: f64,
        unlimited: bool,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        let Some(repository) = self.wallet_reader.as_ref() else {
            return Ok(None);
        };
        repository
            .initialize_auth_api_key_wallet(api_key_id, initial_balance_usd, unlimited)
            .await
    }

    pub(crate) async fn update_auth_user_wallet_limit_mode(
        &self,
        user_id: &str,
        limit_mode: &str,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        let Some(repository) = self.wallet_reader.as_ref() else {
            return Ok(None);
        };
        repository
            .update_auth_user_wallet_limit_mode(user_id, limit_mode)
            .await
    }

    pub(crate) async fn update_auth_api_key_wallet_limit_mode(
        &self,
        api_key_id: &str,
        limit_mode: &str,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        let Some(repository) = self.wallet_reader.as_ref() else {
            return Ok(None);
        };
        repository
            .update_auth_api_key_wallet_limit_mode(api_key_id, limit_mode)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn update_auth_user_wallet_snapshot(
        &self,
        user_id: &str,
        balance: f64,
        limit_mode: &str,
        currency: &str,
        status: &str,
        total_consumed: f64,
        updated_at_unix_secs: Option<u64>,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        let Some(repository) = self.wallet_reader.as_ref() else {
            return Ok(None);
        };
        repository
            .update_auth_user_wallet_snapshot(
                user_id,
                balance,
                limit_mode,
                currency,
                status,
                total_consumed,
                updated_at_unix_secs,
            )
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn update_auth_api_key_wallet_snapshot(
        &self,
        api_key_id: &str,
        balance: f64,
        limit_mode: &str,
        currency: &str,
        status: &str,
        total_consumed: f64,
        updated_at_unix_secs: Option<u64>,
    ) -> Result<Option<StoredWalletSnapshot>, DataLayerError> {
        let Some(repository) = self.wallet_reader.as_ref() else {
            return Ok(None);
        };
        repository
            .update_auth_api_key_wallet_snapshot(
                api_key_id,
                balance,
                limit_mode,
                currency,
                status,
                total_consumed,
                updated_at_unix_secs,
            )
            .await
    }

    pub(crate) async fn count_active_admin_users(&self) -> Result<u64, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(0);
        };
        repository.count_active_admin_users().await
    }

    pub(crate) async fn count_active_local_admin_users_with_valid_password(
        &self,
    ) -> Result<u64, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(0);
        };
        repository
            .count_active_local_admin_users_with_valid_password()
            .await
    }

    pub(crate) async fn delete_local_auth_user(
        &self,
        user_id: &str,
    ) -> Result<bool, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(false);
        };
        let api_keys = self
            .list_auth_api_key_export_records_by_user_ids(&[user_id.to_string()])
            .await?;
        let deleted = repository.delete_local_auth_user(user_id).await?;
        if deleted {
            let scopes =
                std::iter::once(aether_data::repository::admission::AdmissionPolicyScope {
                    kind: aether_data::repository::admission::AdmissionScopeKind::User,
                    subject_id: user_id.to_string(),
                })
                .chain(api_keys.into_iter().map(|api_key| {
                    aether_data::repository::admission::AdmissionPolicyScope {
                        kind: aether_data::repository::admission::AdmissionScopeKind::ApiKey,
                        subject_id: api_key.into_stored().api_key_id,
                    }
                }))
                .collect::<Vec<_>>();
            self.delete_admission_policies(&scopes).await?;
        }
        Ok(deleted)
    }

    pub(crate) async fn register_local_auth_user(
        &self,
        email: Option<String>,
        email_verified: bool,
        username: String,
        password_hash: String,
        initial_balance_usd: f64,
        unlimited: bool,
    ) -> Result<Option<(StoredUserAuthRecord, StoredWalletSnapshot)>, DataLayerError> {
        let Some(user) = self
            .create_local_auth_user(email, email_verified, username, password_hash)
            .await?
        else {
            return Ok(None);
        };

        match self
            .initialize_auth_user_wallet(&user.id, initial_balance_usd, unlimited)
            .await
        {
            Ok(Some(wallet)) => Ok(Some((user, wallet))),
            Ok(None) => {
                let _ = self.delete_local_auth_user(&user.id).await;
                Ok(None)
            }
            Err(err) => {
                let _ = self.delete_local_auth_user(&user.id).await;
                Err(err)
            }
        }
    }

    pub(crate) async fn touch_user_session(
        &self,
        user_id: &str,
        session_id: &str,
        touched_at: chrono::DateTime<chrono::Utc>,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<bool, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(false);
        };
        repository
            .touch_user_session(user_id, session_id, touched_at, ip_address, user_agent)
            .await
    }

    pub(crate) async fn update_user_session_device_label(
        &self,
        user_id: &str,
        session_id: &str,
        device_label: &str,
        updated_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(false);
        };
        repository
            .update_user_session_device_label(user_id, session_id, device_label, updated_at)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn rotate_user_session_refresh_token(
        &self,
        user_id: &str,
        session_id: &str,
        previous_refresh_token_hash: &str,
        next_refresh_token_hash: &str,
        rotated_at: chrono::DateTime<chrono::Utc>,
        expires_at: chrono::DateTime<chrono::Utc>,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<bool, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(false);
        };
        repository
            .rotate_user_session_refresh_token(
                user_id,
                session_id,
                previous_refresh_token_hash,
                next_refresh_token_hash,
                rotated_at,
                expires_at,
                ip_address,
                user_agent,
            )
            .await
    }

    pub(crate) async fn revoke_user_session(
        &self,
        user_id: &str,
        session_id: &str,
        revoked_at: chrono::DateTime<chrono::Utc>,
        reason: &str,
    ) -> Result<bool, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(false);
        };
        repository
            .revoke_user_session(user_id, session_id, revoked_at, reason)
            .await
    }

    pub(crate) async fn revoke_all_user_sessions(
        &self,
        user_id: &str,
        revoked_at: chrono::DateTime<chrono::Utc>,
        reason: &str,
    ) -> Result<u64, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(0);
        };
        repository
            .revoke_all_user_sessions(user_id, revoked_at, reason)
            .await
    }

    pub(crate) async fn list_enabled_oauth_module_providers(
        &self,
    ) -> Result<Vec<StoredOAuthProviderModuleConfig>, DataLayerError> {
        match &self.auth_module_reader {
            Some(repository) => repository.list_enabled_oauth_providers().await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn get_ldap_module_config(
        &self,
    ) -> Result<Option<StoredLdapModuleConfig>, DataLayerError> {
        match &self.auth_module_reader {
            Some(repository) => repository.get_ldap_config().await,
            None => Ok(None),
        }
    }

    pub(crate) async fn upsert_ldap_module_config(
        &self,
        config: &StoredLdapModuleConfig,
    ) -> Result<Option<StoredLdapModuleConfig>, DataLayerError> {
        match &self.auth_module_writer {
            Some(repository) => repository.upsert_ldap_config(config).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn list_oauth_provider_configs(
        &self,
    ) -> Result<Vec<StoredOAuthProviderConfig>, DataLayerError> {
        match &self.oauth_provider_reader {
            Some(repository) => repository.list_oauth_provider_configs().await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn get_oauth_provider_config(
        &self,
        provider_type: &str,
    ) -> Result<Option<StoredOAuthProviderConfig>, DataLayerError> {
        match &self.oauth_provider_reader {
            Some(repository) => repository.get_oauth_provider_config(provider_type).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn count_locked_users_if_oauth_provider_disabled(
        &self,
        provider_type: &str,
        ldap_exclusive: bool,
    ) -> Result<usize, DataLayerError> {
        match &self.oauth_provider_reader {
            Some(repository) => {
                repository
                    .count_locked_users_if_provider_disabled(provider_type, ldap_exclusive)
                    .await
            }
            None => Ok(0),
        }
    }

    pub(crate) async fn upsert_oauth_provider_config(
        &self,
        record: &UpsertOAuthProviderConfigRecord,
    ) -> Result<Option<StoredOAuthProviderConfig>, DataLayerError> {
        match &self.oauth_provider_writer {
            Some(repository) => repository
                .upsert_oauth_provider_config(record)
                .await
                .map(Some),
            None => Ok(None),
        }
    }

    pub(crate) async fn delete_oauth_provider_config(
        &self,
        provider_type: &str,
    ) -> Result<bool, DataLayerError> {
        match &self.oauth_provider_writer {
            Some(repository) => repository.delete_oauth_provider_config(provider_type).await,
            None => Ok(false),
        }
    }

    pub(crate) async fn list_management_tokens(
        &self,
        query: &ManagementTokenListQuery,
    ) -> Result<StoredManagementTokenListPage, DataLayerError> {
        match &self.management_token_reader {
            Some(repository) => repository.list_management_tokens(query).await,
            None => Ok(StoredManagementTokenListPage {
                items: Vec::new(),
                total: 0,
            }),
        }
    }

    pub(crate) async fn get_management_token_with_user(
        &self,
        token_id: &str,
    ) -> Result<Option<StoredManagementTokenWithUser>, DataLayerError> {
        match &self.management_token_reader {
            Some(repository) => repository.get_management_token_with_user(token_id).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn get_management_token_with_user_by_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<StoredManagementTokenWithUser>, DataLayerError> {
        match &self.management_token_reader {
            Some(repository) => {
                repository
                    .get_management_token_with_user_by_hash(token_hash)
                    .await
            }
            None => Ok(None),
        }
    }

    pub(crate) async fn create_management_token(
        &self,
        record: &CreateManagementTokenRecord,
    ) -> Result<LocalMutationOutcome<StoredManagementToken>, DataLayerError> {
        match &self.management_token_writer {
            Some(repository) => match repository.create_management_token(record).await {
                Ok(token) => Ok(LocalMutationOutcome::Applied(token)),
                Err(DataLayerError::InvalidInput(detail)) => {
                    Ok(LocalMutationOutcome::Invalid(detail))
                }
                Err(err) => Err(err),
            },
            None => Ok(LocalMutationOutcome::Unavailable),
        }
    }

    pub(crate) async fn update_management_token(
        &self,
        record: &UpdateManagementTokenRecord,
    ) -> Result<LocalMutationOutcome<StoredManagementToken>, DataLayerError> {
        match &self.management_token_writer {
            Some(repository) => match repository.update_management_token(record).await {
                Ok(Some(token)) => Ok(LocalMutationOutcome::Applied(token)),
                Ok(None) => Ok(LocalMutationOutcome::NotFound),
                Err(DataLayerError::InvalidInput(detail)) => {
                    Ok(LocalMutationOutcome::Invalid(detail))
                }
                Err(err) => Err(err),
            },
            None => Ok(LocalMutationOutcome::Unavailable),
        }
    }

    pub(crate) async fn delete_management_token(
        &self,
        token_id: &str,
    ) -> Result<bool, DataLayerError> {
        match &self.management_token_writer {
            Some(repository) => repository.delete_management_token(token_id).await,
            None => Ok(false),
        }
    }

    pub(crate) async fn record_management_token_usage(
        &self,
        token_id: &str,
        last_used_ip: Option<&str>,
    ) -> Result<Option<StoredManagementToken>, DataLayerError> {
        if let Some(repository) = &self.usage_writer {
            let enqueued = repository
                .enqueue_management_token_counter_delta(ManagementTokenCounterDelta {
                    token_id: token_id.to_string(),
                    usage_count_delta: 1,
                    last_used_at_unix_secs: Some(chrono::Utc::now().timestamp().max(0) as u64),
                    last_used_ip: last_used_ip.map(ToOwned::to_owned),
                })
                .await?;
            if enqueued {
                return Ok(None);
            }
        }

        match &self.management_token_writer {
            Some(repository) => {
                repository
                    .record_management_token_usage(token_id, last_used_ip)
                    .await
            }
            None => Ok(None),
        }
    }

    pub(crate) async fn set_management_token_active(
        &self,
        token_id: &str,
        is_active: bool,
    ) -> Result<Option<StoredManagementToken>, DataLayerError> {
        match &self.management_token_writer {
            Some(repository) => {
                repository
                    .set_management_token_active(token_id, is_active)
                    .await
            }
            None => Ok(None),
        }
    }

    pub(crate) async fn regenerate_management_token_secret(
        &self,
        mutation: &RegenerateManagementTokenSecret,
    ) -> Result<LocalMutationOutcome<StoredManagementToken>, DataLayerError> {
        match &self.management_token_writer {
            Some(repository) => match repository
                .regenerate_management_token_secret(mutation)
                .await
            {
                Ok(Some(token)) => Ok(LocalMutationOutcome::Applied(token)),
                Ok(None) => Ok(LocalMutationOutcome::NotFound),
                Err(DataLayerError::InvalidInput(detail)) => {
                    Ok(LocalMutationOutcome::Invalid(detail))
                }
                Err(err) => Err(err),
            },
            None => Ok(LocalMutationOutcome::Unavailable),
        }
    }

    pub(in crate::data) async fn find_auth_api_key_snapshot(
        &self,
        key: AuthApiKeyLookupKey<'_>,
    ) -> Result<Option<StoredAuthApiKeySnapshot>, DataLayerError> {
        match &self.auth_api_key_reader {
            Some(repository) => repository.find_api_key_snapshot(key).await,
            None => Ok(None),
        }
    }

    pub(crate) async fn list_auth_api_key_snapshots_by_ids(
        &self,
        api_key_ids: &[String],
    ) -> Result<Vec<StoredAuthApiKeySnapshot>, DataLayerError> {
        match &self.auth_api_key_reader {
            Some(repository) => repository.list_api_key_snapshots_by_ids(api_key_ids).await,
            None => Ok(Vec::new()),
        }
    }

    pub(crate) async fn list_auth_api_key_export_records_by_user_ids(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        let records = match &self.auth_api_key_reader {
            Some(repository) => repository.list_export_api_keys_by_user_ids(user_ids).await,
            None => Ok(Vec::new()),
        }?;
        self.enrich_api_key_export_records(records).await
    }

    pub(crate) async fn list_auth_api_key_export_records_by_ids(
        &self,
        api_key_ids: &[String],
    ) -> Result<Vec<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        let records = match &self.auth_api_key_reader {
            Some(repository) => repository.list_export_api_keys_by_ids(api_key_ids).await,
            None => Ok(Vec::new()),
        }?;
        self.enrich_api_key_export_records(records).await
    }

    pub(crate) async fn read_auth_api_key_feature_settings(
        &self,
        user_id: &str,
        api_key_id: &str,
        is_standalone: bool,
    ) -> Result<Option<serde_json::Value>, DataLayerError> {
        if is_standalone {
            return Ok(self
                .find_auth_api_key_export_standalone_record_by_id(api_key_id)
                .await?
                .and_then(|record| record.into_stored().feature_settings));
        }

        Ok(self
            .list_auth_api_key_export_records_by_ids(&[api_key_id.to_string()])
            .await?
            .into_iter()
            .find(|record| record.user_id == user_id && !record.is_standalone)
            .and_then(|record| record.into_stored().feature_settings))
    }

    pub(crate) async fn list_auth_api_key_export_records_by_name_search(
        &self,
        name_search: &str,
    ) -> Result<Vec<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        let records = match &self.auth_api_key_reader {
            Some(repository) => {
                repository
                    .list_export_api_keys_by_name_search(name_search)
                    .await
            }
            None => Ok(Vec::new()),
        }?;
        self.enrich_api_key_export_records(records).await
    }

    pub(crate) async fn list_auth_api_key_export_standalone_records_page(
        &self,
        query: &aether_data::repository::auth::StandaloneApiKeyExportListQuery,
    ) -> Result<Vec<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        let records = match &self.auth_api_key_reader {
            Some(repository) => repository.list_export_standalone_api_keys_page(query).await,
            None => Ok(Vec::new()),
        }?;
        self.enrich_api_key_export_records(records).await
    }

    pub(crate) async fn count_auth_api_key_export_standalone_records(
        &self,
        is_active: Option<bool>,
    ) -> Result<u64, DataLayerError> {
        match &self.auth_api_key_reader {
            Some(repository) => repository.count_export_standalone_api_keys(is_active).await,
            None => Ok(0),
        }
    }

    pub(crate) async fn summarize_auth_api_key_export_records_by_user_ids(
        &self,
        user_ids: &[String],
        now_unix_secs: u64,
    ) -> Result<aether_data::repository::auth::AuthApiKeyExportSummary, DataLayerError> {
        match &self.auth_api_key_reader {
            Some(repository) => {
                repository
                    .summarize_export_api_keys_by_user_ids(user_ids, now_unix_secs)
                    .await
            }
            None => Ok(aether_data::repository::auth::AuthApiKeyExportSummary::default()),
        }
    }

    pub(crate) async fn summarize_auth_api_key_export_non_standalone_records(
        &self,
        now_unix_secs: u64,
    ) -> Result<aether_data::repository::auth::AuthApiKeyExportSummary, DataLayerError> {
        match &self.auth_api_key_reader {
            Some(repository) => {
                repository
                    .summarize_export_non_standalone_api_keys(now_unix_secs)
                    .await
            }
            None => Ok(aether_data::repository::auth::AuthApiKeyExportSummary::default()),
        }
    }

    pub(crate) async fn list_auth_api_key_export_standalone_records(
        &self,
    ) -> Result<Vec<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        let records = match &self.auth_api_key_reader {
            Some(repository) => repository.list_export_standalone_api_keys().await,
            None => Ok(Vec::new()),
        }?;
        self.enrich_api_key_export_records(records).await
    }

    pub(crate) async fn summarize_auth_api_key_export_standalone_records(
        &self,
        now_unix_secs: u64,
    ) -> Result<aether_data::repository::auth::AuthApiKeyExportSummary, DataLayerError> {
        match &self.auth_api_key_reader {
            Some(repository) => {
                repository
                    .summarize_export_standalone_api_keys(now_unix_secs)
                    .await
            }
            None => Ok(aether_data::repository::auth::AuthApiKeyExportSummary::default()),
        }
    }

    pub(crate) async fn find_auth_api_key_export_standalone_record_by_id(
        &self,
        api_key_id: &str,
    ) -> Result<Option<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        let record = match &self.auth_api_key_reader {
            Some(repository) => {
                repository
                    .find_export_standalone_api_key_by_id(api_key_id)
                    .await
            }
            None => Ok(None),
        }?;
        match record {
            Some(record) => self.enrich_api_key_export_record(record).await.map(Some),
            None => Ok(None),
        }
    }

    pub(crate) async fn create_user_api_key(
        &self,
        record: aether_data::repository::auth::CreateUserApiKeyRecord,
        daily_usage_limit_usd: Option<f64>,
    ) -> Result<Option<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        let scope_id = record.api_key_id.clone();
        let user_id = record.user_id.clone();
        let document = aether_data::repository::admission::AdmissionPolicyDocument::default()
            .with_requests_per_minute(Some(record.rate_limit.max(0) as u32))
            .with_concurrent_requests(record.concurrent_limit.map(|value| value.max(0) as u32))
            .with_daily_usage_limit_usd(daily_usage_limit_usd);
        let created = match &self.auth_api_key_writer {
            Some(repository) => repository.create_user_api_key(record).await,
            None => Ok(None),
        }?;
        let Some(created) = created else {
            return Ok(None);
        };
        if let Err(error) = self
            .store_scoped_admission_document(
                aether_data::repository::admission::AdmissionScopeKind::ApiKey,
                &scope_id,
                &document,
            )
            .await
        {
            if let Some(repository) = &self.auth_api_key_writer {
                let _ = repository.delete_user_api_key(&user_id, &scope_id).await;
            }
            return Err(error);
        }
        self.enrich_api_key_export_record(created).await.map(Some)
    }

    pub(crate) async fn create_standalone_api_key(
        &self,
        record: aether_data::repository::auth::CreateStandaloneApiKeyRecord,
        daily_usage_limit_usd: Option<f64>,
    ) -> Result<Option<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        let scope_id = record.api_key_id.clone();
        let document = aether_data::repository::admission::AdmissionPolicyDocument::default()
            .with_requests_per_minute(record.rate_limit.map(|value| value.max(0) as u32))
            .with_concurrent_requests(record.concurrent_limit.map(|value| value.max(0) as u32))
            .with_daily_usage_limit_usd(daily_usage_limit_usd);
        let created = match &self.auth_api_key_writer {
            Some(repository) => repository.create_standalone_api_key(record).await,
            None => Ok(None),
        }?;
        let Some(created) = created else {
            return Ok(None);
        };
        if let Err(error) = self
            .store_scoped_admission_document(
                aether_data::repository::admission::AdmissionScopeKind::ApiKey,
                &scope_id,
                &document,
            )
            .await
        {
            if let Some(repository) = &self.auth_api_key_writer {
                let _ = repository.delete_standalone_api_key(&scope_id).await;
            }
            return Err(error);
        }
        self.enrich_api_key_export_record(created).await.map(Some)
    }

    pub(crate) async fn update_user_api_key_basic(
        &self,
        record: aether_data::repository::auth::UpdateUserApiKeyBasicRecord,
        daily_usage_limit_usd: Option<Option<f64>>,
    ) -> Result<Option<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        let mut document = self
            .scoped_admission_document(
                aether_data::repository::admission::AdmissionScopeKind::ApiKey,
                &record.api_key_id,
            )
            .await?;
        if let Some(rate_limit) = record.rate_limit {
            document = document.with_requests_per_minute(Some(rate_limit.max(0) as u32));
        }
        if let Some(concurrent_limit) = record.concurrent_limit {
            document = document.with_concurrent_requests(Some(concurrent_limit.max(0) as u32));
        }
        if let Some(daily_usage_limit_usd) = daily_usage_limit_usd {
            document = document.with_daily_usage_limit_usd(daily_usage_limit_usd);
        }
        let api_key_id = record.api_key_id.clone();
        let updated = match &self.auth_api_key_writer {
            Some(repository) => repository.update_user_api_key_basic(record).await,
            None => Ok(None),
        }?;
        let Some(updated) = updated else {
            return Ok(None);
        };
        self.store_scoped_admission_document(
            aether_data::repository::admission::AdmissionScopeKind::ApiKey,
            &api_key_id,
            &document,
        )
        .await?;
        self.enrich_api_key_export_record(updated).await.map(Some)
    }

    pub(crate) async fn update_standalone_api_key_basic(
        &self,
        record: aether_data::repository::auth::UpdateStandaloneApiKeyBasicRecord,
        daily_usage_limit_usd: Option<Option<f64>>,
    ) -> Result<Option<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        let mut document = self
            .scoped_admission_document(
                aether_data::repository::admission::AdmissionScopeKind::ApiKey,
                &record.api_key_id,
            )
            .await?;
        if record.rate_limit_present {
            document = document
                .with_requests_per_minute(record.rate_limit.map(|value| value.max(0) as u32));
        }
        if record.concurrent_limit_present {
            document = document
                .with_concurrent_requests(record.concurrent_limit.map(|value| value.max(0) as u32));
        }
        if let Some(daily_usage_limit_usd) = daily_usage_limit_usd {
            document = document.with_daily_usage_limit_usd(daily_usage_limit_usd);
        }
        let api_key_id = record.api_key_id.clone();
        let updated = match &self.auth_api_key_writer {
            Some(repository) => repository.update_standalone_api_key_basic(record).await,
            None => Ok(None),
        }?;
        let Some(updated) = updated else {
            return Ok(None);
        };
        self.store_scoped_admission_document(
            aether_data::repository::admission::AdmissionScopeKind::ApiKey,
            &api_key_id,
            &document,
        )
        .await?;
        self.enrich_api_key_export_record(updated).await.map(Some)
    }

    pub(crate) async fn set_user_api_key_active(
        &self,
        user_id: &str,
        api_key_id: &str,
        is_active: bool,
    ) -> Result<Option<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        let record = match &self.auth_api_key_writer {
            Some(repository) => {
                repository
                    .set_user_api_key_active(user_id, api_key_id, is_active)
                    .await
            }
            None => Ok(None),
        }?;
        self.enrich_optional_api_key_export_record(record).await
    }

    pub(crate) async fn set_standalone_api_key_active(
        &self,
        api_key_id: &str,
        is_active: bool,
    ) -> Result<Option<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        let record = match &self.auth_api_key_writer {
            Some(repository) => {
                repository
                    .set_standalone_api_key_active(api_key_id, is_active)
                    .await
            }
            None => Ok(None),
        }?;
        self.enrich_optional_api_key_export_record(record).await
    }

    pub(crate) async fn set_user_api_key_locked(
        &self,
        user_id: &str,
        api_key_id: &str,
        is_locked: bool,
    ) -> Result<bool, DataLayerError> {
        match &self.auth_api_key_writer {
            Some(repository) => {
                repository
                    .set_user_api_key_locked(user_id, api_key_id, is_locked)
                    .await
            }
            None => Ok(false),
        }
    }

    pub(crate) async fn set_user_api_key_allowed_providers(
        &self,
        user_id: &str,
        api_key_id: &str,
        allowed_providers: Option<Vec<String>>,
    ) -> Result<Option<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        let record = match &self.auth_api_key_writer {
            Some(repository) => {
                repository
                    .set_user_api_key_allowed_providers(user_id, api_key_id, allowed_providers)
                    .await
            }
            None => Ok(None),
        }?;
        self.enrich_optional_api_key_export_record(record).await
    }

    pub(crate) async fn set_user_api_key_force_capabilities(
        &self,
        user_id: &str,
        api_key_id: &str,
        force_capabilities: Option<serde_json::Value>,
    ) -> Result<Option<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        let record = match &self.auth_api_key_writer {
            Some(repository) => {
                repository
                    .set_user_api_key_force_capabilities(user_id, api_key_id, force_capabilities)
                    .await
            }
            None => Ok(None),
        }?;
        self.enrich_optional_api_key_export_record(record).await
    }

    pub(crate) async fn set_user_api_key_feature_settings(
        &self,
        user_id: &str,
        api_key_id: &str,
        feature_settings: Option<serde_json::Value>,
    ) -> Result<Option<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        let record = match &self.auth_api_key_writer {
            Some(repository) => {
                repository
                    .set_user_api_key_feature_settings(user_id, api_key_id, feature_settings)
                    .await
            }
            None => Ok(None),
        }?;
        self.enrich_optional_api_key_export_record(record).await
    }

    pub(crate) async fn set_api_key_usage_totals(
        &self,
        api_key_id: &str,
        total_requests: u64,
        total_tokens: u64,
        total_cost_usd: f64,
    ) -> Result<Option<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        let record = match &self.auth_api_key_writer {
            Some(repository) => {
                repository
                    .set_api_key_usage_totals(
                        api_key_id,
                        total_requests,
                        total_tokens,
                        total_cost_usd,
                    )
                    .await
            }
            None => Ok(None),
        }?;
        self.enrich_optional_api_key_export_record(record).await
    }

    pub(crate) async fn set_standalone_api_key_feature_settings(
        &self,
        api_key_id: &str,
        feature_settings: Option<serde_json::Value>,
    ) -> Result<Option<GatewayAuthApiKeyExportRecord>, DataLayerError> {
        let record = match &self.auth_api_key_writer {
            Some(repository) => {
                repository
                    .set_standalone_api_key_feature_settings(api_key_id, feature_settings)
                    .await
            }
            None => Ok(None),
        }?;
        self.enrich_optional_api_key_export_record(record).await
    }

    pub(crate) async fn delete_user_api_key(
        &self,
        user_id: &str,
        api_key_id: &str,
    ) -> Result<bool, DataLayerError> {
        let deleted = match &self.auth_api_key_writer {
            Some(repository) => repository.delete_user_api_key(user_id, api_key_id).await,
            None => Ok(false),
        }?;
        if deleted {
            self.delete_admission_policy(
                &aether_data::repository::admission::AdmissionPolicyScope {
                    kind: aether_data::repository::admission::AdmissionScopeKind::ApiKey,
                    subject_id: api_key_id.to_string(),
                },
            )
            .await?;
        }
        Ok(deleted)
    }

    pub(crate) async fn delete_standalone_api_key(
        &self,
        api_key_id: &str,
    ) -> Result<bool, DataLayerError> {
        let deleted = match &self.auth_api_key_writer {
            Some(repository) => repository.delete_standalone_api_key(api_key_id).await,
            None => Ok(false),
        }?;
        if deleted {
            self.delete_admission_policy(
                &aether_data::repository::admission::AdmissionPolicyScope {
                    kind: aether_data::repository::admission::AdmissionScopeKind::ApiKey,
                    subject_id: api_key_id.to_string(),
                },
            )
            .await?;
        }
        Ok(deleted)
    }

    pub(crate) async fn read_auth_api_key_snapshot(
        &self,
        user_id: &str,
        api_key_id: &str,
        now_unix_secs: u64,
    ) -> Result<Option<GatewayAuthApiKeySnapshot>, DataLayerError> {
        let snapshot = crate::request_diagnostics::observe_db_operation(
            "auth_api_key_snapshot",
            self.database_pool_summary(),
            self.find_stored_auth_api_key_snapshot(AuthApiKeyLookupKey::UserApiKeyIds {
                user_id,
                api_key_id,
            }),
        )
        .await?;
        self.apply_user_group_effective_policies(snapshot, now_unix_secs)
            .await
    }

    pub(crate) async fn read_auth_api_key_snapshot_by_key_hash(
        &self,
        key_hash: &str,
        now_unix_secs: u64,
    ) -> Result<Option<GatewayAuthApiKeySnapshot>, DataLayerError> {
        let snapshot = crate::request_diagnostics::observe_db_operation(
            "auth_api_key_snapshot_by_hash",
            self.database_pool_summary(),
            self.find_stored_auth_api_key_snapshot(AuthApiKeyLookupKey::KeyHash(key_hash)),
        )
        .await?;
        self.apply_user_group_effective_policies(snapshot, now_unix_secs)
            .await
    }

    async fn apply_user_group_effective_policies(
        &self,
        snapshot: Option<StoredAuthApiKeySnapshot>,
        now_unix_secs: u64,
    ) -> Result<Option<GatewayAuthApiKeySnapshot>, DataLayerError> {
        let Some(mut snapshot) = snapshot else {
            return Ok(None);
        };
        let groups = if let Some(repository) = self.user_reader.as_ref() {
            if let Some(user) = crate::request_diagnostics::observe_db_operation(
                "auth_user_policy",
                self.database_pool_summary(),
                repository.find_user_auth_by_id(&snapshot.user_id),
            )
            .await?
            {
                snapshot.user_role = user.role;
            }
            self.effective_user_groups_for_user(&snapshot.user_id)
                .await?
        } else {
            Vec::new()
        };

        let GatewayUserEffectiveListPolicies {
            allowed_providers,
            allowed_api_formats,
            allowed_models,
        } = resolve_group_effective_list_policies(&groups);
        snapshot.user_allowed_providers = allowed_providers;
        snapshot.user_allowed_api_formats = allowed_api_formats;
        snapshot.user_allowed_models = allowed_models;
        let admission_policy = self
            .resolve_admission_policy(
                &snapshot.user_id,
                &snapshot.api_key_id,
                &groups
                    .iter()
                    .map(|group| group.id.clone())
                    .collect::<Vec<_>>(),
                snapshot.api_key_is_standalone,
            )
            .await?;
        let mut resolved = GatewayAuthApiKeySnapshot::from_stored(snapshot, now_unix_secs);
        resolved.apply_admission_policy(admission_policy);
        Ok(Some(resolved))
    }

    pub(crate) async fn resolve_user_effective_list_policies(
        &self,
        user: &StoredUserAuthRecord,
    ) -> Result<GatewayUserEffectiveListPolicies, DataLayerError> {
        if user.role.eq_ignore_ascii_case("admin") {
            return Ok(GatewayUserEffectiveListPolicies::default());
        }

        let groups = if self.user_reader.is_some() {
            self.effective_user_groups_for_user(&user.id).await?
        } else {
            Vec::new()
        };
        Ok(resolve_group_effective_list_policies(&groups))
    }

    async fn effective_user_groups_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<aether_data::repository::users::StoredUserGroup>, DataLayerError> {
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(Vec::new());
        };
        let mut groups = repository.list_user_groups_for_user(user_id).await?;
        groups.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(groups)
    }

    pub(crate) async fn user_group_policy_sets_for_user(
        &self,
        user_id: &str,
    ) -> Result<GatewayUserGroupPolicySets, DataLayerError> {
        Ok(self
            .user_group_policy_sets_for_users(&[user_id.to_string()])
            .await?
            .remove(user_id)
            .unwrap_or_default())
    }

    pub(crate) async fn user_group_policy_sets_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<std::collections::BTreeMap<String, GatewayUserGroupPolicySets>, DataLayerError>
    {
        let mut group_ids_by_user = user_ids
            .iter()
            .cloned()
            .map(|user_id| (user_id, std::collections::BTreeSet::new()))
            .collect::<std::collections::BTreeMap<_, _>>();
        if user_ids.is_empty() {
            return Ok(std::collections::BTreeMap::new());
        }
        let Some(repository) = self.user_reader.as_ref() else {
            return Ok(group_ids_by_user
                .into_keys()
                .map(|user_id| (user_id, GatewayUserGroupPolicySets::default()))
                .collect());
        };

        for membership in repository
            .list_user_group_memberships_by_user_ids(user_ids)
            .await?
        {
            if let Some(group_ids) = group_ids_by_user.get_mut(&membership.user_id) {
                group_ids.insert(membership.group_id);
            }
        }

        let all_group_ids = group_ids_by_user
            .values()
            .flatten()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let groups_by_id = self
            .enrich_user_groups(repository.list_user_groups_by_ids(&all_group_ids).await?)
            .await?
            .into_iter()
            .map(|group| (group.id.clone(), group))
            .collect::<std::collections::BTreeMap<_, _>>();

        Ok(group_ids_by_user
            .into_iter()
            .map(|(user_id, group_ids)| {
                let mut groups = group_ids
                    .into_iter()
                    .filter_map(|group_id| groups_by_id.get(&group_id).cloned())
                    .collect::<Vec<_>>();
                groups.sort_by(|left, right| {
                    left.name
                        .cmp(&right.name)
                        .then_with(|| left.id.cmp(&right.id))
                });
                (
                    user_id,
                    GatewayUserGroupPolicySets {
                        assigned_groups: groups.clone(),
                        effective_groups: groups,
                    },
                )
            })
            .collect())
    }
}

// Per-user list policy columns are retained only for legacy import/export compatibility.
// Runtime authorization and user-facing catalogs must both treat group policies as authoritative.
fn resolve_group_effective_list_policies(
    groups: &[aether_data::repository::users::StoredUserGroup],
) -> GatewayUserEffectiveListPolicies {
    GatewayUserEffectiveListPolicies {
        allowed_providers: resolve_effective_list_policy(None, "unrestricted", groups, |group| {
            (
                &group.allowed_providers_mode,
                group.allowed_providers.clone(),
            )
        }),
        allowed_api_formats: resolve_effective_api_format_policy(
            None,
            "unrestricted",
            groups,
            |group| {
                (
                    &group.allowed_api_formats_mode,
                    group.allowed_api_formats.clone(),
                )
            },
        ),
        allowed_models: resolve_effective_list_policy(None, "unrestricted", groups, |group| {
            (&group.allowed_models_mode, group.allowed_models.clone())
        }),
    }
}

fn resolve_effective_list_policy(
    user_values: Option<Vec<String>>,
    user_mode: &str,
    groups: &[aether_data::repository::users::StoredUserGroup],
    group_field: impl Fn(
        &aether_data::repository::users::StoredUserGroup,
    ) -> (&str, Option<Vec<String>>),
) -> Option<Vec<String>> {
    let group_policy = union_group_list_policies(groups, group_field);
    let user_policy = list_restriction_from_mode(user_mode, user_values);
    intersect_list_policies(group_policy, user_policy)
}

fn resolve_effective_api_format_policy(
    user_values: Option<Vec<String>>,
    user_mode: &str,
    groups: &[aether_data::repository::users::StoredUserGroup],
    group_field: impl Fn(
        &aether_data::repository::users::StoredUserGroup,
    ) -> (&str, Option<Vec<String>>),
) -> Option<Vec<String>> {
    let group_policy = union_group_list_policies(groups, group_field);
    let user_policy = list_restriction_from_mode(user_mode, user_values);
    intersect_api_format_list_policies(group_policy, user_policy)
}

fn union_group_list_policies(
    groups: &[aether_data::repository::users::StoredUserGroup],
    group_field: impl Fn(
        &aether_data::repository::users::StoredUserGroup,
    ) -> (&str, Option<Vec<String>>),
) -> Option<Vec<String>> {
    let mut saw_restrictive_group = false;
    let mut values = std::collections::BTreeSet::new();

    for group in groups {
        let (mode, group_values) = group_field(group);
        match mode {
            "unrestricted" => return None,
            "specific" => {
                saw_restrictive_group = true;
                values.extend(group_values.unwrap_or_default());
            }
            "deny_all" => {
                saw_restrictive_group = true;
            }
            _ => {}
        }
    }

    saw_restrictive_group.then(|| values.into_iter().collect())
}

fn list_restriction_from_mode(mode: &str, values: Option<Vec<String>>) -> Option<Vec<String>> {
    match mode {
        "specific" => Some(values.unwrap_or_default()),
        "deny_all" => Some(Vec::new()),
        _ => None,
    }
}

fn intersect_list_policies(
    left: Option<Vec<String>>,
    right: Option<Vec<String>>,
) -> Option<Vec<String>> {
    match (left, right) {
        (None, None) => None,
        (Some(values), None) | (None, Some(values)) => Some(values),
        (Some(left_values), Some(right_values)) => {
            let right_values = right_values
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>();
            Some(
                left_values
                    .into_iter()
                    .filter(|value| right_values.contains(value))
                    .collect(),
            )
        }
    }
}

fn intersect_api_format_list_policies(
    left: Option<Vec<String>>,
    right: Option<Vec<String>>,
) -> Option<Vec<String>> {
    match (left, right) {
        (None, None) => None,
        (Some(values), None) | (None, Some(values)) => Some(values),
        (Some(left_values), Some(right_values)) => Some(
            crate::ai_serving::intersect_api_format_allowed_lists(&left_values, &right_values),
        ),
    }
}
