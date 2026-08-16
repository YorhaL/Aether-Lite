use base64::Engine as _;

pub const UPSTREAM_IS_STREAM_KEY: &str = "upstream_is_stream";

pub fn parse_direct_request_body(
    is_json_request: bool,
    body_bytes: &[u8],
) -> Option<(serde_json::Value, Option<String>)> {
    if is_json_request {
        if body_bytes.is_empty() {
            Some((serde_json::json!({}), None))
        } else {
            serde_json::from_slice::<serde_json::Value>(body_bytes)
                .ok()
                .map(|value| (value, None))
        }
    } else {
        Some((
            serde_json::json!({}),
            (!body_bytes.is_empty())
                .then(|| base64::engine::general_purpose::STANDARD.encode(body_bytes)),
        ))
    }
}
