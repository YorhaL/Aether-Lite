use crate::core::OAuthError;
use aether_contracts::ResolvedTransportProfile;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::BTreeMap;

use super::OAuthNetworkContext;

#[derive(Clone, PartialEq)]
pub struct OAuthHttpRequest {
    pub request_id: String,
    pub method: reqwest::Method,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub content_type: Option<String>,
    pub json_body: Option<Value>,
    pub body_bytes: Option<Vec<u8>>,
    pub network: OAuthNetworkContext,
    pub transport_profile: Option<ResolvedTransportProfile>,
}

impl std::fmt::Debug for OAuthHttpRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OAuthHttpRequest")
            .field("request_id", &self.request_id)
            .field("method", &self.method)
            .field("url", &self.url)
            .field("header_names", &self.headers.keys().collect::<Vec<_>>())
            .field("content_type", &self.content_type)
            .field("has_json_body", &self.json_body.is_some())
            .field("body_bytes_len", &self.body_bytes.as_ref().map(Vec::len))
            .field(
                "transport_profile_id",
                &self
                    .transport_profile
                    .as_ref()
                    .map(|profile| profile.profile_id.as_str()),
            )
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OAuthHttpResponse {
    pub status_code: u16,
    pub body_text: String,
    pub json_body: Option<Value>,
}

#[async_trait]
pub trait OAuthHttpExecutor: Send + Sync {
    async fn execute(&self, request: OAuthHttpRequest) -> Result<OAuthHttpResponse, OAuthError>;
}

#[derive(Debug, Clone)]
pub struct ReqwestOAuthHttpExecutor {
    client: reqwest::Client,
}

impl ReqwestOAuthHttpExecutor {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl OAuthHttpExecutor for ReqwestOAuthHttpExecutor {
    async fn execute(&self, request: OAuthHttpRequest) -> Result<OAuthHttpResponse, OAuthError> {
        let mut builder = self
            .client
            .request(request.method.clone(), request.url.as_str());
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        if let Some(json_body) = request.json_body.as_ref() {
            builder = builder.json(json_body);
        } else if let Some(body_bytes) = request.body_bytes.as_ref() {
            builder = builder.body(body_bytes.clone());
        }

        let response = builder
            .send()
            .await
            .map_err(|err| OAuthError::transport(err.to_string()))?;
        let status_code = response.status().as_u16();
        let body_text = response
            .text()
            .await
            .map_err(|err| OAuthError::transport(err.to_string()))?;
        let json_body = serde_json::from_str::<Value>(&body_text).ok();
        Ok(OAuthHttpResponse {
            status_code,
            body_text,
            json_body,
        })
    }
}
