use axum::body::Bytes;

use crate::ai_serving::is_json_request;
use crate::ai_serving::parse_direct_request_body as parse_direct_request_body_impl;
pub(crate) use crate::ai_serving::{
    CLAUDE_CHAT_STREAM_PLAN_KIND, CLAUDE_CHAT_SYNC_PLAN_KIND, CLAUDE_CLI_STREAM_PLAN_KIND,
    CLAUDE_CLI_SYNC_PLAN_KIND, CLAUDE_COUNT_TOKENS_SYNC_PLAN_KIND, EXECUTION_RUNTIME_STREAM_ACTION,
    EXECUTION_RUNTIME_STREAM_DECISION_ACTION, EXECUTION_RUNTIME_SYNC_ACTION,
    EXECUTION_RUNTIME_SYNC_DECISION_ACTION, GEMINI_CHAT_STREAM_PLAN_KIND,
    GEMINI_CHAT_SYNC_PLAN_KIND, GEMINI_CLI_STREAM_PLAN_KIND, GEMINI_CLI_SYNC_PLAN_KIND,
    GEMINI_EMBEDDING_SYNC_PLAN_KIND, GEMINI_FILES_DELETE_PLAN_KIND,
    GEMINI_FILES_DOWNLOAD_PLAN_KIND, GEMINI_FILES_GET_PLAN_KIND, GEMINI_FILES_LIST_PLAN_KIND,
    GEMINI_FILES_UPLOAD_PLAN_KIND, GEMINI_VIDEO_CANCEL_SYNC_PLAN_KIND,
    GEMINI_VIDEO_CREATE_SYNC_PLAN_KIND, OPENAI_CHAT_STREAM_PLAN_KIND, OPENAI_CHAT_SYNC_PLAN_KIND,
    OPENAI_EMBEDDING_SYNC_PLAN_KIND, OPENAI_IMAGE_STREAM_PLAN_KIND, OPENAI_IMAGE_SYNC_PLAN_KIND,
    OPENAI_RERANK_SYNC_PLAN_KIND, OPENAI_RESPONSES_COMPACT_STREAM_PLAN_KIND,
    OPENAI_RESPONSES_COMPACT_SYNC_PLAN_KIND, OPENAI_RESPONSES_STREAM_PLAN_KIND,
    OPENAI_RESPONSES_SYNC_PLAN_KIND, OPENAI_SEARCH_SYNC_PLAN_KIND,
    OPENAI_VIDEO_CANCEL_SYNC_PLAN_KIND, OPENAI_VIDEO_CONTENT_PLAN_KIND,
    OPENAI_VIDEO_CREATE_SYNC_PLAN_KIND, OPENAI_VIDEO_DELETE_SYNC_PLAN_KIND,
    OPENAI_VIDEO_REMIX_SYNC_PLAN_KIND,
};

pub(crate) use aether_ai_serving::AiRequestedModelFamily as RequestedModelFamily;

pub(crate) fn parse_direct_request_body(
    parts: &http::request::Parts,
    body_bytes: &Bytes,
) -> Option<(serde_json::Value, Option<String>)> {
    let is_json_request = is_json_request(&parts.headers);
    let body_bytes = if is_json_request {
        crate::ai_serving::decoded_request_body_bytes(&parts.headers, body_bytes.as_ref()).ok()?
    } else {
        std::borrow::Cow::Borrowed(body_bytes.as_ref())
    };
    parse_direct_request_body_impl(is_json_request, body_bytes.as_ref())
}

pub(crate) fn extract_standard_requested_model(body_json: &serde_json::Value) -> Option<String> {
    aether_ai_serving::extract_ai_standard_requested_model(body_json)
}

pub(crate) fn extract_requested_model_from_request(
    parts: &http::request::Parts,
    body_json: &serde_json::Value,
    family: RequestedModelFamily,
) -> Option<String> {
    aether_ai_serving::extract_ai_requested_model_from_request_path(
        parts.uri.path(),
        body_json,
        family,
    )
}
