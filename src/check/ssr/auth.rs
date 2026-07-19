//! Auth JSON parse, OAuth refresh, JWT helpers, request headers.

use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::Engine;
use reqwest::header::{
    AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, USER_AGENT,
};
use serde_json::Value;

use crate::check::AuthUpload;

pub const DEFAULT_BASE_URL: &str = "https://cli-chat-proxy.grok.com/v1";
pub const DEFAULT_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
pub const DEFAULT_TOKEN_ENDPOINT: &str = "https://auth.x.ai/oauth2/token";

#[derive(Debug, Clone)]
pub struct AuthRecord {
    pub email: String,
    pub access_token: String,
    pub refresh_token: String,
    pub client_id: String,
    pub token_endpoint: String,
    pub base_url: String,
    pub headers: HashMap<String, String>,
    pub disabled: bool,
    /// 原始 JSON，用于合并 refresh 后回传浏览器
    pub raw: Value,
}

pub async fn try_refresh(
    client: &reqwest::Client,
    auth: &mut AuthRecord,
) -> Result<Option<String>, String> {
    let refresh_token = auth.refresh_token.trim();
    if refresh_token.is_empty() {
        return Err("missing_refresh_token".into());
    }

    let endpoint = if auth.token_endpoint.trim().is_empty() {
        DEFAULT_TOKEN_ENDPOINT
    } else {
        auth.token_endpoint.trim()
    };
    let client_id = if auth.client_id.trim().is_empty() {
        DEFAULT_CLIENT_ID
    } else {
        auth.client_id.trim()
    };

    // OAuth refresh_token grant
    let resp = client
        .post(endpoint)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await
        .map_err(|e| format!("network: {e}"))?;

    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if status != 200 {
        return Err(format!(
            "HTTP {status}: {}",
            text.chars().take(200).collect::<String>()
        ));
    }

    let token: Value =
        serde_json::from_str(&text).map_err(|e| format!("invalid token json: {e}"))?;
    let access = token
        .get("access_token")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "refresh response missing access_token".to_string())?;

    auth.access_token = access.to_string();
    apply_token_fields(
        auth,
        access,
        token
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty()),
        token.get("id_token").cloned(),
        token.get("token_type").cloned(),
        token.get("expires_in").cloned(),
    );

    let note = token
        .get("expires_in")
        .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|u| u as i64)))
        .map(|secs| format!("新 token 约 {} 小时有效", (secs / 3600).max(1)));

    // 服务端不落盘，回传 updated_content 写浏览器内存
    Ok(note)
}

/// 把刷新结果写回 auth 字段（不落盘）
fn apply_token_fields(
    auth: &mut AuthRecord,
    access: &str,
    new_refresh: Option<&str>,
    id_token: Option<Value>,
    token_type: Option<Value>,
    expires_in: Option<Value>,
) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    if let Some(rt) = new_refresh {
        auth.refresh_token = rt.to_string();
    }

    let Some(obj) = auth.raw.as_object_mut() else {
        return;
    };

    obj.insert("access_token".into(), Value::String(access.to_string()));
    // 兼容 oauth_* 别名字段
    if obj.contains_key("oauth_access_token") {
        obj.insert(
            "oauth_access_token".into(),
            Value::String(access.to_string()),
        );
    }

    if let Some(rt) = new_refresh {
        obj.insert("refresh_token".into(), Value::String(rt.to_string()));
        if obj.contains_key("oauth_refresh_token") {
            obj.insert("oauth_refresh_token".into(), Value::String(rt.to_string()));
        }
    }

    if let Some(id) = id_token {
        obj.insert("id_token".into(), id);
    }
    if let Some(tt) = token_type {
        obj.insert("token_type".into(), tt);
    }

    let mut expires_at = None;
    if let Some(ei) = expires_in {
        obj.insert("expires_in".into(), ei.clone());
        if let Some(secs) = ei.as_i64().or_else(|| ei.as_u64().map(|u| u as i64)) {
            expires_at = Some(now + secs);
        }
    }
    if let Some(exp) = expires_at {
        // expired / last_refresh 用 ISO UTC
        obj.insert("expired".into(), Value::String(format_unix_utc(exp)));
        obj.insert("expires_at".into(), Value::Number(exp.into()));
    }
    obj.insert("last_refresh".into(), Value::String(format_unix_utc(now)));
}

pub fn humanize_refresh_err(err: &str) -> String {
    let lower = err.to_ascii_lowercase();
    if lower.contains("invalid_grant")
        || lower.contains("revoked")
        || lower.contains("invalid refresh")
    {
        "refresh_token 已失效（被吊销或轮换），请重新登录拿新 auth，或改用上次导出的新文件".into()
    } else if lower.contains("missing_refresh_token") {
        "文件缺少 refresh_token，无法自动刷新".into()
    } else if lower.contains("network") {
        format!(
            "刷新请求网络失败：{}",
            err.chars().take(100).collect::<String>()
        )
    } else {
        format!("刷新失败：{}", err.chars().take(120).collect::<String>())
    }
}

/// days since 1970-01-01 → YYYY-MM-DDTHH:MM:SSZ (UTC), no extra deps
fn format_unix_utc(secs: i64) -> String {
    let secs = secs.max(0) as u64;
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    // civil_from_days (Howard Hinnant)
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if mth <= 2 { y + 1 } else { y };
    format!("{year:04}-{mth:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

pub fn serialize_auth(auth: &AuthRecord) -> Option<String> {
    serde_json::to_string_pretty(&auth.raw).ok()
}

pub fn resolve_auth_record(file: &AuthUpload) -> Result<AuthRecord, String> {
    let data: Value =
        serde_json::from_str(&file.content).map_err(|e| format!("invalid json: {e}"))?;
    let obj = data
        .as_object()
        .ok_or_else(|| "JSON root must be an object".to_string())?;

    let token =
        first_string(obj, &["access_token", "oauth_access_token", "token"]).unwrap_or_default();
    let refresh_token =
        first_string(obj, &["refresh_token", "oauth_refresh_token"]).unwrap_or_default();
    let client_id = first_string(obj, &["client_id"]).unwrap_or_else(|| DEFAULT_CLIENT_ID.into());
    let token_endpoint =
        first_string(obj, &["token_endpoint"]).unwrap_or_else(|| DEFAULT_TOKEN_ENDPOINT.into());
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
        refresh_token,
        client_id,
        token_endpoint,
        base_url,
        headers,
        disabled,
        raw: data,
    })
}

fn first_string(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(v) = obj.get(*key)
            && let Some(s) = v.as_str()
        {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

pub fn normalize_base_url(base_url: &str) -> String {
    let base = base_url.trim();
    if base.is_empty() || base.contains("api.x.ai") {
        DEFAULT_BASE_URL.to_string()
    } else {
        base.trim_end_matches('/').to_string()
    }
}

pub fn build_headers(auth: &AuthRecord) -> Result<HeaderMap, String> {
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

pub fn jwt_claim_tier(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let payload = b64url_json(parts[1])?;
    payload
        .get("tier")
        .and_then(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .or_else(|| v.as_i64().map(|n| n.to_string()))
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        })
        .filter(|s| !s.trim().is_empty())
}

pub fn jwt_expired(token: &str) -> bool {
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
