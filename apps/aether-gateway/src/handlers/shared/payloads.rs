use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
pub(crate) struct InternalGatewayResolveRequest {
    #[serde(default)]
    pub(crate) trace_id: Option<String>,
    pub(crate) method: String,
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) query_string: Option<String>,
    #[serde(default)]
    pub(crate) headers: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct InternalGatewayAuthContextRequest {
    #[serde(default)]
    pub(crate) trace_id: Option<String>,
    #[serde(default)]
    pub(crate) query_string: Option<String>,
    #[serde(default)]
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) auth_endpoint_signature: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct InternalGatewayExecuteRequest {
    #[serde(default)]
    pub(crate) trace_id: Option<String>,
    pub(crate) method: String,
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) query_string: Option<String>,
    #[serde(default)]
    pub(crate) headers: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) body_json: serde_json::Value,
    #[serde(default)]
    pub(crate) body_base64: Option<String>,
    #[serde(default)]
    pub(crate) auth_context: Option<crate::control::GatewayControlAuthContext>,
}
