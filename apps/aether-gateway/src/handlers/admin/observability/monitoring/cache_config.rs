pub(crate) const ADMIN_MONITORING_CACHE_AFFINITY_REDIS_REQUIRED_DETAIL: &str =
    "Redis未初始化，无法获取缓存亲和性";
pub(crate) const ADMIN_MONITORING_REDIS_REQUIRED_DETAIL: &str = "Redis 未启用";
pub(crate) const ADMIN_MONITORING_CACHE_AFFINITY_DEFAULT_TTL_SECS: u64 = 300;
pub(crate) const ADMIN_MONITORING_CACHE_RESERVATION_RATIO: f64 = 0.1;
pub(crate) const ADMIN_MONITORING_DYNAMIC_RESERVATION_PROBE_PHASE_REQUESTS: u64 = 100;
pub(crate) const ADMIN_MONITORING_DYNAMIC_RESERVATION_PROBE_RESERVATION: f64 = 0.1;
pub(crate) const ADMIN_MONITORING_DYNAMIC_RESERVATION_STABLE_MIN_RESERVATION: f64 = 0.1;
pub(crate) const ADMIN_MONITORING_DYNAMIC_RESERVATION_STABLE_MAX_RESERVATION: f64 = 0.35;
pub(crate) const ADMIN_MONITORING_DYNAMIC_RESERVATION_LOW_LOAD_THRESHOLD: f64 = 0.5;
pub(crate) const ADMIN_MONITORING_DYNAMIC_RESERVATION_HIGH_LOAD_THRESHOLD: f64 = 0.8;
pub(crate) const ADMIN_MONITORING_REDIS_CACHE_CATEGORIES: &[(&str, &str, &str, &str)] = &[
    (
        "upstream_models",
        "上游模型",
        "upstream_models:*",
        "Provider 上游获取的模型列表缓存",
    ),
    ("model_id", "模型 ID", "model:id:*", "Model 按 ID 缓存"),
    (
        "model_provider_global",
        "模型映射",
        "model:provider_global:*",
        "Provider-GlobalModel 模型映射缓存",
    ),
    (
        "provider_mapping_preview",
        "映射预览",
        "admin:providers:mapping-preview:*",
        "Provider 详情页 mapping-preview 缓存",
    ),
    (
        "global_model",
        "全局模型",
        "global_model:*",
        "GlobalModel 缓存（ID/名称/解析）",
    ),
    (
        "models_list",
        "模型列表",
        "models:list:*",
        "/v1/models 端点模型列表缓存",
    ),
    ("user", "用户", "user:*", "用户信息缓存（ID/Email）"),
    (
        "apikey",
        "API Key",
        "apikey:*",
        "API Key 认证缓存（Hash/Auth）",
    ),
    (
        "api_key_id",
        "API Key ID",
        "api_key:id:*",
        "API Key 按 ID 缓存",
    ),
    (
        "cache_affinity",
        "缓存亲和性",
        "cache_affinity:*",
        "请求路由亲和性缓存",
    ),
    (
        "provider_rate",
        "Provider 费率",
        "provider_api_key:rate_multiplier:*",
        "ProviderAPIKey 费率倍数缓存",
    ),
    ("health", "健康检查", "health:*", "端点健康状态缓存"),
    (
        "endpoint_status",
        "端点状态",
        "endpoint_status:*",
        "用户端点状态缓存",
    ),
    ("dashboard", "仪表盘", "dashboard:*", "仪表盘统计缓存"),
    (
        "activity_heatmap",
        "活动热力图",
        "activity_heatmap:*",
        "用户活动热力图缓存",
    ),
    (
        "concurrency_lock",
        "并发锁",
        "concurrency:*",
        "请求并发控制锁",
    ),
];
