extern crate self as aether_ai_formats;

pub mod api;
pub mod contracts;
pub mod formats;
pub mod provider_compat;

pub use contracts::{ApiOperation, ClientSurface};

pub use formats::id::{
    api_format_alias_matches, api_format_defaults_to_client_error_failover,
    api_format_defaults_to_non_stream, api_format_permission_covers,
    api_format_permission_storage_aliases, api_format_storage_aliases,
    api_format_uses_body_stream_field, intersect_api_format_allowed_lists,
    is_openai_responses_compact_format, is_openai_responses_family_format,
    is_openai_responses_format, normalize_api_format_alias, FormatFamily, FormatId, FormatProfile,
};
pub use formats::matrix::{
    is_embedding_api_format, is_gemini_interactions_api_format, is_rerank_api_format,
};
pub use formats::openai::prompt_cache::resolve_openai_prompt_cache_ttl_minutes;
pub use formats::openai::prompt_cache::{
    validate_openai_prompt_cache_request, OpenAiPromptCacheContractViolation,
    OpenAiPromptCacheViolationKind,
};
pub use formats::openai::responses::{
    openai_responses_request_operation, OPENAI_RESPONSES_OPERATION_COMPACT,
};
pub use formats::shared::model_directives::{
    apply_model_directive_mapping_patch, apply_model_directive_overrides_from_model,
    apply_model_directive_overrides_from_request, claude_model_uses_adaptive_effort,
    default_model_directive_mapping_patch, default_model_directive_suffixes,
    default_model_directives_config, extract_gemini_model_from_path,
    gemini_model_uses_thinking_level, model_directive_base_model,
    model_directive_builtin_suffix_supported_for_source_model,
    model_directive_suffix_has_builtin_mapping, normalize_model_directive_model,
    openai_model_supports_prompt_cache_options, parse_model_directive,
    parse_model_directive_with_suffixes, reasoning_effort_supported_for_model, ModelDirective,
    ModelDirectiveSuffixResolution, ModelOverride, ReasoningEffort, ServiceTier,
    CROSS_PROVIDER_MODEL_DIRECTIVE_SUFFIXES, MODEL_DIRECTIVE_API_FORMATS,
    OPENAI_MODEL_DIRECTIVE_SUFFIXES,
};
pub use formats::shared::request::{parse_direct_request_body, UPSTREAM_IS_STREAM_KEY};
