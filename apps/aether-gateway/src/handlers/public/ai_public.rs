use crate::ai_serving::{build_core_error_body_for_client_format, LocalCoreSyncErrorKind};
use crate::async_task::CancelVideoTaskError;
use crate::control::GatewayControlDecision;
use crate::control::GatewayPublicRequestContext;
use crate::image_capabilities::openai_image_gateway_max_generation_count;
use crate::{AppState, GatewayError};
use aether_data_contracts::repository::video_tasks::{
    StoredVideoTask, VideoTaskQueryFilter, VideoTaskStatus,
};
use axum::body::{Body, Bytes};
use axum::http::{self, Response};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

const GEMINI_VIDEO_TASK_NOT_FOUND_DETAIL: &str = "Video task not found";
const AI_PUBLIC_METHOD_NOT_ALLOWED_DETAIL: &str = "Method not allowed";
const AI_PUBLIC_UNAUTHORIZED_DETAIL: &str = "Unauthorized";
const OPENAI_IMAGE_PROMPT_DETAIL: &str = "图片生成/编辑请求缺少 prompt";
const OPENAI_IMAGE_EDIT_INPUT_DETAIL: &str = "图片编辑请求至少需要 1 张输入图片";
const OPENAI_IMAGE_PARTIAL_IMAGES_DETAIL: &str =
    "partial_images 仅支持 0-3，且必须配合 stream=true";
const OPENAI_IMAGE_RESPONSE_FORMAT_DETAIL: &str = "response_format 仅支持 url 或 b64_json";
const OPENAI_IMAGE_OUTPUT_FORMAT_DETAIL: &str = "output_format 仅支持 png、jpeg 或 webp";
const OPENAI_IMAGE_QUALITY_DETAIL: &str = "quality 仅支持 auto、low、medium、high、standard 或 hd";
const OPENAI_IMAGE_BACKGROUND_DETAIL: &str = "background 仅支持 auto、opaque 或 transparent";
const OPENAI_IMAGE_MODERATION_DETAIL: &str = "moderation 仅支持 auto 或 low";
const OPENAI_IMAGE_INPUT_FIDELITY_DETAIL: &str = "input_fidelity 仅支持 low 或 high";
const OPENAI_IMAGE_OUTPUT_COMPRESSION_DETAIL: &str = "output_compression 必须是 0-100 的整数";
const OPENAI_IMAGE_INVALID_JSON_DETAIL: &str = "图片接口 JSON 请求体无效";
const OPENAI_IMAGE_INVALID_MULTIPART_DETAIL: &str = "图片接口 multipart/form-data 请求体无效";
const OPENAI_EMBEDDING_CONTENT_TYPE_DETAIL: &str =
    "Embedding request content-type must be application/json";
const OPENAI_EMBEDDING_INVALID_JSON_DETAIL: &str = "Embedding request JSON body is invalid";
const OPENAI_EMBEDDING_MODEL_REQUIRED_DETAIL: &str = "Embedding request model is required";
const OPENAI_EMBEDDING_INPUT_REQUIRED_DETAIL: &str = "Embedding request input is required";
const OPENAI_EMBEDDING_CHAT_PAYLOAD_DETAIL: &str =
    "Embedding request must use input, not chat messages";
const OPENAI_EMBEDDING_STREAM_UNSUPPORTED_DETAIL: &str =
    "Embedding requests do not support streaming";
const OPENAI_RERANK_CONTENT_TYPE_DETAIL: &str =
    "Rerank request content-type must be application/json";
const OPENAI_RERANK_INVALID_JSON_DETAIL: &str = "Rerank request JSON body is invalid";
const OPENAI_RERANK_MODEL_REQUIRED_DETAIL: &str = "Rerank request model is required";
const OPENAI_RERANK_QUERY_REQUIRED_DETAIL: &str = "Rerank request query is required";
const OPENAI_RERANK_DOCUMENTS_REQUIRED_DETAIL: &str = "Rerank request documents are required";
const OPENAI_RERANK_TOP_N_DETAIL: &str = "Rerank request top_n must be a positive integer";
const OPENAI_RERANK_CHAT_PAYLOAD_DETAIL: &str =
    "Rerank request must use query/documents, not chat messages";
const OPENAI_RERANK_STREAM_UNSUPPORTED_DETAIL: &str = "Rerank requests do not support streaming";
const CLAUDE_COUNT_TOKENS_BODY_REQUIRED_DETAIL: &str = "Request body is required";
const CLAUDE_COUNT_TOKENS_INVALID_JSON_DETAIL: &str = "Invalid JSON body";
const CLAUDE_COUNT_TOKENS_MODEL_REQUIRED_DETAIL: &str = "model: Field required";
const CLAUDE_COUNT_TOKENS_MESSAGES_REQUIRED_DETAIL: &str = "messages: Field required";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpenAiImageOperation {
    Generate,
    Edit,
}

impl OpenAiImageOperation {
    fn from_path(path: &str) -> Option<Self> {
        match path {
            "/v1/images/generations" => Some(Self::Generate),
            "/v1/images/edits" => Some(Self::Edit),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
struct OpenAiImageValidationInput {
    model: Option<String>,
    prompt: Option<String>,
    image_count: usize,
    n: Option<u64>,
    stream: bool,
    partial_images: Option<u64>,
    response_format: Option<String>,
    output_format: Option<String>,
    quality: Option<String>,
    background: Option<String>,
    moderation: Option<String>,
    input_fidelity: Option<String>,
    output_compression: Option<u64>,
}

pub(crate) fn ai_public_local_requires_buffered_body(
    request_context: &GatewayPublicRequestContext,
) -> bool {
    request_context
        .control_decision
        .as_ref()
        .is_some_and(|decision| {
            decision.route_class.as_deref() == Some("ai_public")
                && request_context.request_method == http::Method::POST
                && ((decision.route_family.as_deref() == Some("claude")
                    && decision.route_kind.as_deref() == Some("count_tokens"))
                    || (decision.route_family.as_deref() == Some("openai")
                        && decision.route_kind.as_deref() == Some("embedding")
                        && request_context.request_path == "/v1/embeddings")
                    || (decision.route_family.as_deref() == Some("openai")
                        && decision.route_kind.as_deref() == Some("rerank")
                        && request_context.request_path == "/v1/rerank"))
        })
}

pub(crate) async fn maybe_build_local_ai_public_response(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    request_body: Option<&Bytes>,
) -> Option<Response<Body>> {
    let decision = request_context.control_decision.as_ref()?;
    if decision.route_class.as_deref() != Some("ai_public") {
        return None;
    }

    if let Some(response) =
        maybe_build_local_openai_request_validation_response(request_context, request_body)
    {
        return Some(response);
    }

    if let Some(response) =
        maybe_build_local_claude_count_tokens_validation_response(request_context, request_body)
    {
        return Some(response);
    }

    maybe_build_local_gemini_video_operations_response(state, request_context, decision).await
}

fn maybe_build_local_openai_request_validation_response(
    request_context: &GatewayPublicRequestContext,
    request_body: Option<&Bytes>,
) -> Option<Response<Body>> {
    let decision = request_context.control_decision.as_ref()?;
    if decision.route_family.as_deref() != Some("openai")
        || request_context.request_method != http::Method::POST
    {
        return None;
    }

    if decision.route_kind.as_deref() == Some("chat")
        && request_context.request_path == "/v1/chat/completions"
    {
        return None;
    }

    if decision.route_kind.as_deref() == Some("embedding")
        && request_context.request_path == "/v1/embeddings"
    {
        let Some(request_body) = request_body else {
            return Some(build_ai_public_error_response(
                http::StatusCode::BAD_REQUEST,
                OPENAI_EMBEDDING_INVALID_JSON_DETAIL,
            ));
        };
        if let Err(detail) = validate_openai_embedding_request(
            request_context.request_content_type.as_deref(),
            request_body,
        ) {
            return Some(build_ai_public_error_response(
                http::StatusCode::BAD_REQUEST,
                detail,
            ));
        }
        return None;
    }

    if decision.route_kind.as_deref() == Some("rerank")
        && request_context.request_path == "/v1/rerank"
    {
        let Some(request_body) = request_body else {
            return Some(build_ai_public_error_response(
                http::StatusCode::BAD_REQUEST,
                OPENAI_RERANK_INVALID_JSON_DETAIL,
            ));
        };
        if let Err(detail) = validate_openai_rerank_request(
            request_context.request_content_type.as_deref(),
            request_body,
        ) {
            return Some(build_ai_public_error_response(
                http::StatusCode::BAD_REQUEST,
                detail,
            ));
        }
        return None;
    }

    let request_body = request_body?;

    if decision.route_kind.as_deref() != Some("image")
        || !matches!(
            request_context.request_path.as_str(),
            "/v1/images/generations" | "/v1/images/edits"
        )
    {
        return None;
    }

    let Some(operation) = OpenAiImageOperation::from_path(&request_context.request_path) else {
        return None;
    };
    let validation = match parse_openai_image_validation_input(
        operation,
        request_context.request_content_type.as_deref(),
        request_body,
    ) {
        Ok(validation) => validation,
        Err(detail) => {
            return Some(build_ai_public_error_response(
                http::StatusCode::BAD_REQUEST,
                detail,
            ));
        }
    };

    match operation {
        OpenAiImageOperation::Generate | OpenAiImageOperation::Edit
            if validation.prompt.is_none() =>
        {
            return Some(build_ai_public_error_response(
                http::StatusCode::BAD_REQUEST,
                OPENAI_IMAGE_PROMPT_DETAIL,
            ));
        }
        OpenAiImageOperation::Edit if validation.image_count == 0 => {
            return Some(build_ai_public_error_response(
                http::StatusCode::BAD_REQUEST,
                OPENAI_IMAGE_EDIT_INPUT_DETAIL,
            ));
        }
        _ => {}
    }

    if let Some(detail) = validate_openai_image_n(&validation) {
        return Some(build_ai_public_error_response(
            http::StatusCode::BAD_REQUEST,
            detail,
        ));
    }

    if validation.partial_images.is_some_and(|value| value > 3)
        || (validation.partial_images.is_some() && !validation.stream)
    {
        return Some(build_ai_public_error_response(
            http::StatusCode::BAD_REQUEST,
            OPENAI_IMAGE_PARTIAL_IMAGES_DETAIL,
        ));
    }

    if validation
        .response_format
        .as_deref()
        .is_some_and(|value| !matches!(value, "url" | "b64_json"))
    {
        return Some(build_ai_public_error_response(
            http::StatusCode::BAD_REQUEST,
            OPENAI_IMAGE_RESPONSE_FORMAT_DETAIL,
        ));
    }

    if validation
        .output_format
        .as_deref()
        .is_some_and(|value| !matches!(value, "png" | "jpeg" | "jpg" | "webp"))
    {
        return Some(build_ai_public_error_response(
            http::StatusCode::BAD_REQUEST,
            OPENAI_IMAGE_OUTPUT_FORMAT_DETAIL,
        ));
    }

    if validation.quality.as_deref().is_some_and(|value| {
        !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "auto" | "low" | "medium" | "high" | "standard" | "hd"
        )
    }) {
        return Some(build_ai_public_error_response(
            http::StatusCode::BAD_REQUEST,
            OPENAI_IMAGE_QUALITY_DETAIL,
        ));
    }

    if validation
        .background
        .as_deref()
        .is_some_and(|value| !matches!(value, "auto" | "opaque" | "transparent"))
    {
        return Some(build_ai_public_error_response(
            http::StatusCode::BAD_REQUEST,
            OPENAI_IMAGE_BACKGROUND_DETAIL,
        ));
    }

    if validation
        .moderation
        .as_deref()
        .is_some_and(|value| !matches!(value, "auto" | "low"))
    {
        return Some(build_ai_public_error_response(
            http::StatusCode::BAD_REQUEST,
            OPENAI_IMAGE_MODERATION_DETAIL,
        ));
    }

    if validation
        .input_fidelity
        .as_deref()
        .is_some_and(|value| !matches!(value, "low" | "high"))
    {
        return Some(build_ai_public_error_response(
            http::StatusCode::BAD_REQUEST,
            OPENAI_IMAGE_INPUT_FIDELITY_DETAIL,
        ));
    }

    if validation
        .output_compression
        .is_some_and(|value| value > 100)
    {
        return Some(build_ai_public_error_response(
            http::StatusCode::BAD_REQUEST,
            OPENAI_IMAGE_OUTPUT_COMPRESSION_DETAIL,
        ));
    }

    None
}

fn openai_image_n_detail(max_generation_count: u64) -> String {
    if max_generation_count >= openai_image_gateway_max_generation_count() {
        format!("当前图片反代仅支持 n=1..{max_generation_count}")
    } else {
        format!("当前图片模型仅支持 n=1..{max_generation_count}")
    }
}

fn validate_openai_image_n(validation: &OpenAiImageValidationInput) -> Option<String> {
    let max_generation_count = openai_image_gateway_max_generation_count();
    validation
        .n
        .is_some_and(|value| value == 0 || value > max_generation_count)
        .then(|| openai_image_n_detail(max_generation_count))
}

fn validate_openai_embedding_request(
    content_type: Option<&str>,
    request_body: &Bytes,
) -> Result<(), &'static str> {
    if !content_type
        .unwrap_or_default()
        .to_ascii_lowercase()
        .contains("application/json")
    {
        return Err(OPENAI_EMBEDDING_CONTENT_TYPE_DETAIL);
    }
    if request_body.is_empty() {
        return Err(OPENAI_EMBEDDING_INVALID_JSON_DETAIL);
    }
    let payload = serde_json::from_slice::<Value>(request_body)
        .map_err(|_| OPENAI_EMBEDDING_INVALID_JSON_DETAIL)?;
    let object = payload
        .as_object()
        .ok_or(OPENAI_EMBEDDING_INVALID_JSON_DETAIL)?;
    if object.contains_key("messages") {
        return Err(OPENAI_EMBEDDING_CHAT_PAYLOAD_DETAIL);
    }
    if object
        .get("stream")
        .and_then(value_as_bool)
        .unwrap_or(false)
    {
        return Err(OPENAI_EMBEDDING_STREAM_UNSUPPORTED_DETAIL);
    }
    if object
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return Err(OPENAI_EMBEDDING_MODEL_REQUIRED_DETAIL);
    }
    let Some(input) = object.get("input") else {
        return Err(OPENAI_EMBEDDING_INPUT_REQUIRED_DETAIL);
    };
    if !embedding_input_is_non_empty(input) {
        return Err(OPENAI_EMBEDDING_INPUT_REQUIRED_DETAIL);
    }
    Ok(())
}

fn validate_openai_rerank_request(
    content_type: Option<&str>,
    request_body: &Bytes,
) -> Result<(), &'static str> {
    if !content_type
        .unwrap_or_default()
        .to_ascii_lowercase()
        .contains("application/json")
    {
        return Err(OPENAI_RERANK_CONTENT_TYPE_DETAIL);
    }
    if request_body.is_empty() {
        return Err(OPENAI_RERANK_INVALID_JSON_DETAIL);
    }
    let payload = serde_json::from_slice::<Value>(request_body)
        .map_err(|_| OPENAI_RERANK_INVALID_JSON_DETAIL)?;
    let object = payload
        .as_object()
        .ok_or(OPENAI_RERANK_INVALID_JSON_DETAIL)?;
    if object.contains_key("messages") {
        return Err(OPENAI_RERANK_CHAT_PAYLOAD_DETAIL);
    }
    if object
        .get("stream")
        .and_then(value_as_bool)
        .unwrap_or(false)
    {
        return Err(OPENAI_RERANK_STREAM_UNSUPPORTED_DETAIL);
    }
    if object
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return Err(OPENAI_RERANK_MODEL_REQUIRED_DETAIL);
    }
    if object
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return Err(OPENAI_RERANK_QUERY_REQUIRED_DETAIL);
    }
    let Some(documents) = object.get("documents").and_then(Value::as_array) else {
        return Err(OPENAI_RERANK_DOCUMENTS_REQUIRED_DETAIL);
    };
    if documents.is_empty() || documents.iter().any(rerank_document_is_empty) {
        return Err(OPENAI_RERANK_DOCUMENTS_REQUIRED_DETAIL);
    }
    if object
        .get("top_n")
        .or_else(|| object.get("topN"))
        .is_some_and(|value| !positive_json_integer(value))
    {
        return Err(OPENAI_RERANK_TOP_N_DETAIL);
    }
    Ok(())
}

fn rerank_document_is_empty(value: &Value) -> bool {
    match value {
        Value::String(text) => text.trim().is_empty(),
        Value::Object(object) => object
            .get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.trim().is_empty()),
        Value::Null => true,
        _ => false,
    }
}

fn positive_json_integer(value: &Value) -> bool {
    value.as_u64().is_some_and(|number| number > 0)
        || value.as_i64().is_some_and(|number| number > 0)
        || value
            .as_str()
            .and_then(|text| text.trim().parse::<u64>().ok())
            .is_some_and(|number| number > 0)
}

fn embedding_input_is_non_empty(value: &Value) -> bool {
    match value {
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(items) if !items.is_empty() => embedding_array_input_is_non_empty(items),
        _ => false,
    }
}

fn embedding_array_input_is_non_empty(items: &[Value]) -> bool {
    items
        .iter()
        .all(|item| item.as_str().is_some_and(|text| !text.trim().is_empty()))
        || embedding_token_array_is_non_empty(items)
        || items.iter().all(|item| {
            item.as_array()
                .is_some_and(|items| embedding_token_array_is_non_empty(items))
        })
        || items.iter().all(embedding_multimodal_content_is_non_empty)
}

fn embedding_token_array_is_non_empty(items: &[Value]) -> bool {
    !items.is_empty() && items.iter().all(|item| item.as_u64().is_some())
}

fn embedding_multimodal_content_is_non_empty(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let valid_text = object
        .get("text")
        .map(|value| value.as_str().is_some_and(|text| !text.trim().is_empty()));
    let valid_image = object
        .get("image")
        .map(|value| value.as_str().is_some_and(|image| !image.trim().is_empty()));
    let valid_video = object
        .get("video")
        .map(|value| value.as_str().is_some_and(|video| !video.trim().is_empty()));
    let valid_multi_images = object.get("multi_images").map(|value| {
        value.as_array().is_some_and(|items| {
            !items.is_empty()
                && items
                    .iter()
                    .all(|item| item.as_str().is_some_and(|image| !image.trim().is_empty()))
        })
    });

    [valid_text, valid_image, valid_video, valid_multi_images]
        .into_iter()
        .flatten()
        .all(|valid| valid)
        && [valid_text, valid_image, valid_video, valid_multi_images]
            .into_iter()
            .flatten()
            .any(|valid| valid)
}

fn image_request_count(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|number| u64::try_from(number).ok()))
        .or_else(|| {
            value
                .as_str()
                .and_then(|text| text.trim().parse::<u64>().ok())
        })
}

fn parse_openai_image_validation_input(
    operation: OpenAiImageOperation,
    content_type: Option<&str>,
    request_body: &Bytes,
) -> Result<OpenAiImageValidationInput, &'static str> {
    if request_body.is_empty() {
        return Err(match operation {
            OpenAiImageOperation::Generate | OpenAiImageOperation::Edit => {
                OPENAI_IMAGE_PROMPT_DETAIL
            }
        });
    }

    let content_type = content_type.unwrap_or_default();
    if content_type
        .to_ascii_lowercase()
        .contains("multipart/form-data")
    {
        parse_openai_image_validation_input_from_multipart(request_body, content_type)
    } else {
        parse_openai_image_validation_input_from_json(request_body)
    }
}

fn parse_openai_image_validation_input_from_json(
    request_body: &Bytes,
) -> Result<OpenAiImageValidationInput, &'static str> {
    let payload = serde_json::from_slice::<Value>(request_body)
        .map_err(|_| OPENAI_IMAGE_INVALID_JSON_DETAIL)?;
    let object = payload
        .as_object()
        .ok_or(OPENAI_IMAGE_INVALID_JSON_DETAIL)?;

    Ok(OpenAiImageValidationInput {
        model: normalize_openai_image_model_for_operation(
            object.get("model").and_then(Value::as_str),
        ),
        prompt: object
            .get("prompt")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        image_count: count_json_images(object),
        n: object.get("n").and_then(image_request_count),
        stream: object
            .get("stream")
            .and_then(value_as_bool)
            .unwrap_or(false),
        partial_images: object.get("partial_images").and_then(image_request_count),
        response_format: object
            .get("response_format")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase()),
        output_format: object
            .get("output_format")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase()),
        quality: object
            .get("quality")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase()),
        background: object
            .get("background")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase()),
        moderation: object
            .get("moderation")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase()),
        input_fidelity: object
            .get("input_fidelity")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase()),
        output_compression: object
            .get("output_compression")
            .and_then(image_request_count),
    })
}

fn parse_openai_image_validation_input_from_multipart(
    request_body: &Bytes,
    content_type: &str,
) -> Result<OpenAiImageValidationInput, &'static str> {
    let boundary = multipart_boundary(content_type).ok_or(OPENAI_IMAGE_INVALID_MULTIPART_DETAIL)?;
    let fields = parse_multipart_fields(request_body, &boundary);
    if fields.is_empty() {
        return Err(OPENAI_IMAGE_INVALID_MULTIPART_DETAIL);
    }

    let model = fields
        .iter()
        .find(|field| field.name.trim() == "model")
        .map(|field| String::from_utf8_lossy(&field.data).trim().to_string());

    Ok(OpenAiImageValidationInput {
        model: normalize_openai_image_model_for_operation(model.as_deref()),
        prompt: multipart_text_field(&fields, "prompt"),
        image_count: fields
            .iter()
            .filter(|field| {
                matches!(
                    field.name.trim(),
                    "image" | "image[]" | "images" | "images[]"
                )
            })
            .count(),
        n: multipart_text_field(&fields, "n").and_then(|value| value.trim().parse::<u64>().ok()),
        stream: multipart_text_field(&fields, "stream")
            .and_then(|value| parse_bool_string(&value))
            .unwrap_or(false),
        partial_images: multipart_text_field(&fields, "partial_images")
            .and_then(|value| value.trim().parse::<u64>().ok()),
        response_format: multipart_text_field(&fields, "response_format")
            .map(|value| value.to_ascii_lowercase()),
        output_format: multipart_text_field(&fields, "output_format")
            .map(|value| value.to_ascii_lowercase()),
        quality: multipart_text_field(&fields, "quality").map(|value| value.to_ascii_lowercase()),
        background: multipart_text_field(&fields, "background")
            .map(|value| value.to_ascii_lowercase()),
        moderation: multipart_text_field(&fields, "moderation")
            .map(|value| value.to_ascii_lowercase()),
        input_fidelity: multipart_text_field(&fields, "input_fidelity")
            .map(|value| value.to_ascii_lowercase()),
        output_compression: multipart_text_field(&fields, "output_compression")
            .and_then(|value| value.trim().parse::<u64>().ok()),
    })
}

fn normalize_openai_image_model_for_operation(model: Option<&str>) -> Option<String> {
    model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn count_json_images(object: &serde_json::Map<String, Value>) -> usize {
    let mut count = 0usize;
    if let Some(value) = object.get("image") {
        count += json_image_count(value);
    }
    if let Some(values) = object.get("images").and_then(Value::as_array) {
        count += values.iter().map(json_image_count).sum::<usize>();
    }
    count
}

fn json_image_count(value: &Value) -> usize {
    match value {
        Value::Array(values) => values.iter().map(json_image_count).sum(),
        Value::String(text) => (!text.trim().is_empty()) as usize,
        Value::Object(_) => 1,
        _ => 0,
    }
}

fn value_as_bool(value: &Value) -> Option<bool> {
    value
        .as_bool()
        .or_else(|| value.as_str().and_then(parse_bool_string))
}

fn parse_bool_string(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

#[derive(Debug)]
struct MultipartField {
    name: String,
    data: Vec<u8>,
}

fn multipart_text_field(fields: &[MultipartField], name: &str) -> Option<String> {
    fields
        .iter()
        .find(|field| field.name.trim() == name)
        .map(|field| String::from_utf8_lossy(&field.data).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_multipart_fields(body: &[u8], boundary: &str) -> Vec<MultipartField> {
    let delimiter = format!("--{boundary}").into_bytes();
    let mut parts = Vec::new();
    let mut cursor = 0usize;

    while let Some(index) = find_subslice(&body[cursor..], &delimiter) {
        let start = cursor + index + delimiter.len();
        if body.get(start..start + 2) == Some(b"--") {
            break;
        }
        let mut part = &body[start..];
        if part.starts_with(b"\r\n") {
            part = &part[2..];
        }
        let Some(next) = find_subslice(part, &delimiter) else {
            break;
        };
        let raw = &part[..next];
        let raw = raw.strip_suffix(b"\r\n").unwrap_or(raw);
        if let Some(field) = parse_multipart_field(raw) {
            parts.push(field);
        }
        cursor = start + next;
    }

    parts
}

fn multipart_boundary(content_type: &str) -> Option<String> {
    content_type.split(';').find_map(|segment| {
        let (key, value) = segment.trim().split_once('=')?;
        if !key.trim().eq_ignore_ascii_case("boundary") {
            return None;
        }
        let boundary = value.trim().trim_matches('"').trim();
        (!boundary.is_empty()).then(|| boundary.to_string())
    })
}

fn parse_multipart_field(raw: &[u8]) -> Option<MultipartField> {
    let header_end = find_subslice(raw, b"\r\n\r\n")?;
    let headers = &raw[..header_end];
    let data = raw.get(header_end + 4..)?.to_vec();
    let header_text = String::from_utf8_lossy(headers);

    let mut name = None;
    for line in header_text.lines() {
        let trimmed = line.trim();
        if trimmed
            .to_ascii_lowercase()
            .starts_with("content-disposition:")
        {
            name = extract_quoted_header_value(trimmed, "name");
        }
    }

    Some(MultipartField { name: name?, data })
}

fn extract_quoted_header_value(header: &str, key: &str) -> Option<String> {
    let pattern = format!("{key}=\"");
    let start = header.find(&pattern)? + pattern.len();
    let rest = &header[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn maybe_build_local_claude_count_tokens_validation_response(
    request_context: &GatewayPublicRequestContext,
    request_body: Option<&Bytes>,
) -> Option<Response<Body>> {
    let decision = request_context.control_decision.as_ref()?;
    if decision.route_family.as_deref() != Some("claude")
        || decision.route_kind.as_deref() != Some("count_tokens")
        || request_context.request_method != http::Method::POST
        || request_context.request_path != "/v1/messages/count_tokens"
    {
        return None;
    }

    let validation = validate_claude_count_tokens_request(request_body);
    validation.err().map(build_claude_invalid_request_response)
}

fn validate_claude_count_tokens_request(request_body: Option<&Bytes>) -> Result<(), &'static str> {
    let request_body = request_body
        .filter(|body| !body.is_empty())
        .ok_or(CLAUDE_COUNT_TOKENS_BODY_REQUIRED_DETAIL)?;
    let payload = serde_json::from_slice::<Value>(request_body)
        .map_err(|_| CLAUDE_COUNT_TOKENS_INVALID_JSON_DETAIL)?;
    let object = payload
        .as_object()
        .ok_or(CLAUDE_COUNT_TOKENS_INVALID_JSON_DETAIL)?;

    if object
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .is_none()
    {
        return Err(CLAUDE_COUNT_TOKENS_MODEL_REQUIRED_DETAIL);
    }
    if object.get("messages").and_then(Value::as_array).is_none() {
        return Err(CLAUDE_COUNT_TOKENS_MESSAGES_REQUIRED_DETAIL);
    }

    Ok(())
}

fn build_claude_invalid_request_response(detail: &'static str) -> Response<Body> {
    let body = build_core_error_body_for_client_format(
        "claude:messages",
        detail,
        None,
        LocalCoreSyncErrorKind::InvalidRequest,
    )
    .expect("Claude core error format should be available");
    (http::StatusCode::BAD_REQUEST, Json(body)).into_response()
}

async fn maybe_build_local_gemini_video_operations_response(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    decision: &GatewayControlDecision,
) -> Option<Response<Body>> {
    if decision.route_family.as_deref() != Some("gemini")
        || decision.route_kind.as_deref() != Some("video")
    {
        return None;
    }

    if request_context.request_path == "/v1beta/operations" {
        return Some(match request_context.request_method {
            http::Method::GET => {
                build_local_gemini_video_operations_list_response(state, decision).await
            }
            _ => build_ai_public_error_response(
                http::StatusCode::METHOD_NOT_ALLOWED,
                AI_PUBLIC_METHOD_NOT_ALLOWED_DETAIL,
            ),
        });
    }

    let Some(operation_path) = request_context
        .request_path
        .strip_prefix("/v1beta/operations/")
    else {
        return None;
    };

    Some(match request_context.request_method {
        http::Method::GET => {
            build_local_gemini_video_operation_detail_response(state, decision, operation_path)
                .await
        }
        http::Method::POST if operation_path.ends_with(":cancel") => {
            build_local_gemini_video_operation_cancel_response(state, decision, operation_path)
                .await
        }
        _ => build_ai_public_error_response(
            http::StatusCode::METHOD_NOT_ALLOWED,
            AI_PUBLIC_METHOD_NOT_ALLOWED_DETAIL,
        ),
    })
}

async fn build_local_gemini_video_operations_list_response(
    state: &AppState,
    decision: &GatewayControlDecision,
) -> Response<Body> {
    let Some(user_id) = decision
        .auth_context
        .as_ref()
        .map(|auth_context| auth_context.user_id.trim())
        .filter(|value| !value.is_empty())
    else {
        return build_ai_public_error_response(
            http::StatusCode::UNAUTHORIZED,
            AI_PUBLIC_UNAUTHORIZED_DETAIL,
        );
    };

    let filter = VideoTaskQueryFilter {
        user_id: Some(user_id.to_string()),
        status: None,
        model_substring: None,
        client_api_format: Some("gemini:video".to_string()),
    };
    let tasks = match state.list_video_task_page(&filter, 0, 100).await {
        Ok(tasks) => tasks,
        Err(err) => {
            return build_ai_public_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("{err:?}"),
            );
        }
    };
    let operations = tasks
        .into_iter()
        .filter(is_gemini_video_task)
        .map(|task| build_gemini_video_operation_payload(&task))
        .collect::<Vec<_>>();

    Json(json!({ "operations": operations })).into_response()
}

async fn build_local_gemini_video_operation_detail_response(
    state: &AppState,
    decision: &GatewayControlDecision,
    operation_path: &str,
) -> Response<Body> {
    let task =
        match find_user_gemini_video_task_for_operation(state, decision, operation_path).await {
            Ok(Some(task)) => task,
            Ok(None) => {
                return build_ai_public_error_response(
                    http::StatusCode::NOT_FOUND,
                    GEMINI_VIDEO_TASK_NOT_FOUND_DETAIL,
                );
            }
            Err(err) => {
                return build_ai_public_error_response(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("{err:?}"),
                );
            }
        };

    Json(build_gemini_video_operation_payload(&task)).into_response()
}

async fn build_local_gemini_video_operation_cancel_response(
    state: &AppState,
    decision: &GatewayControlDecision,
    operation_path: &str,
) -> Response<Body> {
    let task =
        match find_user_gemini_video_task_for_operation(state, decision, operation_path).await {
            Ok(Some(task)) => task,
            Ok(None) => {
                return build_ai_public_error_response(
                    http::StatusCode::NOT_FOUND,
                    GEMINI_VIDEO_TASK_NOT_FOUND_DETAIL,
                );
            }
            Err(err) => {
                return build_ai_public_error_response(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("{err:?}"),
                );
            }
        };

    match crate::async_task::cancel_video_task_record(state, &task.id).await {
        Ok(_) => Json(json!({})).into_response(),
        Err(CancelVideoTaskError::NotFound) => build_ai_public_error_response(
            http::StatusCode::NOT_FOUND,
            GEMINI_VIDEO_TASK_NOT_FOUND_DETAIL,
        ),
        Err(CancelVideoTaskError::InvalidStatus(status)) => build_ai_public_error_response(
            http::StatusCode::BAD_REQUEST,
            format!(
                "Cannot cancel task with status: {}",
                video_task_status_name(status)
            ),
        ),
        Err(CancelVideoTaskError::Response(response)) => response,
        Err(CancelVideoTaskError::Gateway(err)) => build_ai_public_error_response(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{err:?}"),
        ),
    }
}

async fn find_user_gemini_video_task_for_operation(
    state: &AppState,
    decision: &GatewayControlDecision,
    operation_path: &str,
) -> Result<Option<StoredVideoTask>, GatewayError> {
    let Some(user_id) = decision
        .auth_context
        .as_ref()
        .map(|auth_context| auth_context.user_id.trim())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let Some(short_id) = extract_short_id_from_gemini_operation_path(operation_path) else {
        return Ok(None);
    };
    let Some(task) = state.find_video_task_by_short_id(short_id).await? else {
        return Ok(None);
    };
    if task.user_id.as_deref().map(str::trim) != Some(user_id) || !is_gemini_video_task(&task) {
        return Ok(None);
    }
    Ok(Some(task))
}

fn extract_short_id_from_gemini_operation_path(operation_path: &str) -> Option<&str> {
    let trimmed = operation_path.trim_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let short_id = trimmed
        .strip_suffix(":cancel")
        .unwrap_or(trimmed)
        .rsplit('/')
        .next()?;
    (!short_id.is_empty()).then_some(short_id)
}

fn is_gemini_video_task(task: &StoredVideoTask) -> bool {
    matches!(
        task.provider_api_format
            .as_deref()
            .or(task.client_api_format.as_deref())
            .map(str::trim),
        Some("gemini:video")
    )
}

fn build_gemini_video_operation_payload(task: &StoredVideoTask) -> serde_json::Value {
    match task.status {
        VideoTaskStatus::Completed => json!({
            "name": gemini_video_operation_name(task),
            "done": true,
            "response": {
                "generateVideoResponse": {
                    "generatedSamples": [
                        {
                            "video": {
                                "uri": format!(
                                    "/v1beta/files/aev_{}:download?alt=media",
                                    gemini_operation_short_id(task)
                                ),
                                "mimeType": "video/mp4",
                            }
                        }
                    ]
                }
            }
        }),
        VideoTaskStatus::Failed | VideoTaskStatus::Expired => json!({
            "name": gemini_video_operation_name(task),
            "done": true,
            "error": {
                "code": task.error_code.clone().unwrap_or_else(|| "UNKNOWN".to_string()),
                "message": task
                    .error_message
                    .clone()
                    .unwrap_or_else(|| "Video generation failed".to_string()),
            }
        }),
        _ => json!({
            "name": gemini_video_operation_name(task),
            "done": false,
            "metadata": gemini_video_operation_metadata(task),
        }),
    }
}

fn gemini_video_operation_name(task: &StoredVideoTask) -> String {
    format!(
        "models/{}/operations/{}",
        gemini_operation_model(task),
        gemini_operation_short_id(task)
    )
}

fn gemini_operation_model(task: &StoredVideoTask) -> String {
    task.model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            task.external_task_id.as_deref().and_then(|external_id| {
                let parts = external_id.split('/').collect::<Vec<_>>();
                if parts.len() >= 2 && parts[0] == "models" && !parts[1].trim().is_empty() {
                    Some(parts[1].trim().to_string())
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn gemini_operation_short_id(task: &StoredVideoTask) -> String {
    task.short_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(task.id.as_str())
        .to_string()
}

fn gemini_video_operation_metadata(task: &StoredVideoTask) -> serde_json::Value {
    task.request_metadata
        .as_ref()
        .and_then(|metadata| metadata.get("rust_local_snapshot"))
        .and_then(|snapshot| snapshot.get("Gemini"))
        .and_then(|gemini| gemini.get("metadata"))
        .cloned()
        .unwrap_or_else(|| json!({}))
}

fn video_task_status_name(status: VideoTaskStatus) -> &'static str {
    match status {
        VideoTaskStatus::Pending => "pending",
        VideoTaskStatus::Submitted => "submitted",
        VideoTaskStatus::Queued => "queued",
        VideoTaskStatus::Processing => "processing",
        VideoTaskStatus::Completed => "completed",
        VideoTaskStatus::Failed => "failed",
        VideoTaskStatus::Cancelled => "cancelled",
        VideoTaskStatus::Expired => "expired",
        VideoTaskStatus::Deleted => "deleted",
    }
}

fn build_ai_public_error_response(
    status: http::StatusCode,
    detail: impl Into<String>,
) -> Response<Body> {
    (status, Json(json!({ "detail": detail.into() }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::{
        parse_openai_image_validation_input, validate_claude_count_tokens_request,
        validate_openai_image_n, OpenAiImageOperation, CLAUDE_COUNT_TOKENS_BODY_REQUIRED_DETAIL,
        CLAUDE_COUNT_TOKENS_INVALID_JSON_DETAIL, CLAUDE_COUNT_TOKENS_MESSAGES_REQUIRED_DETAIL,
        CLAUDE_COUNT_TOKENS_MODEL_REQUIRED_DETAIL,
    };
    use axum::body::Bytes;
    use serde_json::json;

    #[test]
    fn count_tokens_validation_rejects_only_structurally_invalid_requests() {
        assert_eq!(
            validate_claude_count_tokens_request(None),
            Err(CLAUDE_COUNT_TOKENS_BODY_REQUIRED_DETAIL)
        );
        assert_eq!(
            validate_claude_count_tokens_request(Some(&Bytes::from_static(b"{"))),
            Err(CLAUDE_COUNT_TOKENS_INVALID_JSON_DETAIL)
        );
        assert_eq!(
            validate_claude_count_tokens_request(Some(&Bytes::from_static(br#"{"messages":[]}"#,))),
            Err(CLAUDE_COUNT_TOKENS_MODEL_REQUIRED_DETAIL)
        );
        assert_eq!(
            validate_claude_count_tokens_request(Some(&Bytes::from_static(
                br#"{"model":"claude-sonnet-4-5"}"#,
            ))),
            Err(CLAUDE_COUNT_TOKENS_MESSAGES_REQUIRED_DETAIL)
        );
        assert_eq!(
            validate_claude_count_tokens_request(Some(&Bytes::from_static(
                br#"{"model":"claude-sonnet-4-5","messages":[],"tools":[{"name":"x"}]}"#,
            ))),
            Ok(())
        );
    }

    #[test]
    fn image_validation_accepts_custom_model_name() {
        let body =
            Bytes::from_static(br#"{"model":" Custom/Image-Model:V1 ","prompt":"draw an image"}"#);

        let validation = parse_openai_image_validation_input(
            OpenAiImageOperation::Generate,
            Some("application/json"),
            &body,
        )
        .expect("custom image model should validate");

        assert_eq!(validation.model.as_deref(), Some("Custom/Image-Model:V1"));
    }

    #[test]
    fn image_validation_accepts_multipart_with_mixed_case_boundary() {
        let boundary = "------------------------OYNWsMZCt0ILTwn8naP4Gb";
        let body = Bytes::from(format!(
            concat!(
                "--{boundary}\r\n",
                "Content-Disposition: form-data; name=\"model\"\r\n\r\n",
                "gpt-image-2\r\n",
                "--{boundary}\r\n",
                "Content-Disposition: form-data; name=\"prompt\"\r\n\r\n",
                "edit this image\r\n",
                "--{boundary}\r\n",
                "Content-Disposition: form-data; name=\"image\"; filename=\"image.jpg\"\r\n",
                "Content-Type: image/jpeg\r\n\r\n",
                "image-bytes\r\n",
                "--{boundary}--\r\n"
            ),
            boundary = boundary,
        ));

        let validation = parse_openai_image_validation_input(
            OpenAiImageOperation::Edit,
            Some(&format!("multipart/form-data; boundary={boundary}")),
            &body,
        )
        .expect("multipart image edit should validate");

        assert_eq!(validation.model.as_deref(), Some("gpt-image-2"));
        assert_eq!(validation.prompt.as_deref(), Some("edit this image"));
        assert_eq!(validation.image_count, 1);
    }

    #[test]
    fn image_validation_applies_the_global_count_limit_before_model_mapping() {
        let openai_body = Bytes::from_static(br#"{"model":"gpt-image-2","prompt":"draw","n":2}"#);
        let openai_validation = parse_openai_image_validation_input(
            OpenAiImageOperation::Generate,
            Some("application/json"),
            &openai_body,
        )
        .expect("valid image payload should parse");

        assert!(validate_openai_image_n(&openai_validation).is_none());

        let multi_image_body =
            Bytes::from_static(br#"{"model":"production-image-alias","prompt":"draw","n":4}"#);
        let multi_image_validation = parse_openai_image_validation_input(
            OpenAiImageOperation::Generate,
            Some("application/json"),
            &multi_image_body,
        )
        .expect("valid image payload should parse");

        assert!(validate_openai_image_n(&multi_image_validation).is_none());

        let alias_body =
            Bytes::from_static(br#"{"model":"production-image-alias","prompt":"draw","n":10}"#);
        let alias_validation = parse_openai_image_validation_input(
            OpenAiImageOperation::Generate,
            Some("application/json"),
            &alias_body,
        )
        .expect("valid image alias payload should parse");
        assert!(validate_openai_image_n(&alias_validation).is_none());

        let excessive_body =
            Bytes::from_static(br#"{"model":"production-image-alias","prompt":"draw","n":11}"#);
        let excessive_validation = parse_openai_image_validation_input(
            OpenAiImageOperation::Generate,
            Some("application/json"),
            &excessive_body,
        )
        .expect("image payload should parse before count validation");
        assert_eq!(
            validate_openai_image_n(&excessive_validation).as_deref(),
            Some("当前图片反代仅支持 n=1..10")
        );
    }
}
