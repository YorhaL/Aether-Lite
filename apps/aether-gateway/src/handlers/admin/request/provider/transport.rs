use super::*;

impl<'a> AdminAppState<'a> {
    pub(crate) async fn read_provider_transport_snapshot_uncached(
        &self,
        provider_id: &str,
        endpoint_id: &str,
        key_id: &str,
    ) -> Result<Option<AdminGatewayProviderTransportSnapshot>, GatewayError> {
        self.app
            .read_provider_transport_snapshot_uncached(provider_id, endpoint_id, key_id)
            .await
    }

    pub(crate) async fn read_provider_transport_snapshot(
        &self,
        provider_id: &str,
        endpoint_id: &str,
        key_id: &str,
    ) -> Result<Option<AdminGatewayProviderTransportSnapshot>, GatewayError> {
        self.app
            .read_provider_transport_snapshot(provider_id, endpoint_id, key_id)
            .await
    }

    pub(crate) fn supports_local_gemini_transport(
        &self,
        transport: &AdminGatewayProviderTransportSnapshot,
        api_format: &str,
    ) -> bool {
        crate::provider_transport::policy::supports_local_gemini_transport(transport, api_format)
    }

    pub(crate) fn resolve_local_gemini_auth(
        &self,
        transport: &AdminGatewayProviderTransportSnapshot,
    ) -> Option<(String, String)> {
        crate::provider_transport::auth::resolve_local_gemini_auth(transport)
    }

    pub(crate) fn build_passthrough_headers_with_auth(
        &self,
        headers: &axum::http::HeaderMap,
        auth_header: &str,
        auth_value: &str,
        extra_headers: &BTreeMap<String, String>,
    ) -> BTreeMap<String, String> {
        crate::provider_transport::auth::build_passthrough_headers_with_auth(
            headers,
            auth_header,
            auth_value,
            extra_headers,
        )
    }

    pub(crate) fn apply_local_header_rules(
        &self,
        headers: &mut BTreeMap<String, String>,
        rules: Option<&serde_json::Value>,
        protected_keys: &[&str],
        body: &serde_json::Value,
        original_body: Option<&serde_json::Value>,
    ) -> bool {
        crate::provider_transport::apply_local_header_rules(
            headers,
            rules,
            protected_keys,
            body,
            original_body,
        )
    }

    pub(crate) fn resolve_transport_profile(
        &self,
        transport: &AdminGatewayProviderTransportSnapshot,
    ) -> Option<aether_contracts::ResolvedTransportProfile> {
        crate::provider_transport::resolve_transport_profile(transport)
    }

    pub(crate) fn resolve_transport_execution_timeouts(
        &self,
        transport: &AdminGatewayProviderTransportSnapshot,
    ) -> Option<aether_contracts::ExecutionTimeouts> {
        crate::provider_transport::resolve_transport_execution_timeouts(transport)
    }

    pub(crate) fn build_passthrough_path_url(
        &self,
        upstream_base_url: &str,
        path: &str,
        query: Option<&str>,
        blocked_keys: &[&str],
    ) -> Option<String> {
        crate::provider_transport::url::build_passthrough_path_url(
            upstream_base_url,
            path,
            query,
            blocked_keys,
        )
    }

    pub(crate) fn build_claude_messages_url(
        &self,
        upstream_base_url: &str,
        query: Option<&str>,
    ) -> String {
        crate::provider_transport::url::build_claude_messages_url(upstream_base_url, query)
    }

    pub(crate) fn build_gemini_content_url(
        &self,
        upstream_base_url: &str,
        model: &str,
        stream: bool,
        query: Option<&str>,
    ) -> Option<String> {
        crate::provider_transport::url::build_gemini_content_url(
            upstream_base_url,
            model,
            stream,
            query,
        )
    }

    pub(crate) fn build_openai_chat_url(
        &self,
        upstream_base_url: &str,
        query: Option<&str>,
    ) -> String {
        crate::provider_transport::url::build_openai_chat_url(upstream_base_url, query)
    }
}
