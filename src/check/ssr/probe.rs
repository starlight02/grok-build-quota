//! `/v1/responses` probe: HTTP status + rate-limit headers + error code.

use std::sync::LazyLock;

use super::{
    auth::{AuthRecord, build_headers},
    http::{
        error_text_parts_of, extract_error_message, header_i64, header_string, parse_exhausted,
        top_level_code,
    },
    markers::{
        is_build_usage_balance_exhausted, is_chat_endpoint_denied, is_model_unavailable,
        is_spending_limit_exhausted,
    },
};
use crate::check::QuotaPeriod;

/// 探针模型只是「敲门砖」——发一次请求触发限流头 / 状态码，不依赖模型能力。
/// 默认追新到 grok-4.6，旧模型 grok-4.5 作为过渡期兜底；换代无需改代码，
/// 用环境变量 GBQ_PROBE_MODEL 覆盖（逗号分隔，按序尝试）即可。
const PROBE_MODEL_ENV: &str = "GBQ_PROBE_MODEL";
const DEFAULT_PROBE_MODELS: &[&str] = &["grok-4.6", "grok-4.5"];

fn parse_probe_models(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

static PROBE_MODELS: LazyLock<Vec<String>> = LazyLock::new(|| {
    std::env::var(PROBE_MODEL_ENV)
        .ok()
        .map(|raw| parse_probe_models(&raw))
        .filter(|models| !models.is_empty())
        .unwrap_or_else(|| DEFAULT_PROBE_MODELS.iter().map(|s| s.to_string()).collect())
});

/// 对齐 check_accounts.py summarize_response 的 code 体系
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeCode {
    /// 429 + "tokens (actual/limit): N/M" → subscription:free-usage-exhausted
    FreeUsageExhausted,
    /// 402 或 body 含 build usage balance exhausted
    BuildBalanceExhausted,
    /// 付费 API 月度消费上限 / 团队额度（spending-limit markers）
    SpendingLimitExhausted,
    /// 403 + "access to the chat endpoint is denied"
    ChatEndpointDenied,
    /// 探针模型无效 / 已下线：候选链全部失败后上报，提示更新 GBQ_PROBE_MODEL
    ModelUnavailable,
}

pub struct ProbeOutcome {
    pub status_code: Option<u16>,
    pub quota_period: QuotaPeriod,
    pub remaining_tokens: Option<i64>,
    pub limit_tokens: Option<i64>,
    pub remaining_requests: Option<i64>,
    pub limit_requests: Option<i64>,
    pub detail: Option<String>,
    pub code: Option<ProbeCode>,
    pub network_error: bool,
}

fn parse_header_period(value: Option<String>) -> QuotaPeriod {
    let raw = value.unwrap_or_default().to_ascii_lowercase();
    if raw.contains("day") || raw.contains("daily") {
        QuotaPeriod::Daily
    } else if raw.contains("week") || raw.contains("weekly") {
        QuotaPeriod::Weekly
    } else if raw.contains("month") || raw.contains("monthly") {
        QuotaPeriod::Monthly
    } else if raw.contains("rolling") {
        QuotaPeriod::Rolling
    } else {
        QuotaPeriod::Unknown
    }
}

pub async fn probe_responses(
    client: &reqwest::Client,
    auth: &AuthRecord,
    url: &str,
    cli_version: &str,
) -> Result<ProbeOutcome, String> {
    let models = &*PROBE_MODELS;
    let mut model_unavailable: Option<ProbeOutcome> = None;
    for model in models {
        let outcome = probe_once(client, auth, url, cli_version, model).await?;
        // 只有「模型无效」才换下一个候选；其它结果都是账号级真实信号，直接返回。
        if outcome.code == Some(ProbeCode::ModelUnavailable) {
            model_unavailable = Some(outcome);
            continue;
        }
        return Ok(outcome);
    }
    // 候选链全部失效：给出可执行的运维提示（列出尝试过的模型 + 如何修）。
    let mut outcome = model_unavailable.expect("PROBE_MODELS is never empty");
    outcome.detail = Some(format!(
        "探针模型均不可用（已尝试 {}）；设置环境变量 {} 指向当前有效模型即可恢复",
        models.join("、"),
        PROBE_MODEL_ENV
    ));
    Ok(outcome)
}

async fn probe_once(
    client: &reqwest::Client,
    auth: &AuthRecord,
    url: &str,
    cli_version: &str,
    model: &str,
) -> Result<ProbeOutcome, String> {
    let headers = build_headers(auth, cli_version)?;
    let body = serde_json::json!({
        "model": model,
        "input": "Reply exactly: OK",
        "max_output_tokens": 8,
    });

    let resp = client
        .post(url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status_code = resp.status().as_u16();
    let header_limit = header_i64(resp.headers(), "x-ratelimit-limit-tokens");
    let header_remaining = header_i64(resp.headers(), "x-ratelimit-remaining-tokens");
    let header_req_limit = header_i64(resp.headers(), "x-ratelimit-limit-requests");
    let header_req_remaining = header_i64(resp.headers(), "x-ratelimit-remaining-requests");
    let quota_period = parse_header_period(header_string(
        resp.headers(),
        &[
            "x-ratelimit-period",
            "x-ratelimit-window",
            "x-ratelimit-limit-period",
            "x-quota-period",
        ],
    ));
    let text = resp.text().await.unwrap_or_default();

    let mut remaining_tokens = header_remaining;
    let mut limit_tokens = header_limit;
    let mut detail = None;
    let mut code = None;

    if status_code == 429 {
        // Free 滚动窗口耗尽：body 带 "tokens (actual/limit): N/M"
        if let Some((actual, limit)) = parse_exhausted(&text) {
            limit_tokens = Some(limit);
            remaining_tokens = Some((limit - actual).max(0));
            code = Some(ProbeCode::FreeUsageExhausted);
            detail = Some(format!("免费额度已用尽（{actual}/{limit}）"));
        } else {
            // 其余 429 仍可能是付费 balance / spending 信号（Python 在 verdict 补判）
            let parts = error_text_parts_of(&text);
            let top = top_level_code(&text);
            if is_build_usage_balance_exhausted(status_code, &text, &parts, top.as_deref()) {
                code = Some(ProbeCode::BuildBalanceExhausted);
            } else if is_spending_limit_exhausted(&text, &parts, top.as_deref()) {
                code = Some(ProbeCode::SpendingLimitExhausted);
            }
            if !text.is_empty() {
                detail = Some(extract_error_message(&text));
            }
        }
    } else if status_code >= 400 {
        let msg = extract_error_message(&text);
        let parts = error_text_parts_of(&text);
        let top = top_level_code(&text);
        // 模型失效优先：命中说明本次没测到账号真实状态，交给降级链换模型重试。
        if is_model_unavailable(status_code, &text, &parts, top.as_deref()) {
            code = Some(ProbeCode::ModelUnavailable);
            detail = Some(format!("探针模型 `{model}` 不可用：{msg}"));
        } else {
            code = if is_chat_endpoint_denied(status_code, &text) {
                Some(ProbeCode::ChatEndpointDenied)
            } else if is_build_usage_balance_exhausted(status_code, &text, &parts, top.as_deref()) {
                // Python：402 无 ratelimit 头，余额耗尽时 remaining 记 0
                if remaining_tokens.is_none() {
                    remaining_tokens = Some(0);
                }
                Some(ProbeCode::BuildBalanceExhausted)
            } else if is_spending_limit_exhausted(&text, &parts, top.as_deref()) {
                Some(ProbeCode::SpendingLimitExhausted)
            } else {
                None
            };
            detail = Some(msg);
        }
    }

    Ok(ProbeOutcome {
        status_code: Some(status_code),
        quota_period,
        remaining_tokens,
        limit_tokens,
        remaining_requests: header_req_remaining,
        limit_requests: header_req_limit,
        detail,
        code,
        network_error: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_probe_models_env_override() {
        assert_eq!(
            parse_probe_models("grok-4.7, grok-4.6 , grok-4.5"),
            vec![
                "grok-4.7".to_string(),
                "grok-4.6".to_string(),
                "grok-4.5".to_string(),
            ]
        );
        assert_eq!(parse_probe_models("grok-4.6"), vec!["grok-4.6".to_string()]);
        // 全空 / 纯分隔符 → 空表（LazyLock 会回退到默认候选）
        assert!(parse_probe_models("  ,  , ").is_empty());
        assert!(parse_probe_models("").is_empty());
    }

    #[test]
    fn parses_rate_limit_period_headers() {
        assert_eq!(
            parse_header_period(Some("1 day".into())),
            QuotaPeriod::Daily
        );
        assert_eq!(
            parse_header_period(Some("weekly".into())),
            QuotaPeriod::Weekly
        );
        assert_eq!(
            parse_header_period(Some("calendar-month".into())),
            QuotaPeriod::Monthly
        );
        assert_eq!(
            parse_header_period(Some("rolling-7d".into())),
            QuotaPeriod::Rolling
        );
        assert_eq!(
            parse_header_period(Some("3600".into())),
            QuotaPeriod::Unknown
        );
        assert_eq!(parse_header_period(None), QuotaPeriod::Unknown);
    }
}
