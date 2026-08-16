use super::{
    auth_email_is_verified, auth_now, auth_registration_email_configured,
    auth_verification_code_expire_minutes, auth_verification_send_cooldown_seconds,
    build_auth_error_response, build_auth_json_response, build_auth_verification_email,
    clear_auth_email_pending_code, clear_auth_email_verification, generate_auth_verification_code,
    http, json, mark_auth_email_verified, read_auth_email_verification_code, read_auth_smtp_config,
    send_auth_email, store_auth_email_verification_code, system_config_bool, system_config_f64,
    system_config_string, system_config_string_list, verify_auth_turnstile, AppState,
    AuthTurnstileAction, Body, GatewayError, Regex, Response,
};
use serde::Deserialize;

const AUTH_REGISTRATION_STORAGE_UNAVAILABLE_DETAIL: &str = "注册数据存储暂不可用";

#[derive(Debug, Deserialize)]
struct AuthRegisterRequest {
    email: Option<String>,
    username: String,
    password: String,
    turnstile_token: Option<String>,
    privacy_policy_accepted: Option<bool>,
    privacy_policy_version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthEmailRequest {
    email: String,
    turnstile_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthVerifyEmailRequest {
    email: String,
    code: String,
}

fn normalize_auth_email(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return None;
    }
    let pattern = Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$")
        .expect("email regex should compile");
    pattern.is_match(&value).then_some(value)
}

fn normalize_auth_optional_email(value: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    normalize_auth_email(value)
        .map(Some)
        .ok_or_else(|| "邮箱格式无效".to_string())
}

fn validate_auth_register_username(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("用户名不能为空".to_string());
    }
    if value.len() < 3 {
        return Err("用户名长度至少为3个字符".to_string());
    }
    if value.len() > 30 {
        return Err("用户名长度不能超过30个字符".to_string());
    }
    let pattern = Regex::new(r"^[a-zA-Z0-9_.\\-]+$").expect("username regex should compile");
    if !pattern.is_match(value) {
        return Err("用户名只能包含字母、数字、下划线、连字符和点号".to_string());
    }
    if matches!(
        value.to_ascii_lowercase().as_str(),
        "admin"
            | "root"
            | "system"
            | "api"
            | "test"
            | "demo"
            | "user"
            | "guest"
            | "bot"
            | "webhook"
            | "support"
    ) {
        return Err("该用户名为系统保留用户名".to_string());
    }
    Ok(value.to_string())
}

pub(crate) fn validate_auth_register_password(password: &str, policy: &str) -> Result<(), String> {
    if password.is_empty() {
        return Err("密码不能为空".to_string());
    }
    if password.as_bytes().len() > 72 {
        return Err("密码长度不能超过72字节".to_string());
    }
    let min_len = if matches!(policy, "medium" | "strong") {
        8
    } else {
        6
    };
    if password.chars().count() < min_len {
        return Err(format!("密码长度至少为{min_len}个字符"));
    }
    if policy == "medium" {
        if !password.chars().any(|ch| ch.is_ascii_alphabetic()) {
            return Err("密码必须包含至少一个字母".to_string());
        }
        if !password.chars().any(|ch| ch.is_ascii_digit()) {
            return Err("密码必须包含至少一个数字".to_string());
        }
    } else if policy == "strong" {
        if !password.chars().any(|ch| ch.is_ascii_uppercase()) {
            return Err("密码必须包含至少一个大写字母".to_string());
        }
        if !password.chars().any(|ch| ch.is_ascii_lowercase()) {
            return Err("密码必须包含至少一个小写字母".to_string());
        }
        if !password.chars().any(|ch| ch.is_ascii_digit()) {
            return Err("密码必须包含至少一个数字".to_string());
        }
        if !password
            .chars()
            .any(|ch| r#"!@#$%^&*()_+-=[]{};:'",.<>?/\|`~"#.contains(ch))
        {
            return Err("密码必须包含至少一个特殊字符".to_string());
        }
    }
    Ok(())
}

struct RegistrationPrivacyPolicySettings {
    enabled: bool,
    version: String,
}

async fn read_registration_privacy_policy_settings(
    state: &AppState,
) -> Result<RegistrationPrivacyPolicySettings, GatewayError> {
    let enabled = state
        .read_system_config_json_value("registration_privacy_policy_enabled")
        .await?;
    let version = state
        .read_system_config_json_value("registration_privacy_policy_version")
        .await?;
    Ok(RegistrationPrivacyPolicySettings {
        enabled: system_config_bool(enabled.as_ref(), false),
        version: system_config_string(version.as_ref()).unwrap_or_else(|| "1".to_string()),
    })
}

pub(crate) async fn auth_password_policy_level(state: &AppState) -> Result<String, GatewayError> {
    let config = state
        .read_system_config_json_value("password_policy_level")
        .await?;
    Ok(match system_config_string(config.as_ref()) {
        Some(value) if matches!(value.as_str(), "weak" | "medium" | "strong") => value,
        _ => "weak".to_string(),
    })
}

async fn validate_auth_email_suffix(
    state: &AppState,
    email: &str,
) -> Result<Result<(), String>, GatewayError> {
    let mode_config = state
        .read_system_config_json_value("email_suffix_mode")
        .await?;
    let mode = system_config_string(mode_config.as_ref()).unwrap_or_else(|| "none".to_string());
    if mode == "none" {
        return Ok(Ok(()));
    }

    let suffixes_config = state
        .read_system_config_json_value("email_suffix_list")
        .await?;
    let suffixes = system_config_string_list(suffixes_config.as_ref());
    if suffixes.is_empty() {
        return Ok(Ok(()));
    }

    let Some((_, suffix)) = email.split_once('@') else {
        return Ok(Err("邮箱格式无效".to_string()));
    };
    let suffix = suffix.to_ascii_lowercase();
    if mode == "whitelist" && !suffixes.iter().any(|item| item == &suffix) {
        return Ok(Err(format!(
            "该邮箱后缀不在允许列表中，仅支持: {}",
            suffixes.join(", ")
        )));
    }
    if mode == "blacklist" && suffixes.iter().any(|item| item == &suffix) {
        return Ok(Err(format!("该邮箱后缀 ({suffix}) 不允许注册")));
    }
    Ok(Ok(()))
}

pub(super) async fn handle_auth_send_verification_code(
    state: &AppState,
    headers: &http::HeaderMap,
    cf_connecting_ip: Option<&str>,
    request_body: Option<&axum::body::Bytes>,
) -> Response<Body> {
    let Some(request_body) = request_body else {
        return build_auth_error_response(http::StatusCode::BAD_REQUEST, "请求数据验证失败", false);
    };
    let payload = match serde_json::from_slice::<AuthEmailRequest>(request_body) {
        Ok(value) => value,
        Err(_) => {
            return build_auth_error_response(
                http::StatusCode::BAD_REQUEST,
                "请求数据验证失败",
                false,
            );
        }
    };
    let Some(email) = normalize_auth_email(&payload.email) else {
        return build_auth_error_response(http::StatusCode::BAD_REQUEST, "邮箱格式无效", false);
    };

    if let Err(response) = verify_auth_turnstile(
        state,
        headers,
        cf_connecting_ip,
        payload.turnstile_token.as_deref(),
        AuthTurnstileAction::SendVerificationCode,
    )
    .await
    {
        return response;
    }

    match validate_auth_email_suffix(state, &email).await {
        Ok(Ok(())) => {}
        Ok(Err(detail)) => {
            return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false);
        }
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("auth settings lookup failed: {err:?}"),
                false,
            );
        }
    }

    if state
        .find_user_auth_by_identifier(&email)
        .await
        .ok()
        .flatten()
        .is_some()
    {
        return build_auth_error_response(
            http::StatusCode::BAD_REQUEST,
            "该邮箱已被注册，请直接登录或使用其他邮箱",
            false,
        );
    }

    let smtp_config = match read_auth_smtp_config(state).await {
        Ok(Some(value)) => value,
        Ok(None) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "发送验证码失败，请稍后重试",
                false,
            );
        }
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("auth smtp settings lookup failed: {err:?}"),
                false,
            );
        }
    };

    let now = auth_now();
    if let Ok(Some(stored)) = read_auth_email_verification_code(state, &email).await {
        let created_at = chrono::DateTime::parse_from_rfc3339(&stored.created_at)
            .ok()
            .map(|value| value.with_timezone(&chrono::Utc));
        let expires_at = created_at.map(|value| {
            value + chrono::Duration::minutes(auth_verification_code_expire_minutes())
        });
        if expires_at.is_some_and(|value| value <= now) {
            let _ = clear_auth_email_pending_code(state, &email).await;
        } else if let Some(created_at) = created_at {
            let elapsed = now.signed_duration_since(created_at).num_seconds();
            let remaining = auth_verification_send_cooldown_seconds() - elapsed;
            if remaining > 0 {
                return build_auth_error_response(
                    http::StatusCode::BAD_REQUEST,
                    format!("请在 {remaining} 秒后重试"),
                    false,
                );
            }
        }
    }

    let expire_minutes = auth_verification_code_expire_minutes();
    let code = generate_auth_verification_code();
    let email_message =
        match build_auth_verification_email(state, &email, &code, expire_minutes).await {
            Ok(value) => value,
            Err(err) => {
                return build_auth_error_response(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("auth verification email render failed: {err:?}"),
                    false,
                );
            }
        };

    if let Err(err) = store_auth_email_verification_code(
        state,
        &email,
        &code,
        now,
        u64::try_from(expire_minutes.saturating_mul(60)).unwrap_or(300),
    )
    .await
    {
        return build_auth_error_response(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("auth verification code save failed: {err:?}"),
            false,
        );
    }

    if let Err(_err) = send_auth_email(state, smtp_config, email_message).await {
        let _ = clear_auth_email_pending_code(state, &email).await;
        return build_auth_error_response(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "发送验证码失败，请稍后重试",
            false,
        );
    }

    build_auth_json_response(
        http::StatusCode::OK,
        json!({
            "message": "验证码已发送，请查收邮件",
            "success": true,
            "expire_minutes": expire_minutes,
        }),
        None,
    )
}

pub(super) async fn handle_auth_register(
    state: &AppState,
    headers: &http::HeaderMap,
    cf_connecting_ip: Option<&str>,
    request_body: Option<&axum::body::Bytes>,
) -> Response<Body> {
    let Some(request_body) = request_body else {
        return build_auth_error_response(http::StatusCode::BAD_REQUEST, "缺少请求体", false);
    };
    let payload = match serde_json::from_slice::<AuthRegisterRequest>(request_body) {
        Ok(value) => value,
        Err(_) => {
            return build_auth_error_response(http::StatusCode::BAD_REQUEST, "输入验证失败", false)
        }
    };
    let email = match normalize_auth_optional_email(payload.email.as_deref()) {
        Ok(value) => value,
        Err(detail) => {
            return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false);
        }
    };
    let username = match validate_auth_register_username(&payload.username) {
        Ok(value) => value,
        Err(detail) => {
            return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false);
        }
    };
    let password_policy = match auth_password_policy_level(state).await {
        Ok(value) => value,
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("auth settings lookup failed: {err:?}"),
                false,
            );
        }
    };
    if let Err(detail) = validate_auth_register_password(&payload.password, &password_policy) {
        return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false);
    }

    let enable_registration = match state
        .read_system_config_json_value("enable_registration")
        .await
    {
        Ok(value) => system_config_bool(value.as_ref(), false),
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("auth settings lookup failed: {err:?}"),
                false,
            );
        }
    };
    if !enable_registration {
        return build_auth_error_response(http::StatusCode::FORBIDDEN, "系统暂不开放注册", false);
    }
    let privacy_policy = match read_registration_privacy_policy_settings(state).await {
        Ok(value) => value,
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("auth settings lookup failed: {err:?}"),
                false,
            );
        }
    };
    if privacy_policy.enabled {
        let accepted = payload.privacy_policy_accepted.unwrap_or(false);
        let accepted_version = payload
            .privacy_policy_version
            .as_deref()
            .map(str::trim)
            .unwrap_or_default();
        if !accepted || accepted_version != privacy_policy.version {
            return build_auth_error_response(
                http::StatusCode::BAD_REQUEST,
                "请先阅读并同意当前版本的隐私政策",
                false,
            );
        }
    }

    if let Err(response) = verify_auth_turnstile(
        state,
        headers,
        cf_connecting_ip,
        payload.turnstile_token.as_deref(),
        AuthTurnstileAction::Register,
    )
    .await
    {
        return response;
    }

    let email_configured = match auth_registration_email_configured(state).await {
        Ok(value) => value,
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("auth settings lookup failed: {err:?}"),
                false,
            );
        }
    };
    let require_verification = match state
        .read_system_config_json_value("require_email_verification")
        .await
    {
        Ok(value) => system_config_bool(value.as_ref(), false) && email_configured,
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("auth settings lookup failed: {err:?}"),
                false,
            );
        }
    };

    if require_verification && email.is_none() {
        return build_auth_error_response(
            http::StatusCode::BAD_REQUEST,
            "系统要求邮箱验证，请填写邮箱",
            false,
        );
    }
    if require_verification {
        if let Some(email) = email.as_deref() {
            let is_verified = match auth_email_is_verified(state, email).await {
                Ok(value) => value,
                Err(err) => {
                    return build_auth_error_response(
                        http::StatusCode::INTERNAL_SERVER_ERROR,
                        format!("auth verification lookup failed: {err:?}"),
                        false,
                    );
                }
            };
            if !is_verified {
                return build_auth_error_response(
                    http::StatusCode::BAD_REQUEST,
                    "请先完成邮箱验证。请发送验证码并验证后再注册。",
                    false,
                );
            }
        }
    }
    if let Some(email) = email.as_deref() {
        match validate_auth_email_suffix(state, email).await {
            Ok(Ok(())) => {}
            Ok(Err(detail)) => {
                return build_auth_error_response(http::StatusCode::BAD_REQUEST, detail, false);
            }
            Err(err) => {
                return build_auth_error_response(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("auth settings lookup failed: {err:?}"),
                    false,
                );
            }
        }
        if state
            .find_user_auth_by_identifier(email)
            .await
            .ok()
            .flatten()
            .is_some()
        {
            return build_auth_error_response(
                http::StatusCode::BAD_REQUEST,
                format!("邮箱已存在: {email}"),
                false,
            );
        }
    }
    if state
        .find_user_auth_by_identifier(&username)
        .await
        .ok()
        .flatten()
        .is_some()
    {
        return build_auth_error_response(
            http::StatusCode::BAD_REQUEST,
            format!("用户名已存在: {username}"),
            false,
        );
    }

    let password_hash = match bcrypt::hash(&payload.password, bcrypt::DEFAULT_COST) {
        Ok(value) => value,
        Err(_) => {
            return build_auth_error_response(
                http::StatusCode::BAD_REQUEST,
                "密码长度不能超过72字节",
                false,
            );
        }
    };
    let initial_balance = match state
        .read_system_config_json_value("default_user_initial_balance_usd")
        .await
    {
        Ok(value) => system_config_f64(value.as_ref(), 10.0),
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("auth settings lookup failed: {err:?}"),
                false,
            );
        }
    };
    let Some((user, _wallet)) = (match state
        .register_local_auth_user(
            email.clone(),
            require_verification && email.is_some(),
            username.clone(),
            password_hash,
            initial_balance,
            false,
        )
        .await
    {
        Ok(value) => value,
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("auth register failed: {err:?}"),
                false,
            );
        }
    }) else {
        return build_auth_error_response(
            http::StatusCode::SERVICE_UNAVAILABLE,
            AUTH_REGISTRATION_STORAGE_UNAVAILABLE_DETAIL,
            false,
        );
    };
    if let Err(err) = state
        .assign_default_group_to_self_registered_user(&user.id)
        .await
    {
        let _ = state.delete_local_auth_user(&user.id).await;
        return build_auth_error_response(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("auth default user group assignment failed: {err:?}"),
            false,
        );
    }
    if privacy_policy.enabled {
        match state
            .record_user_privacy_policy_acceptance(&user.id, &privacy_policy.version)
            .await
        {
            Ok(true) => {}
            Ok(false) => {
                let _ = state.delete_local_auth_user(&user.id).await;
                return build_auth_error_response(
                    http::StatusCode::SERVICE_UNAVAILABLE,
                    AUTH_REGISTRATION_STORAGE_UNAVAILABLE_DETAIL,
                    false,
                );
            }
            Err(err) => {
                let _ = state.delete_local_auth_user(&user.id).await;
                return build_auth_error_response(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("auth privacy policy acceptance failed: {err:?}"),
                    false,
                );
            }
        }
    }
    if require_verification {
        if let Some(email) = email.as_deref() {
            let _ = clear_auth_email_verification(state, email).await;
        }
    }
    build_auth_json_response(
        http::StatusCode::OK,
        json!({
            "user_id": user.id,
            "email": user.email,
            "username": user.username,
            "message": "注册成功",
        }),
        None,
    )
}

pub(super) async fn handle_auth_verify_email(
    state: &AppState,
    request_body: Option<&axum::body::Bytes>,
) -> Response<Body> {
    let Some(request_body) = request_body else {
        return build_auth_error_response(http::StatusCode::BAD_REQUEST, "缺少请求体", false);
    };
    let payload = match serde_json::from_slice::<AuthVerifyEmailRequest>(request_body) {
        Ok(value) => value,
        Err(_) => {
            return build_auth_error_response(http::StatusCode::BAD_REQUEST, "输入验证失败", false)
        }
    };
    let Some(email) = normalize_auth_email(&payload.email) else {
        return build_auth_error_response(http::StatusCode::BAD_REQUEST, "邮箱格式无效", false);
    };
    let code = payload.code.trim();
    if code.len() != 6 || !code.chars().all(|ch| ch.is_ascii_digit()) {
        return build_auth_error_response(
            http::StatusCode::BAD_REQUEST,
            "验证码必须是6位数字",
            false,
        );
    }
    let pending = match read_auth_email_verification_code(state, &email).await {
        Ok(value) => value,
        Err(err) => {
            return build_auth_error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("verification lookup failed: {err:?}"),
                false,
            )
        }
    };
    let Some(pending) = pending else {
        return build_auth_error_response(
            http::StatusCode::BAD_REQUEST,
            "验证码不存在或已过期",
            false,
        );
    };
    let created_at = chrono::DateTime::parse_from_rfc3339(&pending.created_at)
        .ok()
        .map(|value| value.with_timezone(&chrono::Utc));
    let expires_at = created_at
        .map(|value| value + chrono::Duration::minutes(auth_verification_code_expire_minutes()));
    if expires_at.is_some_and(|value| value <= auth_now()) {
        let _ = clear_auth_email_pending_code(state, &email).await;
        return build_auth_error_response(
            http::StatusCode::BAD_REQUEST,
            "验证码不存在或已过期",
            false,
        );
    }
    if pending.code != code {
        return build_auth_error_response(http::StatusCode::BAD_REQUEST, "验证码错误", false);
    }
    if mark_auth_email_verified(state, &email).await.ok() != Some(true) {
        return build_auth_error_response(http::StatusCode::BAD_REQUEST, "系统错误", false);
    }
    let _ = clear_auth_email_pending_code(state, &email).await;
    build_auth_json_response(
        http::StatusCode::OK,
        json!({ "message": "邮箱验证成功", "success": true }),
        None,
    )
}

pub(super) async fn handle_auth_verification_status(
    state: &AppState,
    request_body: Option<&axum::body::Bytes>,
) -> Response<Body> {
    let Some(request_body) = request_body else {
        return build_auth_error_response(http::StatusCode::BAD_REQUEST, "缺少请求体", false);
    };
    let payload = match serde_json::from_slice::<AuthEmailRequest>(request_body) {
        Ok(value) => value,
        Err(_) => {
            return build_auth_error_response(http::StatusCode::BAD_REQUEST, "输入验证失败", false)
        }
    };
    let Some(email) = normalize_auth_email(&payload.email) else {
        return build_auth_error_response(http::StatusCode::BAD_REQUEST, "邮箱格式无效", false);
    };
    let pending = read_auth_email_verification_code(state, &email)
        .await
        .ok()
        .flatten();
    let is_verified = auth_email_is_verified(state, &email).await.unwrap_or(false);
    let now = auth_now();
    let (has_pending_code, cooldown_remaining, code_expires_in) = if let Some(pending) = pending {
        let created_at = chrono::DateTime::parse_from_rfc3339(&pending.created_at)
            .ok()
            .map(|value| value.with_timezone(&chrono::Utc));
        let expires_at = created_at.map(|value| {
            value + chrono::Duration::minutes(auth_verification_code_expire_minutes())
        });
        if expires_at.is_some_and(|value| value <= now) {
            let _ = clear_auth_email_pending_code(state, &email).await;
            (false, None, None)
        } else {
            let cooldown_remaining = created_at.and_then(|value| {
                let elapsed = now.signed_duration_since(value).num_seconds();
                let remaining = auth_verification_send_cooldown_seconds() - elapsed;
                (remaining > 0)
                    .then_some(i32::try_from(remaining).ok())
                    .flatten()
            });
            let code_expires_in = expires_at.and_then(|value| {
                let remaining = value.signed_duration_since(now).num_seconds();
                (remaining > 0)
                    .then_some(i32::try_from(remaining).ok())
                    .flatten()
            });
            (true, cooldown_remaining, code_expires_in)
        }
    } else {
        (false, None, None)
    };
    build_auth_json_response(
        http::StatusCode::OK,
        json!({
            "email": email,
            "has_pending_code": has_pending_code,
            "is_verified": is_verified,
            "cooldown_remaining": cooldown_remaining,
            "code_expires_in": code_expires_in,
        }),
        None,
    )
}
