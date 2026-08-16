use std::collections::{BTreeMap, BTreeSet};

use aether_data_contracts::repository::provider_catalog::{
    StoredProviderCatalogEndpoint, StoredProviderCatalogKey,
};
use aether_provider_transport::url::build_openai_compatible_models_url;
use regex::Regex;
use serde_json::Value;

const MODEL_FETCH_FORMAT_PRIORITY: &[&[&str]] = &[
    &[
        "openai:chat",
        "openai:responses",
        "openai:responses:compact",
    ],
    &["claude:messages"],
    &["gemini:generate_content"],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFetchRunSummary {
    pub attempted: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelsFetchSuccess {
    pub fetched_model_ids: Vec<String>,
    pub cached_models: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelsFetchPage {
    pub fetched_model_ids: Vec<String>,
    pub cached_models: Vec<Value>,
    pub has_more: bool,
    pub next_after_id: Option<String>,
}

pub fn extract_error_message(value: &Value) -> Option<String> {
    value
        .get("error")
        .and_then(Value::as_object)
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
}

pub fn build_models_fetch_url(
    endpoint_api_format: &str,
    base_url: &str,
) -> Option<(String, String)> {
    let api_format = normalize_api_format(endpoint_api_format);
    if !endpoint_supports_rust_models_fetch(&api_format) {
        return None;
    }
    let url = if api_format.starts_with("openai:") {
        build_v1_models_url(base_url)
    } else if api_format.starts_with("claude:") {
        build_claude_models_url(base_url)
    } else if api_format.starts_with("gemini:") {
        build_gemini_models_url(base_url)
    } else {
        None
    }?;
    Some((url, api_format))
}

pub fn parse_models_response(
    endpoint_api_format: &str,
    body: &Value,
) -> Result<ModelsFetchSuccess, String> {
    let parsed = parse_models_response_page(endpoint_api_format, body)?;
    Ok(ModelsFetchSuccess {
        fetched_model_ids: parsed.fetched_model_ids,
        cached_models: parsed.cached_models,
    })
}

pub fn parse_models_response_page(
    endpoint_api_format: &str,
    body: &Value,
) -> Result<ModelsFetchPage, String> {
    let api_format = normalize_api_format(endpoint_api_format);
    let mut cached_models = Vec::new();
    let mut fetched_model_ids = Vec::new();
    let mut seen = BTreeSet::new();
    let mut has_more = false;
    let mut next_after_id = None;

    if api_format.starts_with("openai:") || api_format.starts_with("claude:") {
        let items = if let Some(items) = body.get("data").and_then(Value::as_array) {
            has_more = body
                .get("has_more")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if api_format.starts_with("claude:") && has_more {
                next_after_id = body
                    .get("last_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned);
            }
            items
        } else if let Some(items) = body.as_array() {
            items
        } else if let Some(items) = body.get("models").and_then(Value::as_array) {
            items
        } else {
            return Err("models response is missing data array".to_string());
        };
        for item in items {
            let Some(model_id) = model_id_from_openai_like_item(item) else {
                continue;
            };
            if !seen.insert(model_id.clone()) {
                continue;
            }
            fetched_model_ids.push(model_id.clone());
            cached_models.push(normalize_cached_model(item, &model_id, &api_format));
        }
    } else if api_format.starts_with("gemini:") {
        let items = body
            .get("models")
            .and_then(Value::as_array)
            .ok_or_else(|| "gemini models response is missing models array".to_string())?;
        for item in items {
            let Some(name) = item
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let model_id = name.strip_prefix("models/").unwrap_or(name).trim();
            if model_id.is_empty() || !seen.insert(model_id.to_string()) {
                continue;
            }
            fetched_model_ids.push(model_id.to_string());
            cached_models.push(normalize_cached_model(item, model_id, &api_format));
        }
    } else {
        return Err("models response parser does not support this provider format".to_string());
    }

    Ok(ModelsFetchPage {
        fetched_model_ids,
        cached_models,
        has_more,
        next_after_id,
    })
}

pub fn selected_models_fetch_endpoints(
    endpoints: &[StoredProviderCatalogEndpoint],
    key: &StoredProviderCatalogKey,
) -> Vec<StoredProviderCatalogEndpoint> {
    let key_formats = json_string_list(key.api_formats.as_ref())
        .into_iter()
        .map(|value| normalize_api_format(&value))
        .collect::<BTreeSet<_>>();
    let mut by_format = BTreeMap::<String, StoredProviderCatalogEndpoint>::new();

    for endpoint in endpoints.iter().filter(|endpoint| endpoint.is_active) {
        let api_format = normalize_api_format(&endpoint.api_format);
        if api_format.is_empty() || !endpoint_supports_rust_models_fetch(&api_format) {
            continue;
        }
        if !key_formats.is_empty() && !key_formats.contains(&api_format) {
            continue;
        }
        if let Some(existing) = by_format.get_mut(&api_format) {
            if endpoint.api_format.trim().eq_ignore_ascii_case(&api_format)
                && !existing.api_format.trim().eq_ignore_ascii_case(&api_format)
            {
                *existing = endpoint.clone();
            }
        } else {
            by_format.insert(api_format, endpoint.clone());
        }
    }

    MODEL_FETCH_FORMAT_PRIORITY
        .iter()
        .filter_map(|candidates| {
            candidates
                .iter()
                .find_map(|api_format| by_format.remove(*api_format))
        })
        .collect()
}

pub fn select_models_fetch_endpoint(
    endpoints: &[StoredProviderCatalogEndpoint],
    key: &StoredProviderCatalogKey,
) -> Option<StoredProviderCatalogEndpoint> {
    selected_models_fetch_endpoints(endpoints, key)
        .into_iter()
        .next()
}

pub fn endpoint_supports_rust_models_fetch(api_format: &str) -> bool {
    let api_format = normalize_api_format(api_format);
    matches!(
        api_format.as_str(),
        "openai:chat"
            | "openai:responses"
            | "openai:responses:compact"
            | "claude:messages"
            | "gemini:generate_content"
    )
}

pub fn apply_model_filters(
    fetched_model_ids: &[String],
    locked_models: Vec<String>,
    include_patterns: Vec<String>,
    exclude_patterns: Vec<String>,
) -> Vec<String> {
    let mut filtered = BTreeSet::new();
    for model_id in fetched_model_ids {
        if model_id.trim().is_empty() {
            continue;
        }
        let included = if include_patterns.is_empty() {
            true
        } else {
            include_patterns
                .iter()
                .any(|pattern| wildcard_matches(pattern, model_id))
        };
        if !included {
            continue;
        }
        let excluded = exclude_patterns
            .iter()
            .any(|pattern| wildcard_matches(pattern, model_id));
        if !excluded {
            filtered.insert(model_id.trim().to_string());
        }
    }
    for model in locked_models {
        let trimmed = model.trim();
        if !trimmed.is_empty() {
            filtered.insert(trimmed.to_string());
        }
    }
    filtered.into_iter().collect()
}

pub fn json_string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn api_format_priority(api_format: &str) -> Option<(usize, usize)> {
    MODEL_FETCH_FORMAT_PRIORITY
        .iter()
        .enumerate()
        .find_map(|(group_index, group)| {
            group
                .iter()
                .position(|candidate| candidate.eq_ignore_ascii_case(api_format))
                .map(|format_index| (group_index, format_index))
        })
}

fn sorted_api_formats(formats: BTreeSet<String>) -> Vec<String> {
    let mut formats = formats.into_iter().collect::<Vec<_>>();
    formats.sort_by(
        |left, right| match (api_format_priority(left), api_format_priority(right)) {
            (Some(left_priority), Some(right_priority)) => left_priority.cmp(&right_priority),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => left.cmp(right),
        },
    );
    formats
}

pub fn aggregate_models_for_cache(models: &[Value]) -> Vec<Value> {
    let mut aggregated = BTreeMap::<String, serde_json::Map<String, Value>>::new();

    for model in models {
        let Some(object) = model.as_object() else {
            continue;
        };
        let Some(model_id) = object
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };

        let entry = aggregated.entry(model_id.to_string()).or_insert_with(|| {
            let mut cloned = object.clone();
            cloned.remove("api_format");
            cloned
        });

        let api_formats = object
            .get("api_formats")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        let legacy_api_format = object
            .get("api_format")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let existing_formats = entry
            .get("api_formats")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        let mut merged_formats = existing_formats
            .union(&api_formats)
            .cloned()
            .collect::<BTreeSet<_>>();
        if let Some(api_format) = legacy_api_format {
            merged_formats.insert(api_format);
        }
        let merged_formats = sorted_api_formats(merged_formats)
            .into_iter()
            .map(Value::String)
            .collect::<Vec<_>>();
        entry.insert("api_formats".to_string(), Value::Array(merged_formats));

        for (key, value) in object {
            if key == "api_format" || entry.contains_key(key) {
                continue;
            }
            entry.insert(key.clone(), value.clone());
        }
    }

    aggregated.into_values().map(Value::Object).collect()
}

fn build_v1_models_url(base_url: &str) -> Option<String> {
    build_openai_compatible_models_url(base_url)
}

fn build_claude_models_url(base_url: &str) -> Option<String> {
    let (trimmed_base_url, base_query) = split_url_query(base_url);
    let trimmed_base_url = trimmed_base_url.trim_end_matches('/');
    if trimmed_base_url.is_empty() {
        return None;
    }

    let mut url = if trimmed_base_url.ends_with("/models") {
        trimmed_base_url.to_string()
    } else {
        format!("{trimmed_base_url}/models")
    };
    if let Some(query) = base_query.filter(|value| !value.trim().is_empty()) {
        url.push('?');
        url.push_str(query);
    }
    Some(url)
}

fn build_gemini_models_url(base_url: &str) -> Option<String> {
    let (trimmed_base_url, base_query) = split_url_query(base_url);
    let trimmed_base_url = trimmed_base_url.trim_end_matches('/');
    if trimmed_base_url.is_empty() {
        return None;
    }

    let mut url = if trimmed_base_url.ends_with("/v1beta") {
        format!("{trimmed_base_url}/models")
    } else if trimmed_base_url.contains("/v1beta/models") {
        trimmed_base_url.to_string()
    } else {
        format!("{trimmed_base_url}/v1beta/models")
    };
    if let Some(query) = base_query.filter(|value| !value.trim().is_empty()) {
        url.push('?');
        url.push_str(query);
    }
    Some(url)
}

fn model_id_from_openai_like_item(item: &Value) -> Option<String> {
    if let Some(value) = item
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(value.trim_start_matches("models/").to_string());
    }

    ["id", "model", "slug", "name"].iter().find_map(|field| {
        item.get(*field)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.trim_start_matches("models/").to_string())
    })
}

fn split_url_query(base_url: &str) -> (&str, Option<&str>) {
    let trimmed = base_url.trim();
    trimmed
        .split_once('?')
        .map(|(base, query)| (base, Some(query)))
        .unwrap_or((trimmed, None))
}

fn normalize_cached_model(item: &Value, model_id: &str, api_format: &str) -> Value {
    let mut object = item.as_object().cloned().unwrap_or_default();
    object.insert("id".to_string(), Value::String(model_id.to_string()));
    object.insert(
        "api_formats".to_string(),
        Value::Array(vec![Value::String(api_format.to_string())]),
    );
    if api_format.starts_with("gemini:") {
        object
            .entry("owned_by".to_string())
            .or_insert_with(|| Value::String("google".to_string()));
        if !object.contains_key("display_name") {
            let display_name = item
                .get("displayName")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(model_id);
            object.insert(
                "display_name".to_string(),
                Value::String(display_name.to_string()),
            );
        }
    }
    object.remove("api_format");
    Value::Object(object)
}

fn wildcard_matches(pattern: &str, model_id: &str) -> bool {
    let mut regex = String::from("^");
    for ch in pattern.chars() {
        match ch {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            other => regex.push_str(&regex::escape(&other.to_string())),
        }
    }
    regex.push('$');
    Regex::new(&regex)
        .ok()
        .is_some_and(|compiled| compiled.is_match(model_id))
}

fn normalize_api_format(value: &str) -> String {
    aether_ai_formats::normalize_api_format_alias(value)
}
