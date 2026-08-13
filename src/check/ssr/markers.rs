//! Upstream error markers for Grok Build / CLIProxyAPI probe classification.
//!
//! # Upstream snapshot
//! Observed against `cli-chat-proxy.grok.com` + `auth.x.ai` responses as of **2026-08**
//! (grok-4.6 shipped 2026-08-12, builds on grok-4.5; both accepted during the overlap).
//! Matching is intentionally string/code based (same approach as the original
//! Python `check_accounts.py`). When xAI renames error bodies or codes, update
//! the tables below and extend the unit tests in this module.
//!
//! Do not scatter marker strings into probe/classify — keep them here so a
//! single file is the change surface for upstream drift.

use super::http::joined_candidates;

/// Body / code substrings that mean SuperGrok / weekly Build balance is empty.
/// Matched case-insensitively against joined error candidates + raw body.
pub const BUILD_BALANCE_EXHAUSTED_MARKERS: &[&str] = &[
    "build_usage_balance_exhausted",
    "grok build usage balance exhausted",
];

/// Requires both "usage balance exhausted" and "build" in the joined text.
pub const BUILD_BALANCE_EXHAUSTED_PAIR: (&str, &str) = ("usage balance exhausted", "build");

/// HTTP status that alone implies build balance exhausted on cli-chat-proxy.
pub const BUILD_BALANCE_BARE_STATUS: u16 = 402;

/// Paid API / team monthly spending-limit markers.
pub const SPENDING_LIMIT_MARKERS: &[&str] = &[
    "monthly spending limit",
    "used all available credits",
    "personal-team-blocked:spending-limit",
    "spending-limit",
];

/// When present, spending markers must also match (too broad alone).
pub const SPENDING_LIMIT_REQUIRES_COMPANION: &str = "permission-denied";

/// Chat endpoint permanently denied (403 + body).
pub const CHAT_ENDPOINT_DENIED_MARKERS: &[&str] = &[
    "access to the chat endpoint is denied",
    "chat_endpoint_denied",
];

/// Loose "access denied" phrase accepted only under HTTP 403.
pub const CHAT_ENDPOINT_DENIED_ACCESS_DENIED: &str = "access denied";

/// 探针模型无效 / 已下线。命中后由 probe 层降级到下一个候选模型，
/// 而不是把账号误判成账号级错误。
///
/// # 实测（cli-chat-proxy.grok.com，2026-08）
/// 无效模型返回 `400 {"code":"invalid-argument","error":"Model not found: <model>"}`。
/// 顶层 code 是宽泛的 `invalid-argument`（不能单独当模型信号），真正可辨识的是
/// error 文本里的 "Model not found"。故 CODES 覆盖 OpenAI 兼容体系的历史写法，
/// PHRASES 覆盖 xAI 实测文本（"not found"），二者取并集。
pub const MODEL_UNAVAILABLE_CODES: &[&str] = &[
    "model_not_found",
    "model_not_supported",
    "model_not_available",
    "model_does_not_exist",
    "invalid_model",
    "unknown_model",
];

/// 需与 "model" 同现的短语（单独出现太宽，避免误伤账号级 4xx）。
/// "not found" 对应 xAI 实测的 "Model not found: <model>"。
pub const MODEL_UNAVAILABLE_PHRASES: &[&str] = &[
    "not found",
    "does not exist",
    "is not supported",
    "not supported",
    "no such model",
    "unknown model",
    "invalid model",
];

/// SuperGrok / 周度 Build 余额耗尽（对齐 Python is_build_usage_balance_exhausted）
pub fn is_build_usage_balance_exhausted(
    status: u16,
    body: &str,
    parts: &[String],
    code: Option<&str>,
) -> bool {
    let joined = joined_candidates(body, parts, code);
    if BUILD_BALANCE_EXHAUSTED_MARKERS
        .iter()
        .any(|m| joined.contains(m))
    {
        return true;
    }
    let (a, b) = BUILD_BALANCE_EXHAUSTED_PAIR;
    if joined.contains(a) && joined.contains(b) {
        return true;
    }
    // cli-chat-proxy 的裸 402 基本都是该信号
    status == BUILD_BALANCE_BARE_STATUS
}

/// 付费 API / 团队月度消费上限（对齐 Python is_spending_limit_exhausted）
pub fn is_spending_limit_exhausted(body: &str, parts: &[String], code: Option<&str>) -> bool {
    let joined = joined_candidates(body, parts, code);
    if joined.contains(SPENDING_LIMIT_REQUIRES_COMPANION) {
        // permission-denied 本身太宽，必须伴随 spending 标记
        return SPENDING_LIMIT_MARKERS.iter().any(|m| joined.contains(m));
    }
    SPENDING_LIMIT_MARKERS.iter().any(|m| joined.contains(m))
}

/// 探针模型无效 / 已下线（404/400 model_not_found、"model ... does not exist" 等）。
/// 仅在客户端 4xx 命中——5xx 是上游故障，不该触发模型降级。
pub fn is_model_unavailable(status: u16, body: &str, parts: &[String], code: Option<&str>) -> bool {
    if !(400..500).contains(&status) {
        return false;
    }
    let joined = joined_candidates(body, parts, code);
    if MODEL_UNAVAILABLE_CODES.iter().any(|m| joined.contains(m)) {
        return true;
    }
    // 短语必须与 "model" 同现，避免把 "endpoint not found" 之类误判为模型失效
    joined.contains("model") && MODEL_UNAVAILABLE_PHRASES.iter().any(|p| joined.contains(p))
}

pub fn is_chat_endpoint_denied(status: u16, body: &str) -> bool {
    // 403 且 body 表明 chat endpoint 被永久拒绝
    if status != 403 {
        return false;
    }
    let lower = body.to_ascii_lowercase();
    if CHAT_ENDPOINT_DENIED_MARKERS
        .iter()
        .any(|m| lower.contains(m))
    {
        return true;
    }
    for part in body.split(['"', '\'', ',', '{', '}']) {
        let n = part
            .trim()
            .trim_matches(|c: char| matches!(c, '.' | '!' | ' ' | '\t' | '\r' | '\n'));
        if n.eq_ignore_ascii_case(CHAT_ENDPOINT_DENIED_ACCESS_DENIED) {
            return true;
        }
    }
    let normalized = body
        .trim()
        .trim_matches(|c: char| matches!(c, '.' | '!' | ' ' | '\t' | '\r' | '\n'));
    normalized.eq_ignore_ascii_case(CHAT_ENDPOINT_DENIED_ACCESS_DENIED)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::ssr::http::error_text_parts_of;

    #[test]
    fn detects_build_balance_exhausted() {
        // 裸 402（Python：cli-chat-proxy 裸 402 基本都是余额信号）
        assert!(is_build_usage_balance_exhausted(402, "", &[], None));
        // body 文本标记
        let body = r#"{"error":{"message":"Grok Build usage balance exhausted"}}"#;
        let parts = error_text_parts_of(body);
        assert!(is_build_usage_balance_exhausted(429, body, &parts, None));
        // 顶层 code 标记
        let body2 = r#"{"code":"build_usage_balance_exhausted"}"#;
        let top = crate::check::ssr::http::top_level_code(body2);
        assert!(is_build_usage_balance_exhausted(
            400,
            body2,
            &[],
            top.as_deref()
        ));
        // 普通 429 文本不误判
        assert!(!is_build_usage_balance_exhausted(
            429,
            "rate limit reached, slow down",
            &[],
            None
        ));
    }

    #[test]
    fn detects_model_unavailable() {
        // xAI cli-chat-proxy 实测（2026-08）：400 + code invalid-argument + "Model not found: <model>"
        let real =
            r#"{"code":"invalid-argument","error":"Model not found: grok-4.7-does-not-exist"}"#;
        let real_parts = error_text_parts_of(real);
        let real_top = crate::check::ssr::http::top_level_code(real);
        assert!(is_model_unavailable(
            400,
            real,
            &real_parts,
            real_top.as_deref()
        ));
        // 同为 invalid-argument 但与模型无关的参数错误 → 不误判（"model" 未同现）
        let other = r#"{"code":"invalid-argument","error":"max_output_tokens must be positive"}"#;
        let other_parts = error_text_parts_of(other);
        assert!(!is_model_unavailable(
            400,
            other,
            &other_parts,
            Some("invalid-argument")
        ));
        // OpenAI 兼容体系历史写法：顶层 code 内嵌在 error 里，flatten 后由 parts 命中
        let body = r#"{"error":{"message":"The model `grok-4.5` does not exist or you do not have access to it","code":"model_not_found"}}"#;
        let parts = error_text_parts_of(body);
        let top = crate::check::ssr::http::top_level_code(body);
        assert!(is_model_unavailable(404, body, &parts, top.as_deref()));
        // 纯 message（无 code）：model + "does not exist" 同现
        let body2 = r#"{"error":{"message":"The model grok-9 does not exist"}}"#;
        let parts2 = error_text_parts_of(body2);
        assert!(is_model_unavailable(404, body2, &parts2, None));
        // 账号级 4xx 不误判：裸 402 / 限流 / chat denied / spending
        assert!(!is_model_unavailable(402, "", &[], None));
        assert!(!is_model_unavailable(
            429,
            "rate limit reached, slow down",
            &[],
            None
        ));
        assert!(!is_model_unavailable(
            403,
            "access to the chat endpoint is denied",
            &[],
            None
        ));
        assert!(!is_model_unavailable(
            403,
            "monthly spending limit reached",
            &[],
            None
        ));
        // 5xx 上游故障不算模型失效
        assert!(!is_model_unavailable(
            503,
            "the model does not exist",
            &[],
            None
        ));
    }

    #[test]
    fn detects_spending_limit_exhausted() {
        // permission-denied 单独出现太宽，不算
        let body = r#"{"error":{"code":"permission-denied","message":"forbidden"}}"#;
        let parts = error_text_parts_of(body);
        assert!(!is_spending_limit_exhausted(body, &parts, None));
        // permission-denied + spending 标记 → 算
        let body2 =
            r#"{"error":{"code":"permission-denied","message":"monthly spending limit reached"}}"#;
        let parts2 = error_text_parts_of(body2);
        assert!(is_spending_limit_exhausted(body2, &parts2, None));
        // 纯 spending 标记 → 算
        assert!(is_spending_limit_exhausted(
            "personal-team-blocked:spending-limit",
            &[],
            None
        ));
        assert!(is_spending_limit_exhausted(
            "You have used all available credits",
            &[],
            None
        ));
    }
}
