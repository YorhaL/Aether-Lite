use crate::backup::config::S3BackupConfig;
use crate::handlers::admin::request::AdminAppState;
use crate::handlers::shared::{module_available_from_env, system_config_bool};
use crate::important_notification::{
    important_notification_configured, IMPORTANT_NOTIFICATION_ENABLED_KEY,
    LEGACY_NOTIFICATION_EMAIL_ENABLED_KEY,
};
use crate::system_features::ENABLE_MODEL_DIRECTIVES_CONFIG_KEY;
use crate::GatewayError;
use aether_admin::system as admin_system_kernel;
use serde_json::json;

pub(crate) struct AdminModuleDefinition {
    pub(crate) name: &'static str,
    pub(crate) display_name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) category: &'static str,
    pub(crate) env_key: &'static str,
    pub(crate) default_available: bool,
    pub(crate) admin_route: Option<&'static str>,
    pub(crate) admin_menu_icon: Option<&'static str>,
    pub(crate) admin_menu_group: Option<&'static str>,
    pub(crate) admin_menu_order: i32,
}

pub(crate) const ADMIN_MODULE_DEFINITIONS: &[AdminModuleDefinition] = &[
    AdminModuleDefinition {
        name: "oauth",
        display_name: "OAuth 登录",
        description: "支持通过第三方 OAuth Provider 登录/绑定账号",
        category: "auth",
        env_key: "OAUTH_AVAILABLE",
        default_available: true,
        admin_route: Some("/admin/oauth"),
        admin_menu_icon: Some("Key"),
        admin_menu_group: None,
        admin_menu_order: 55,
    },
    AdminModuleDefinition {
        name: "ldap",
        display_name: "LDAP 认证",
        description: "支持通过 LDAP/Active Directory 进行用户认证",
        category: "auth",
        env_key: "LDAP_AVAILABLE",
        default_available: true,
        admin_route: Some("/admin/ldap"),
        admin_menu_icon: Some("Users"),
        admin_menu_group: Some("system"),
        admin_menu_order: 50,
    },
    AdminModuleDefinition {
        name: "management_tokens",
        display_name: "访问令牌",
        description: "管理 API 访问令牌，支持细粒度权限控制和 IP 限制",
        category: "security",
        env_key: "MANAGEMENT_TOKENS_AVAILABLE",
        default_available: true,
        admin_route: Some("/admin/management-tokens"),
        admin_menu_icon: None,
        admin_menu_group: None,
        admin_menu_order: 0,
    },
    AdminModuleDefinition {
        name: "chat_pii_redaction",
        display_name: "敏感信息保护",
        description: "发送给供应商前将聊天消息中的敏感信息替换为占位符，返回客户端前自动还原。",
        category: "security",
        env_key: "CHAT_PII_REDACTION_AVAILABLE",
        default_available: true,
        admin_route: Some("/admin/modules/chat-pii-redaction"),
        admin_menu_icon: Some("ShieldCheck"),
        admin_menu_group: Some("system"),
        admin_menu_order: 59,
    },
    AdminModuleDefinition {
        name: "important_notification",
        display_name: "通知服务",
        description: "统一管理通知项、模板和推送服务选择，供后台任务和用户通知使用",
        category: "integration",
        env_key: "IMPORTANT_NOTIFICATION_AVAILABLE",
        default_available: true,
        admin_route: Some("/admin/notification-service"),
        admin_menu_icon: Some("BellRing"),
        admin_menu_group: None,
        admin_menu_order: 58,
    },
    AdminModuleDefinition {
        name: "model_directives",
        display_name: "模型后缀参数",
        description: "允许通过模型名后缀覆盖推理参数或服务层级",
        category: "integration",
        env_key: "MODEL_DIRECTIVES_AVAILABLE",
        default_available: true,
        admin_route: Some("/admin/model-directives"),
        admin_menu_icon: Some("SlidersHorizontal"),
        admin_menu_group: None,
        admin_menu_order: 59,
    },
    AdminModuleDefinition {
        name: "s3_backup",
        display_name: "S3 备份",
        description: "将配置、用户或完整数据定期备份到 S3-compatible 对象存储",
        category: "integration",
        env_key: "S3_BACKUP_AVAILABLE",
        default_available: true,
        admin_route: Some("/admin/modules/s3-backup"),
        admin_menu_icon: Some("CloudUpload"),
        admin_menu_group: None,
        admin_menu_order: 60,
    },
];

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct AdminSetModuleEnabledRequest {
    pub(crate) enabled: bool,
}

pub(crate) struct AdminModuleRuntimeState {
    oauth_providers: Vec<aether_data::repository::auth_modules::StoredOAuthProviderModuleConfig>,
    ldap_config: Option<aether_data::repository::auth_modules::StoredLdapModuleConfig>,
    important_notification_configured: bool,
    s3_backup_configured: bool,
}

pub(crate) fn admin_module_by_name(name: &str) -> Option<&'static AdminModuleDefinition> {
    let name = if name == "notification_email" {
        "important_notification"
    } else {
        name
    };
    ADMIN_MODULE_DEFINITIONS
        .iter()
        .find(|module| module.name == name)
}

pub(crate) fn admin_module_name_from_status_path(request_path: &str) -> Option<String> {
    admin_system_kernel::admin_module_name_from_status_path(request_path)
}

pub(crate) fn admin_module_name_from_enabled_path(request_path: &str) -> Option<String> {
    admin_system_kernel::admin_module_name_from_enabled_path(request_path)
}

pub(crate) fn admin_module_enabled_config_key(module: &AdminModuleDefinition) -> String {
    if module.name == "model_directives" {
        ENABLE_MODEL_DIRECTIVES_CONFIG_KEY.to_string()
    } else if module.name == "important_notification" {
        IMPORTANT_NOTIFICATION_ENABLED_KEY.to_string()
    } else if module.name == "s3_backup" {
        crate::backup::S3_BACKUP_ENABLED_KEY.to_string()
    } else {
        format!("module.{}.enabled", module.name)
    }
}

fn admin_module_available(module: &AdminModuleDefinition) -> bool {
    if module.name == "important_notification" {
        let legacy_default =
            module_available_from_env("NOTIFICATION_EMAIL_AVAILABLE", module.default_available);
        return module_available_from_env(module.env_key, legacy_default);
    }
    module_available_from_env(module.env_key, module.default_available)
}

pub(crate) fn oauth_module_config_is_valid(
    providers: &[aether_data::repository::auth_modules::StoredOAuthProviderModuleConfig],
) -> bool {
    admin_system_kernel::oauth_module_config_is_valid(providers)
}

pub(crate) fn ldap_module_config_is_valid(
    config: Option<&aether_data::repository::auth_modules::StoredLdapModuleConfig>,
) -> bool {
    admin_system_kernel::ldap_module_config_is_valid(config)
}

pub(crate) async fn build_admin_module_runtime_state(
    state: &AdminAppState<'_>,
) -> Result<AdminModuleRuntimeState, GatewayError> {
    let oauth_providers = state.list_enabled_oauth_module_providers().await?;
    let ldap_config = state.get_ldap_module_config().await?;

    let notification_configured = important_notification_configured(state.app()).await?;
    let backup_configured = s3_backup_configured(state.app()).await;

    Ok(AdminModuleRuntimeState {
        oauth_providers,
        ldap_config,
        important_notification_configured: notification_configured,
        s3_backup_configured: backup_configured,
    })
}

async fn s3_backup_configured(app: &crate::AppState) -> bool {
    let Ok(mut values) = crate::backup::task::load_s3_backup_config_values(app).await else {
        return false;
    };
    values.insert(
        crate::backup::S3_BACKUP_ENABLED_KEY.to_string(),
        json!(true),
    );
    S3BackupConfig::from_json_map(&values).is_ok()
}

pub(crate) fn build_admin_module_validation_result(
    module: &AdminModuleDefinition,
    runtime: &AdminModuleRuntimeState,
) -> (bool, Option<String>) {
    admin_system_kernel::build_admin_module_validation_result(
        admin_system_kernel::AdminModuleValidationInput {
            module_name: module.name,
            oauth_providers: &runtime.oauth_providers,
            ldap_config: runtime.ldap_config.as_ref(),
            important_notification_configured: runtime.important_notification_configured,
            s3_backup_configured: runtime.s3_backup_configured,
        },
    )
}

pub(crate) fn build_admin_module_health(module: &AdminModuleDefinition) -> &'static str {
    admin_system_kernel::build_admin_module_health(module.name)
}

pub(crate) async fn build_admin_module_status_payload(
    state: &AdminAppState<'_>,
    module: &AdminModuleDefinition,
    runtime: &AdminModuleRuntimeState,
) -> Result<serde_json::Value, GatewayError> {
    let available = admin_module_available(module);
    let enabled = if available {
        let enabled_value = state
            .read_system_config_json_value(&admin_module_enabled_config_key(module))
            .await?;
        let enabled_value = if module.name == "important_notification" && enabled_value.is_none() {
            state
                .read_system_config_json_value(LEGACY_NOTIFICATION_EMAIL_ENABLED_KEY)
                .await?
        } else {
            enabled_value
        };
        system_config_bool(enabled_value.as_ref(), false)
    } else {
        false
    };
    let (config_validated, config_error) = if available {
        build_admin_module_validation_result(module, runtime)
    } else {
        (false, None)
    };
    let health = if available {
        build_admin_module_health(module)
    } else {
        "unknown"
    };
    Ok(admin_system_kernel::build_admin_module_status_payload(
        module.name,
        module.display_name,
        module.description,
        module.category,
        module.admin_route,
        module.admin_menu_icon,
        module.admin_menu_group,
        module.admin_menu_order,
        available,
        enabled,
        config_validated,
        config_error,
        health,
    ))
}

pub(crate) async fn build_admin_modules_status_payload(
    state: &AdminAppState<'_>,
) -> Result<serde_json::Value, GatewayError> {
    let runtime = build_admin_module_runtime_state(state).await?;
    let mut payload = serde_json::Map::new();
    for module in ADMIN_MODULE_DEFINITIONS {
        payload.insert(
            module.name.to_string(),
            build_admin_module_status_payload(state, module, &runtime).await?,
        );
    }
    Ok(serde_json::Value::Object(payload))
}
