pub const OPENAI_IMAGE_STREAM_PLAN_KIND: &str = "openai_image_stream";
pub const OPENAI_IMAGE_SYNC_PLAN_KIND: &str = "openai_image_sync";
pub const OPENAI_CHAT_STREAM_PLAN_KIND: &str = "openai_chat_stream";
pub const CLAUDE_CHAT_STREAM_PLAN_KIND: &str = "claude_chat_stream";
pub const CLAUDE_COUNT_TOKENS_SYNC_PLAN_KIND: &str = "claude_count_tokens_sync";
pub const GEMINI_CHAT_STREAM_PLAN_KIND: &str = "gemini_chat_stream";
pub const GEMINI_INTERACTIONS_STREAM_PLAN_KIND: &str = "gemini_interactions_stream";
pub const OPENAI_RESPONSES_STREAM_PLAN_KIND: &str = "openai_responses_stream";
pub const OPENAI_REALTIME_STREAM_PLAN_KIND: &str = "openai_realtime_stream";
pub const OPENAI_RESPONSES_COMPACT_STREAM_PLAN_KIND: &str = "openai_responses_compact_stream";
pub const CLAUDE_CLI_STREAM_PLAN_KIND: &str = "claude_cli_stream";
pub const GEMINI_CLI_STREAM_PLAN_KIND: &str = "gemini_cli_stream";
pub const OPENAI_CHAT_SYNC_PLAN_KIND: &str = "openai_chat_sync";
pub const OPENAI_EMBEDDING_SYNC_PLAN_KIND: &str = "openai_embedding_sync";
pub const OPENAI_RERANK_SYNC_PLAN_KIND: &str = "openai_rerank_sync";
pub const OPENAI_SEARCH_SYNC_PLAN_KIND: &str = "openai_search_sync";
pub const GEMINI_EMBEDDING_SYNC_PLAN_KIND: &str = "gemini_embedding_sync";
pub const OPENAI_RESPONSES_SYNC_PLAN_KIND: &str = "openai_responses_sync";
pub const OPENAI_RESPONSES_COMPACT_SYNC_PLAN_KIND: &str = "openai_responses_compact_sync";
pub const CLAUDE_CHAT_SYNC_PLAN_KIND: &str = "claude_chat_sync";
pub const GEMINI_CHAT_SYNC_PLAN_KIND: &str = "gemini_chat_sync";
pub const GEMINI_INTERACTIONS_SYNC_PLAN_KIND: &str = "gemini_interactions_sync";
pub const CLAUDE_CLI_SYNC_PLAN_KIND: &str = "claude_cli_sync";
pub const GEMINI_CLI_SYNC_PLAN_KIND: &str = "gemini_cli_sync";

pub fn is_openai_responses_stream_plan_kind(plan_kind: &str) -> bool {
    matches!(
        plan_kind,
        OPENAI_RESPONSES_STREAM_PLAN_KIND | OPENAI_RESPONSES_COMPACT_STREAM_PLAN_KIND
    )
}

pub fn is_openai_responses_sync_plan_kind(plan_kind: &str) -> bool {
    matches!(
        plan_kind,
        OPENAI_RESPONSES_SYNC_PLAN_KIND | OPENAI_RESPONSES_COMPACT_SYNC_PLAN_KIND
    )
}
