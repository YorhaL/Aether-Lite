use super::AdminAppState;

impl<'a> AdminAppState<'a> {
    pub(crate) fn http_client(&self) -> &reqwest::Client {
        &self.app.client
    }

    pub(crate) fn encryption_key(&self) -> Option<&str> {
        self.app.encryption_key()
    }

    pub(crate) fn encrypt_catalog_secret_with_fallbacks(&self, secret: &str) -> Option<String> {
        crate::handlers::admin::shared::encrypt_catalog_secret_with_fallbacks(self.app, secret)
    }

    pub(crate) fn decrypt_catalog_secret_with_fallbacks(&self, ciphertext: &str) -> Option<String> {
        crate::handlers::admin::shared::decrypt_catalog_secret_with_fallbacks(
            self.app.encryption_key(),
            ciphertext,
        )
    }

    pub(crate) fn has_provider_catalog_data_reader(&self) -> bool {
        self.app.has_provider_catalog_data_reader()
    }

    pub(crate) fn has_provider_catalog_data_writer(&self) -> bool {
        self.app.has_provider_catalog_data_writer()
    }

    pub(crate) fn has_request_candidate_data_reader(&self) -> bool {
        self.app.has_request_candidate_data_reader()
    }

    pub(crate) fn has_management_token_reader(&self) -> bool {
        self.app.has_management_token_reader()
    }

    pub(crate) fn has_management_token_writer(&self) -> bool {
        self.app.has_management_token_writer()
    }

    pub(crate) fn has_global_model_data_reader(&self) -> bool {
        self.app.has_global_model_data_reader()
    }

    pub(crate) fn has_global_model_data_writer(&self) -> bool {
        self.app.has_global_model_data_writer()
    }

    pub(crate) fn has_routing_group_data_reader(&self) -> bool {
        self.app.has_routing_group_data_reader()
    }

    pub(crate) fn has_routing_group_data_writer(&self) -> bool {
        self.app.has_routing_group_data_writer()
    }

    pub(crate) fn has_usage_data_reader(&self) -> bool {
        self.app.has_usage_data_reader()
    }

    pub(crate) fn has_background_task_data_reader(&self) -> bool {
        self.app.has_background_task_data_reader()
    }

    pub(crate) fn has_background_task_data_writer(&self) -> bool {
        self.app.has_background_task_data_writer()
    }

    pub(crate) fn has_auth_api_key_data_reader(&self) -> bool {
        self.app.has_auth_api_key_data_reader()
    }

    pub(crate) fn has_user_data_reader(&self) -> bool {
        self.app.has_user_data_reader()
    }

    pub(crate) fn has_auth_user_data_reader(&self) -> bool {
        self.app.has_auth_user_data_reader()
    }

    pub(crate) fn has_auth_api_key_writer(&self) -> bool {
        self.app.data.has_auth_api_key_writer()
    }

    pub(crate) fn has_auth_module_writer(&self) -> bool {
        self.app.has_auth_module_writer()
    }

    pub(crate) fn has_auth_user_write_capability(&self) -> bool {
        self.app.has_auth_user_write_capability()
    }

    pub(crate) fn has_auth_wallet_write_capability(&self) -> bool {
        self.app.has_auth_wallet_write_capability()
    }

    pub(crate) fn mark_provider_key_rpm_reset(&self, key_id: &str, now_unix_secs: u64) {
        self.app.mark_provider_key_rpm_reset(key_id, now_unix_secs)
    }

    pub(crate) fn runtime_state(&self) -> &aether_runtime_state::RuntimeState {
        self.app.runtime_state.as_ref()
    }

    pub(crate) fn provider_key_rpm_reset_at(
        &self,
        key_id: &str,
        now_unix_secs: u64,
    ) -> Option<u64> {
        self.app.provider_key_rpm_reset_at(key_id, now_unix_secs)
    }

    pub(crate) fn has_wallet_data_writer(&self) -> bool {
        self.app.has_wallet_data_writer()
    }

    pub(crate) fn mark_admin_monitoring_error_stats_reset(&self, now_unix_secs: u64) {
        self.app
            .mark_admin_monitoring_error_stats_reset(now_unix_secs);
    }

    pub(crate) fn admin_monitoring_error_stats_reset_at(&self) -> Option<u64> {
        self.app.admin_monitoring_error_stats_reset_at()
    }
}
