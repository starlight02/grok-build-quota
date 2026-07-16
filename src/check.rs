use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthUpload {
    pub filename: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AccountStatus {
    Ok,
    Exhausted,
    AuthFailed,
    Disabled,
    Expired,
    Invalid,
    NetworkError,
    Error,
}

impl AccountStatus {
    pub fn as_label(&self) -> &'static str {
        match self {
            Self::Ok => "可用",
            Self::Exhausted => "额度耗尽",
            Self::AuthFailed => "鉴权失败",
            Self::Disabled => "已禁用",
            Self::Expired => "Token 过期",
            Self::Invalid => "无效文件",
            Self::NetworkError => "网络错误",
            Self::Error => "检测失败",
        }
    }

    pub fn tone(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Exhausted => "warn",
            Self::AuthFailed | Self::Disabled | Self::Expired | Self::Invalid | Self::Error => {
                "bad"
            }
            Self::NetworkError => "mute",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CheckResult {
    pub account: String,
    pub filename: String,
    pub status: AccountStatus,
    pub status_label: String,
    pub quota: String,
    pub usable: bool,
    pub remaining_tokens: Option<i64>,
    pub limit_tokens: Option<i64>,
    pub remaining_requests: Option<i64>,
    pub limit_requests: Option<i64>,
    pub http_status: Option<u16>,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CheckSummary {
    pub total: usize,
    pub usable: usize,
    pub exhausted: usize,
    pub failed: usize,
    pub results: Vec<CheckResult>,
}

#[server(CheckAuthFile, "/api")]
pub async fn check_auth_file(file: AuthUpload) -> Result<CheckResult, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        return Ok(ssr::check_one(ssr::shared_client(), file).await);
    }

    #[cfg(not(feature = "ssr"))]
    {
        let _ = file;
        Err(ServerFnError::new("server only"))
    }
}

#[cfg(feature = "ssr")]
mod ssr {
    use super::*;

    const DEFAULT_BASE_URL: &str = "https://cli-chat-proxy.grok.com/v1";
    const PROBE_MODEL: &str = "grok-4.5";
    use std::{
        collections::HashMap,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use base64::Engine;
    use regex::Regex;
    use reqwest::header::{
        AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, USER_AGENT,
    };
    use serde_json::Value;

    #[derive(Debug)]
    struct AuthRecord {
        email: String,
        access_token: String,
        base_url: String,
        headers: HashMap<String, String>,
        disabled: bool,
    }

    static CLIENT: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(build_client);

    pub fn shared_client() -> &'static reqwest::Client {
        &CLIENT
    }

    fn build_client() -> reqwest::Client {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(45))
            .user_agent("grok-cli/0.2.93")
            .redirect(reqwest::redirect::Policy::limited(5));

        if let Ok(proxy) = std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("HTTP_PROXY")) {
            if let Ok(p) = reqwest::Proxy::all(proxy) {
                builder = builder.proxy(p);
            }
        }

        builder.build().unwrap_or_else(|_| reqwest::Client::new())
    }

    pub async fn check_one(client: &reqwest::Client, file: AuthUpload) -> CheckResult {
        let filename = file.filename.clone();
        let auth = match resolve_auth_record(&file) {
            Ok(v) => v,
            Err(err) => {
                return CheckResult {
                    account: filename.clone(),
                    filename: filename.clone(),
                    status: AccountStatus::Invalid,
                    status_label: AccountStatus::Invalid.as_label().into(),
                    quota: "--".into(),
                    usable: false,
                    remaining_tokens: None,
                    limit_tokens: None,
                    remaining_requests: None,
                    limit_requests: None,
                    http_status: None,
                    detail: Some(err),
                };
            }
        };

        let account = if auth.email.trim().is_empty() {
            filename.clone()
        } else {
            auth.email.trim().to_string()
        };

        if auth.disabled {
            return CheckResult {
                account,
                filename,
                status: AccountStatus::Disabled,
                status_label: AccountStatus::Disabled.as_label().into(),
                quota: "--".into(),
                usable: false,
                remaining_tokens: None,
                limit_tokens: None,
                remaining_requests: None,
                limit_requests: None,
                http_status: None,
                detail: Some("disabled=true".into()),
            };
        }

        if auth.access_token.trim().is_empty() {
            return CheckResult {
                account,
                filename,
                status: AccountStatus::Invalid,
                status_label: AccountStatus::Invalid.as_label().into(),
                quota: "--".into(),
                usable: false,
                remaining_tokens: None,
                limit_tokens: None,
                remaining_requests: None,
                limit_requests: None,
                http_status: None,
                detail: Some("missing access_token".into()),
            };
        }

        if jwt_expired(&auth.access_token) {
            return CheckResult {
                account,
                filename,
                status: AccountStatus::Expired,
                status_label: AccountStatus::Expired.as_label().into(),
                quota: "--".into(),
                usable: false,
                remaining_tokens: None,
                limit_tokens: None,
                remaining_requests: None,
                limit_requests: None,
                http_status: None,
                detail: Some("access_token_expired".into()),
            };
        }

        let base_url = normalize_base_url(&auth.base_url);
        let url = format!("{}/responses", base_url.trim_end_matches('/'));
        let headers = match build_headers(&auth) {
            Ok(h) => h,
            Err(err) => {
                return CheckResult {
                    account,
                    filename,
                    status: AccountStatus::Error,
                    status_label: AccountStatus::Error.as_label().into(),
                    quota: "--".into(),
                    usable: false,
                    remaining_tokens: None,
                    limit_tokens: None,
                    remaining_requests: None,
                    limit_requests: None,
                    http_status: None,
                    detail: Some(err),
                };
            }
        };

        let body = serde_json::json!({
            "model": PROBE_MODEL,
            "input": "Reply exactly: OK",
            "max_output_tokens": 8,
        });

        let resp = match client.post(&url).headers(headers).json(&body).send().await {
            Ok(r) => r,
            Err(err) => {
                return CheckResult {
                    account,
                    filename,
                    status: AccountStatus::NetworkError,
                    status_label: AccountStatus::NetworkError.as_label().into(),
                    quota: "--".into(),
                    usable: false,
                    remaining_tokens: None,
                    limit_tokens: None,
                    remaining_requests: None,
                    limit_requests: None,
                    http_status: None,
                    detail: Some(err.to_string()),
                };
            }
        };

        let status_code = resp.status().as_u16();
        let header_limit = header_i64(resp.headers(), "x-ratelimit-limit-tokens");
        let header_remaining = header_i64(resp.headers(), "x-ratelimit-remaining-tokens");
        let header_req_limit = header_i64(resp.headers(), "x-ratelimit-limit-requests");
        let header_req_remaining = header_i64(resp.headers(), "x-ratelimit-remaining-requests");
        let text = resp.text().await.unwrap_or_default();

        let mut remaining_tokens = header_remaining;
        let mut limit_tokens = header_limit;
        let mut detail = None;

        if status_code == 429 {
            if let Some((actual, limit)) = parse_exhausted(&text) {
                limit_tokens = Some(limit);
                remaining_tokens = Some((limit - actual).max(0));
                detail = Some(format!(
                    "subscription:free-usage-exhausted actual/limit={actual}/{limit}"
                ));
            } else if !text.is_empty() {
                detail = Some(text.chars().take(180).collect());
            }
        } else if status_code >= 400 {
            detail = Some(extract_error_message(&text));
        }

        let quota = format_quota(remaining_tokens, limit_tokens);
        let (status, usable) = match status_code {
            200 => (AccountStatus::Ok, true),
            429 => (AccountStatus::Exhausted, false),
            401 | 403 => (AccountStatus::AuthFailed, false),
            _ => (AccountStatus::Error, false),
        };

        CheckResult {
            account,
            filename,
            status: status.clone(),
            status_label: status.as_label().into(),
            quota,
            usable,
            remaining_tokens,
            limit_tokens,
            remaining_requests: header_req_remaining,
            limit_requests: header_req_limit,
            http_status: Some(status_code),
            detail,
        }
    }

    fn resolve_auth_record(file: &AuthUpload) -> Result<AuthRecord, String> {
        let data: Value =
            serde_json::from_str(&file.content).map_err(|e| format!("invalid json: {e}"))?;
        let obj = data
            .as_object()
            .ok_or_else(|| "JSON root must be an object".to_string())?;

        // accounts_output style: linked cliproxyapi_auth path is not resolvable from browser
        // upload context, so fall through to embedded tokens.

        let token =
            first_string(obj, &["access_token", "oauth_access_token", "token"]).unwrap_or_default();
        let base_url = first_string(obj, &["base_url", "build_base_url"])
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let email = first_string(obj, &["email"]).unwrap_or_default();
        let disabled = obj
            .get("disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut headers = HashMap::new();
        if let Some(Value::Object(map)) = obj.get("headers") {
            for (k, v) in map {
                if let Some(s) = v.as_str() {
                    headers.insert(k.clone(), s.to_string());
                }
            }
        }

        Ok(AuthRecord {
            email,
            access_token: token,
            base_url,
            headers,
            disabled,
        })
    }

    fn first_string(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
        for key in keys {
            if let Some(v) = obj.get(*key) {
                if let Some(s) = v.as_str() {
                    let t = s.trim();
                    if !t.is_empty() {
                        return Some(t.to_string());
                    }
                }
            }
        }
        None
    }

    fn normalize_base_url(base_url: &str) -> String {
        let base = base_url.trim();
        if base.is_empty() || base.contains("api.x.ai") {
            DEFAULT_BASE_URL.to_string()
        } else {
            base.trim_end_matches('/').to_string()
        }
    }

    fn build_headers(auth: &AuthRecord) -> Result<HeaderMap, String> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", auth.access_token))
                .map_err(|e| e.to_string())?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            HeaderName::from_static("accept"),
            HeaderValue::from_static("application/json"),
        );
        headers.insert(USER_AGENT, HeaderValue::from_static("grok-cli/0.2.93"));
        headers.insert(
            HeaderName::from_static("x-xai-token-auth"),
            HeaderValue::from_static("xai-grok-cli"),
        );
        headers.insert(
            HeaderName::from_static("x-grok-client-version"),
            HeaderValue::from_static("0.2.93"),
        );
        headers.insert(
            HeaderName::from_static("x-grok-client-identifier"),
            HeaderValue::from_static("grok-shell"),
        );

        for (k, v) in &auth.headers {
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(v),
            ) {
                headers.insert(name, value);
            }
        }
        Ok(headers)
    }

    fn header_i64(headers: &HeaderMap, name: &str) -> Option<i64> {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<i64>().ok())
    }

    fn parse_exhausted(body: &str) -> Option<(i64, i64)> {
        let re = Regex::new(r"(?i)tokens\s*\(actual/limit\)\s*:\s*(\d+)\s*/\s*(\d+)").ok()?;
        let caps = re.captures(body)?;
        let actual = caps.get(1)?.as_str().parse().ok()?;
        let limit = caps.get(2)?.as_str().parse().ok()?;
        Some((actual, limit))
    }

    fn extract_error_message(body: &str) -> String {
        if let Ok(v) = serde_json::from_str::<Value>(body) {
            if let Some(err) = v.get("error") {
                return err.to_string().chars().take(180).collect();
            }
        }
        body.chars().take(180).collect()
    }

    fn jwt_expired(token: &str) -> bool {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() < 2 {
            return true;
        }
        let payload = match b64url_json(parts[1]) {
            Some(v) => v,
            None => return true,
        };
        let exp = match payload.get("exp").and_then(|v| v.as_i64()) {
            Some(v) => v,
            None => return false,
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        exp < now
    }

    fn b64url_json(segment: &str) -> Option<Value> {
        let rem = segment.len() % 4;
        let padded = if rem == 0 {
            segment.to_string()
        } else {
            format!("{}{}", segment, "=".repeat(4 - rem))
        };
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(segment)
            .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(padded.as_bytes()))
            .ok()?;
        serde_json::from_slice(&raw).ok()
    }

    fn format_quota(remaining: Option<i64>, limit: Option<i64>) -> String {
        match (remaining, limit) {
            (Some(r), Some(l)) => format!("{r} / {l}"),
            (Some(r), None) => format!("{r} / --"),
            (None, Some(l)) => format!("-- / {l}"),
            _ => "--".into(),
        }
    }
}
