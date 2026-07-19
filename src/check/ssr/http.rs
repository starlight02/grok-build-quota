//! Generic HTTP helpers: JSON GET, header parse, error body extraction.

use regex::Regex;
use reqwest::header::HeaderMap;
use serde_json::Value;

pub async fn get_json(client: &reqwest::Client, url: String, headers: HeaderMap) -> Option<Value> {
    let response = client.get(url).headers(headers).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json().await.ok()
}

pub fn header_i64(headers: &HeaderMap, name: &str) -> Option<i64> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok())
}

pub fn json_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub fn parse_exhausted(body: &str) -> Option<(i64, i64)> {
    let re = Regex::new(r"(?i)tokens\s*\(actual/limit\)\s*:\s*(\d+)\s*/\s*(\d+)").ok()?;
    let caps = re.captures(body)?;
    let actual = caps.get(1)?.as_str().parse().ok()?;
    let limit = caps.get(2)?.as_str().parse().ok()?;
    Some((actual, limit))
}

pub fn extract_error_message(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(body)
        && let Some(err) = v.get("error")
    {
        return err.to_string().chars().take(180).collect();
    }
    body.chars().take(180).collect()
}

/// 展平嵌套 error JSON 为可匹配字符串（对齐 Python _error_text_parts）
pub fn flatten_error_parts(v: &Value) -> Vec<String> {
    match v {
        Value::Object(map) => {
            let mut out = Vec::new();
            for key in ["message", "code", "type", "error"] {
                if let Some(inner) = map.get(key) {
                    out.extend(flatten_error_parts(inner));
                }
            }
            if out.is_empty() {
                out.push(v.to_string());
            }
            out
        }
        Value::String(s) => vec![s.clone()],
        other => vec![other.to_string()],
    }
}

pub fn error_text_parts_of(body: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    v.get("error").map(flatten_error_parts).unwrap_or_default()
}

/// 顶层 "code" 字段（Python upstream_code）
pub fn top_level_code(body: &str) -> Option<String> {
    let v = serde_json::from_str::<Value>(body).ok()?;
    v.get("code")?
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn joined_candidates(body: &str, parts: &[String], code: Option<&str>) -> String {
    let mut candidates: Vec<&str> = Vec::new();
    if let Some(c) = code {
        candidates.push(c);
    }
    candidates.extend(parts.iter().map(|s| s.as_str()));
    let mut joined = candidates.join(" ").to_ascii_lowercase();
    if !body.is_empty() {
        if !joined.is_empty() {
            joined.push(' ');
        }
        joined.push_str(&body.to_ascii_lowercase());
    }
    joined
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_code_and_error_parts_parse() {
        let body = r#"{"code":"some_error","error":{"message":"m","type":"t"}}"#;
        assert_eq!(top_level_code(body).as_deref(), Some("some_error"));
        let parts = error_text_parts_of(body);
        assert!(parts.iter().any(|p| p == "m"));
        assert!(parts.iter().any(|p| p == "t"));
        assert!(top_level_code("not json").is_none());
        assert!(error_text_parts_of("not json").is_empty());
    }
}
