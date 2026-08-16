use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OAuthAuthorizeResponse {
    pub authorize_url: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_challenge: Option<String>,
}
