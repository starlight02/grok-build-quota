use super::*;

const DEFAULT_BASE_URL: &str = "https://cli-chat-proxy.grok.com/v1";
const PROBE_MODEL: &str = "grok-4.5";
const DEFAULT_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const DEFAULT_TOKEN_ENDPOINT: &str = "https://auth.x.ai/oauth2/token";
// chat endpoint denied is detected by body text, not a fixed label constant

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

#[derive(Debug, Clone)]
struct AuthRecord {
    email: String,
    access_token: String,
    refresh_token: String,
    client_id: String,
    token_endpoint: String,
    base_url: String,
    headers: HashMap<String, String>,
    disabled: bool,
    /// 原始 JSON，用于合并 refresh 后回传浏览器
    raw: Value,
}

/// 对齐 check_accounts.py summarize_response 的 code 体系
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProbeCode {
    /// 429 + "tokens (actual/limit): N/M" → subscription:free-usage-exhausted
    FreeUsageExhausted,
    /// 402 或 body 含 build usage balance exhausted
    BuildBalanceExhausted,
    /// 付费 API 月度消费上限 / 团队额度（spending-limit markers）
    SpendingLimitExhausted,
    /// 403 + "access to the chat endpoint is denied"
    ChatEndpointDenied,
}

struct ProbeOutcome {
    status_code: Option<u16>,
    remaining_tokens: Option<i64>,
    limit_tokens: Option<i64>,
    remaining_requests: Option<i64>,
    limit_requests: Option<i64>,
    detail: Option<String>,
    code: Option<ProbeCode>,
    network_error: bool,
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

    if let Ok(proxy) = std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("HTTP_PROXY"))
        && let Ok(p) = reqwest::Proxy::all(proxy)
    {
        builder = builder.proxy(p);
    }

    builder.build().unwrap_or_else(|_| reqwest::Client::new())
}

struct ResultContext {
    account: String,
    filename: String,
}

impl ResultContext {
    fn new(account: String, filename: String) -> Self {
        Self { account, filename }
    }

    #[allow(clippy::too_many_arguments)]
    fn make(
        &self,
        status: AccountStatus,
        quota: impl Into<String>,
        usable: bool,
        remaining_tokens: Option<i64>,
        limit_tokens: Option<i64>,
        remaining_requests: Option<i64>,
        limit_requests: Option<i64>,
        http_status: Option<u16>,
        detail: Option<String>,
        refreshed: bool,
        updated_content: Option<String>,
    ) -> CheckResult {
        CheckResult::make(
            self.account.clone(),
            self.filename.clone(),
            status,
            quota,
            usable,
            remaining_tokens,
            limit_tokens,
            remaining_requests,
            limit_requests,
            http_status,
            detail,
            refreshed,
            updated_content,
        )
    }

    fn empty(
        &self,
        status: AccountStatus,
        detail: Option<String>,
        refreshed: bool,
        updated_content: Option<String>,
    ) -> CheckResult {
        self.make(
            status,
            "--",
            false,
            None,
            None,
            None,
            None,
            None,
            detail,
            refreshed,
            updated_content,
        )
    }

    fn empty_with_plan(
        &self,
        status: AccountStatus,
        detail: Option<String>,
        refreshed: bool,
        updated_content: Option<String>,
        plan: AccountPlan,
    ) -> CheckResult {
        self.empty(status, detail, refreshed, updated_content)
            .with_plan(plan)
    }
}

/// allow_refresh：是否自动用 refresh_token 换新（开关关闭时 401/过期只报 Token 过期）
pub async fn check_one(
    client: &reqwest::Client,
    file: AuthUpload,
    allow_refresh: bool,
) -> CheckResult {
    let filename = file.filename.clone();
    let mut auth = match resolve_auth_record(&file) {
        Ok(v) => v,
        Err(err) => {
            return ResultContext::new(filename.clone(), filename).empty(
                AccountStatus::Invalid,
                Some(err),
                false,
                None,
            );
        }
    };

    let account = if auth.email.trim().is_empty() {
        filename.clone()
    } else {
        auth.email.trim().to_string()
    };
    let result_context = ResultContext::new(account, filename);

    if auth.disabled {
        return result_context.empty(
            AccountStatus::Disabled,
            Some("账号已禁用".into()),
            false,
            None,
        );
    }

    let mut did_refresh = false;
    let mut refresh_notes: Vec<String> = Vec::new();
    let mut updated_content: Option<String> = None;

    // 缺 token / JWT 过期：自动刷新关闭时不静默换新，报 Token 过期等手动刷新
    let need_refresh_now = auth.access_token.trim().is_empty() || jwt_expired(&auth.access_token);
    if need_refresh_now && !allow_refresh && !auth.refresh_token.trim().is_empty() {
        return result_context.empty(
            AccountStatus::Expired,
            Some(
                "access_token 已过期/缺失，未刷新（自动刷新已关闭）；点「刷新 Token」手动更新"
                    .into(),
            ),
            false,
            None,
        );
    }
    if need_refresh_now {
        match try_refresh(client, &mut auth).await {
            Ok(note) => {
                did_refresh = true;
                refresh_notes.push("已自动刷新 access_token".into());
                if let Some(n) = note {
                    refresh_notes.push(n);
                }
                updated_content = serialize_auth(&auth);
            }
            Err(err) => {
                let is_net = err.to_ascii_lowercase().contains("network");
                let human = humanize_refresh_err(&err);
                if auth.access_token.trim().is_empty() {
                    return result_context.empty(
                        if is_net {
                            AccountStatus::NetworkError
                        } else {
                            AccountStatus::Invalid
                        },
                        Some(format!("缺少 access_token，且刷新失败：{human}")),
                        false,
                        None,
                    );
                }
                // 过期且刷新失败：网络 → 网络错误；吊销/无效 → 刷新失败
                if jwt_expired(&auth.access_token) {
                    return result_context.empty(
                        if is_net {
                            AccountStatus::NetworkError
                        } else {
                            AccountStatus::RefreshFailed
                        },
                        Some(human),
                        false,
                        None,
                    );
                }
                refresh_notes.push(format!("刷新失败：{human}"));
            }
        }
    }

    if auth.access_token.trim().is_empty() {
        return result_context.empty(
            AccountStatus::Invalid,
            Some(join_notes("缺少 access_token", &refresh_notes)),
            did_refresh,
            updated_content,
        );
    }

    let base_url = normalize_base_url(&auth.base_url);
    let responses_url = format!("{}/responses", base_url.trim_end_matches('/'));

    // 账号类型：settings + user + JWT tier（驱动 Free/付费检测分流）
    let jwt_tier = jwt_claim_tier(&auth.access_token);
    let mut info = fetch_plan(client, &auth, &base_url, jwt_tier).await;
    let mut plan = info.plan;

    let mut probe = match probe_responses(client, &auth, &responses_url).await {
        Ok(p) => p,
        Err(err) => {
            return result_context.empty_with_plan(
                AccountStatus::NetworkError,
                Some(join_notes(&format!("网络错误：{err}"), &refresh_notes)),
                false,
                updated_content,
                plan,
            );
        }
    };

    // 401 且尚未 refresh 且有 refresh_token → 刷新后重探；自动刷新关闭时报 Token 过期
    if probe.status_code == Some(401) && !did_refresh && !auth.refresh_token.trim().is_empty() {
        if !allow_refresh {
            return result_context.empty_with_plan(
                AccountStatus::Expired,
                Some(
                    "鉴权被拒（HTTP 401），token 未刷新；点「刷新 Token」或开启「自动刷新 Token」"
                        .into(),
                ),
                false,
                None,
                plan,
            );
        }
        match try_refresh(client, &mut auth).await {
            Ok(note) => {
                did_refresh = true;
                refresh_notes.push("探测返回 401 后已自动刷新".into());
                if let Some(n) = note {
                    refresh_notes.push(n);
                }
                updated_content = serialize_auth(&auth);
                info =
                    fetch_plan(client, &auth, &base_url, jwt_claim_tier(&auth.access_token)).await;
                plan = info.plan;
                match probe_responses(client, &auth, &responses_url).await {
                    Ok(p) => probe = p,
                    Err(err) => {
                        return result_context.empty_with_plan(
                            AccountStatus::NetworkError,
                            Some(join_notes(&format!("网络错误：{err}"), &refresh_notes)),
                            false,
                            updated_content,
                            plan,
                        );
                    }
                }
            }
            Err(err) => {
                let is_net = err.to_ascii_lowercase().contains("network");
                let human = humanize_refresh_err(&err);
                if is_net {
                    return result_context.empty_with_plan(
                        AccountStatus::NetworkError,
                        Some(join_notes(&human, &refresh_notes)),
                        false,
                        None,
                        plan,
                    );
                }
                refresh_notes.push(format!("探测 401 后刷新失败：{human}"));
            }
        }
    }

    // 分流（对齐 check_accounts.py probe_strategy_for_plan）：
    // Free → responses ratelimit 权威，不碰 billing；付费 → billing productUsage 主探
    let mut remaining_tokens = probe.remaining_tokens;
    let mut limit_tokens = probe.limit_tokens;
    let mut usage_percent: Option<f64> = None;
    let mut quota = format_quota(remaining_tokens, limit_tokens);
    let mut billing_note: Option<String> = None;

    let want_billing = plan != AccountPlan::Free && probe.status_code != Some(401);
    if want_billing {
        if let Some(billing) = fetch_billing(client, &auth, &base_url).await {
            // billing 细化类型时保留 settings/user 标签（Lite/Heavy 消歧）
            let refined = classify_plan(
                info.tier_display.as_deref(),
                info.subscription_tiers.as_deref(),
                jwt_claim_tier(&auth.access_token).as_deref(),
                billing.has_product_usage,
                billing.credit_usage_percent,
                billing.monthly_limit_cents,
            );
            if refined != AccountPlan::Free && refined != AccountPlan::Unknown {
                plan = refined;
            }
            // 周限额总量优先（CPA 周限额条）：402「usage balance exhausted」由总量
            // 触发，GrokBuild 分项可能只有 2%，取分项会严重误报剩余
            if let Some(pct) = billing.weekly_percent() {
                usage_percent = Some(pct);
                // 用百分比反推展示：剩余% / 100%
                let rem_pct = (100.0 - pct).clamp(0.0, 100.0);
                remaining_tokens = Some(rem_pct.round() as i64);
                limit_tokens = Some(100);
                quota = format!("周 {:.0}% 已用", pct);
                billing_note = billing.breakdown_note();
            } else if let Some((pct, monthly_quota)) =
                monthly_quota_display(billing.monthly_used_cents, billing.monthly_limit_cents)
            {
                // 月 included 美元额度：仅作周数据缺失时的 fallback；
                // 不塞进 remaining_tokens（那是 token/百分比语义，不是美元）
                usage_percent = Some(pct);
                remaining_tokens = None;
                limit_tokens = None;
                quota = monthly_quota;
                billing_note = Some("月度套餐 included 额度，非 Build 周池".into());
            }
        }
    }

    let status_code = probe.status_code;

    // Free 额度是每日 token 窗口：标明「日」（付费周/月已在 billing 分支标注）
    if plan == AccountPlan::Free && (remaining_tokens.is_some() || limit_tokens.is_some()) {
        quota = format!("日 {quota}");
    }

    // 402 / Build balance 耗尽 = 周池已空；billing 延迟可能仍显示余量，
    // 强制 100% 已用，避免状态「耗尽」与额度条「还剩 X%」拧巴
    if balance_exhausted_probe(status_code, probe.code) {
        usage_percent = Some(100.0);
        remaining_tokens = Some(0);
        limit_tokens = Some(100);
        quota = "周 100% 已用".into();
    }

    // 判定表对齐 check_accounts.py verdict：状态码 + probe code 驱动；
    // billing 百分比只反映用量条，绝不把 200 反向降级为耗尽
    let (status, usable, mut detail) = classify_probe(
        status_code,
        probe.code,
        &plan,
        probe.detail,
        probe.network_error,
        usage_percent,
    );

    if let Some(d) = detail.take() {
        detail = Some(join_notes(&d, &refresh_notes));
    } else if !refresh_notes.is_empty() {
        detail = Some(refresh_notes.join(" · "));
    }

    // 周用量分项（Api/Build/Chat）拼进 detail，解释总量构成
    if let Some(note) = billing_note {
        detail = Some(match detail {
            Some(d) => format!("{d} · {note}"),
            None => note,
        });
    }

    // JWT 仍过期且未 usable → Expired 优先于其它
    let (status, usable) = if !usable && jwt_expired(&auth.access_token) && status_code != Some(200)
    {
        (AccountStatus::Expired, false)
    } else {
        (status, usable)
    };
    // 「已刷新」只在本轮探测最终可用时展示；失败/网络错误即使 token 已换也不标绿
    let show_refreshed = did_refresh && usable;
    let mut result = result_context
        .make(
            status,
            quota,
            usable,
            remaining_tokens,
            limit_tokens,
            probe.remaining_requests,
            probe.limit_requests,
            status_code,
            detail,
            show_refreshed,
            updated_content,
        )
        .with_plan(plan);
    result.usage_percent = usage_percent;
    result
}

fn classify_probe(
    status_code: Option<u16>,
    probe_code: Option<ProbeCode>,
    plan: &AccountPlan,
    detail: Option<String>,
    network_error: bool,
    usage_percent: Option<f64>,
) -> (AccountStatus, bool, Option<String>) {
    match status_code {
        Some(200) => {
            let note = if usage_percent.map(|p| p >= 99.5).unwrap_or(false) {
                Some("周限额已打满，探测仍可用（窗口重置前可能随时被拒）".into())
            } else {
                None
            };
            (AccountStatus::Ok, true, note)
        }
        Some(429) => match probe_code {
            Some(ProbeCode::FreeUsageExhausted) => (
                AccountStatus::Exhausted,
                false,
                Some(detail.unwrap_or_else(|| "免费额度已用尽".into())),
            ),
            Some(ProbeCode::BuildBalanceExhausted) => (
                AccountStatus::Exhausted,
                false,
                Some("Build 余额不足".into()),
            ),
            // 付费周额度走 billing，429 基本是瞬时限流而非额度归零
            _ if plan.is_paid() => (
                AccountStatus::RateLimited,
                false,
                Some(detail.unwrap_or_else(|| "触发限流（HTTP 429），额度未耗尽".into())),
            ),
            _ => (
                AccountStatus::Exhausted,
                false,
                Some(detail.unwrap_or_else(|| "额度耗尽或触发限流".into())),
            ),
        },
        Some(402) => (
            AccountStatus::Exhausted,
            false,
            Some("Build 余额不足".into()),
        ),
        Some(403) => match probe_code {
            Some(ProbeCode::ChatEndpointDenied) => (
                AccountStatus::ChatDenied,
                false,
                Some("上游永久拒绝 chat 访问".into()),
            ),
            Some(ProbeCode::SpendingLimitExhausted) => (
                AccountStatus::SpendingLimited,
                false,
                Some("已达月度消费上限或团队额度".into()),
            ),
            _ => (
                AccountStatus::AuthFailed,
                false,
                Some("鉴权被拒（HTTP 403）".into()),
            ),
        },
        Some(401) => (
            AccountStatus::AuthFailed,
            false,
            Some("鉴权被拒（HTTP 401）".into()),
        ),
        // 其它状态兜底 balance / spending 文本（Python verdict 同款）
        Some(http_code) => match probe_code {
            Some(ProbeCode::BuildBalanceExhausted) => (
                AccountStatus::Exhausted,
                false,
                Some("Build 余额不足".into()),
            ),
            Some(ProbeCode::SpendingLimitExhausted) => (
                AccountStatus::SpendingLimited,
                false,
                Some("已达月度消费上限或团队额度".into()),
            ),
            _ => (
                AccountStatus::Error,
                false,
                Some(
                    detail
                        .map(|d| format!("HTTP {http_code}：{d}"))
                        .unwrap_or_else(|| format!("HTTP {http_code}")),
                ),
            ),
        },
        None if network_error => (
            AccountStatus::NetworkError,
            false,
            detail
                .map(|d| format!("网络错误：{d}"))
                .or_else(|| Some("网络错误".into())),
        ),
        None => (
            AccountStatus::Error,
            false,
            detail.or_else(|| Some("探测失败".into())),
        ),
    }
}

#[derive(Default)]
struct BillingUsage {
    build_usage_percent: Option<f64>,
    credit_usage_percent: Option<f64>,
    api_usage_percent: Option<f64>,
    chat_usage_percent: Option<f64>,
    monthly_limit_cents: Option<f64>,
    monthly_used_cents: Option<f64>,
    has_product_usage: bool,
}

impl BillingUsage {
    /// 周限额总量（CPA「周限额」条）：402/耗尽由它决定，GrokBuild 分项只是参考
    fn weekly_percent(&self) -> Option<f64> {
        self.credit_usage_percent.or(self.build_usage_percent)
    }

    /// 周用量分项（Api/Build/Chat），有内容时进 detail 解释总量构成
    fn breakdown_note(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(p) = self.api_usage_percent {
            parts.push(format!("Api {p:.0}%"));
        }
        if let Some(p) = self.build_usage_percent {
            parts.push(format!("Build {p:.0}%"));
        }
        if let Some(p) = self.chat_usage_percent {
            parts.push(format!("Chat {p:.0}%"));
        }
        if parts.is_empty() {
            None
        } else {
            Some(format!("周用量分解：{}", parts.join(" · ")))
        }
    }
    fn merge_credit(&mut self, other: Self) {
        self.credit_usage_percent = other.credit_usage_percent;
        self.build_usage_percent = other.build_usage_percent;
        self.api_usage_percent = other.api_usage_percent;
        self.chat_usage_percent = other.chat_usage_percent;
        self.monthly_limit_cents = other.monthly_limit_cents.or(self.monthly_limit_cents);
        self.monthly_used_cents = other.monthly_used_cents.or(self.monthly_used_cents);
        self.has_product_usage = other.has_product_usage;
    }

    fn merge_monthly(&mut self, other: Self) {
        self.monthly_limit_cents = other.monthly_limit_cents.or(self.monthly_limit_cents);
        self.monthly_used_cents = other.monthly_used_cents.or(self.monthly_used_cents);
    }
}

const MONTHLY_LIMIT_SUPERGROK: f64 = 15_000.0;
const MONTHLY_LIMIT_SUPERGROK_HEAVY: f64 = 150_000.0;

fn norm_label(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-')
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn classify_plan(
    tier_display: Option<&str>,
    subscription_tiers: Option<&str>,
    jwt_tier: Option<&str>,
    has_product_usage: bool,
    credit_usage_percent: Option<f64>,
    monthly_limit: Option<f64>,
) -> AccountPlan {
    if let Some(limit) = monthly_limit {
        if limit > 0.0 {
            if (limit - MONTHLY_LIMIT_SUPERGROK_HEAVY).abs() < 0.5 {
                return AccountPlan::SuperGrokHeavy;
            }
            if (limit - MONTHLY_LIMIT_SUPERGROK).abs() < 0.5 {
                let display = tier_display.map(norm_label).unwrap_or_default();
                if display.contains("lite") {
                    return AccountPlan::SuperGrokLite;
                }
                return AccountPlan::SuperGrok;
            }
            return AccountPlan::PaidOther;
        }
    }

    let display = tier_display.map(norm_label).unwrap_or_default();
    let tiers = subscription_tiers.map(norm_label).unwrap_or_default();

    if !display.is_empty() {
        if matches!(display.as_str(), "free" | "grokfree" | "none" | "null") {
            return AccountPlan::Free;
        }
        if display.contains("lite") {
            return AccountPlan::SuperGrokLite;
        }
        if display.contains("heavy") {
            return AccountPlan::SuperGrokHeavy;
        }
        if display.contains("supergrok") || display.contains("grokpro") || display == "pro" {
            return AccountPlan::SuperGrok;
        }
        return AccountPlan::PaidOther;
    }

    if !tiers.is_empty() {
        if tiers.contains("heavy") {
            return AccountPlan::SuperGrokHeavy;
        }
        if tiers.contains("lite") {
            return AccountPlan::SuperGrokLite;
        }
        if tiers.contains("supergrok") || tiers.contains("grokpro") || tiers.contains("pro") {
            return AccountPlan::SuperGrok;
        }
        if !matches!(tiers.as_str(), "free" | "none" | "null") {
            return AccountPlan::PaidOther;
        }
    }

    if has_product_usage {
        return AccountPlan::SuperGrok;
    }
    if credit_usage_percent.is_some() {
        return AccountPlan::PaidOther;
    }
    if let Some(t) = jwt_tier {
        let t = t.trim();
        if !t.is_empty() && t != "0" && !t.eq_ignore_ascii_case("none") {
            return AccountPlan::PaidOther;
        }
    }
    AccountPlan::Free
}

fn jwt_claim_tier(token: &str) -> Option<String> {
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

fn billing_val(v: &Value) -> Option<f64> {
    if let Some(n) = v.as_f64() {
        return Some(n);
    }
    if let Some(n) = v.as_i64() {
        return Some(n as f64);
    }
    if let Some(n) = v.as_u64() {
        return Some(n as f64);
    }
    if let Some(obj) = v.as_object() {
        if let Some(inner) = obj.get("val") {
            return billing_val(inner);
        }
    }
    None
}

/// billing 解析结果：周限额总量 + Api/Build/Chat 分项 + 月度美分额度
fn parse_billing_payload(data: &Value) -> BillingUsage {
    let cfg = data.get("config").filter(|c| c.is_object()).unwrap_or(data);
    let mut out = BillingUsage {
        credit_usage_percent: cfg.get("creditUsagePercent").and_then(billing_val),
        ..Default::default()
    };
    if let Some(arr) = cfg.get("productUsage").and_then(|v| v.as_array()) {
        for item in arr {
            let Some(obj) = item.as_object() else {
                continue;
            };
            let name = obj
                .get("product")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_lowercase()
                .replace(' ', "");
            let pct = obj.get("usagePercent").and_then(billing_val);
            if pct.is_some() {
                out.has_product_usage = true;
            }
            match name.as_str() {
                "grokbuild" | "build" => out.build_usage_percent = pct,
                "api" | "xaiapi" => out.api_usage_percent = pct,
                "grokchat" | "chat" => out.chat_usage_percent = pct,
                _ => {}
            }
        }
    }
    out.monthly_limit_cents = cfg
        .get("monthlyLimit")
        .or_else(|| cfg.get("monthly_limit"))
        .and_then(billing_val);
    out.monthly_used_cents = cfg.get("used").and_then(billing_val);
    out
}

async fn get_json(client: &reqwest::Client, url: String, headers: HeaderMap) -> Option<Value> {
    let response = client.get(url).headers(headers).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json().await.ok()
}

fn json_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// 类型探测结果：plan + 原始标签（billing 细化时保留以消歧 Lite/Heavy）
struct PlanInfo {
    plan: AccountPlan,
    tier_display: Option<String>,
    subscription_tiers: Option<String>,
}

async fn fetch_plan(
    client: &reqwest::Client,
    auth: &AuthRecord,
    base_url: &str,
    jwt_tier: Option<String>,
) -> PlanInfo {
    let Ok(headers) = build_headers(auth) else {
        return PlanInfo {
            plan: AccountPlan::Unknown,
            tier_display: None,
            subscription_tiers: None,
        };
    };
    let base = base_url.trim_end_matches('/');
    let mut tier_display = None;
    let mut subscription_tiers = None;

    if let Some(value) = get_json(client, format!("{base}/settings"), headers.clone()).await {
        tier_display = json_string(&value, "subscription_tier_display");
    }

    if let Some(value) =
        get_json(client, format!("{base}/user?include=subscription"), headers).await
    {
        subscription_tiers = json_string(&value, "subscriptionTier");
    }

    let plan = classify_plan(
        tier_display.as_deref(),
        subscription_tiers.as_deref(),
        jwt_tier.as_deref(),
        false,
        None,
        None,
    );
    PlanInfo {
        plan,
        tier_display,
        subscription_tiers,
    }
}

async fn fetch_billing(
    client: &reqwest::Client,
    auth: &AuthRecord,
    base_url: &str,
) -> Option<BillingUsage> {
    let headers = build_headers(auth).ok()?;
    let base = base_url.trim_end_matches('/');

    let mut out = BillingUsage::default();

    // 周额度：/billing?format=credits 提供总量和产品分项。
    if let Some(value) = get_json(
        client,
        format!("{base}/billing?format=credits"),
        headers.clone(),
    )
    .await
    {
        out.merge_credit(parse_billing_payload(&value));
    }

    // 月度额度：/billing 提供 included 美分额度。
    if let Some(value) = get_json(client, format!("{base}/billing"), headers).await {
        out.merge_monthly(parse_billing_payload(&value));
    }

    Some(out)
}

async fn probe_responses(
    client: &reqwest::Client,
    auth: &AuthRecord,
    url: &str,
) -> Result<ProbeOutcome, String> {
    let headers = build_headers(auth)?;
    let body = serde_json::json!({
        "model": PROBE_MODEL,
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

    Ok(ProbeOutcome {
        status_code: Some(status_code),
        remaining_tokens,
        limit_tokens,
        remaining_requests: header_req_remaining,
        limit_requests: header_req_limit,
        detail,
        code,
        network_error: false,
    })
}

async fn try_refresh(
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

fn humanize_refresh_err(err: &str) -> String {
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

fn serialize_auth(auth: &AuthRecord) -> Option<String> {
    serde_json::to_string_pretty(&auth.raw).ok()
}

fn join_notes(primary: &str, notes: &[String]) -> String {
    if notes.is_empty() {
        primary.to_string()
    } else {
        format!("{primary} · {}", notes.join(" · "))
    }
}

fn resolve_auth_record(file: &AuthUpload) -> Result<AuthRecord, String> {
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
    if let Ok(v) = serde_json::from_str::<Value>(body)
        && let Some(err) = v.get("error")
    {
        return err.to_string().chars().take(180).collect();
    }
    body.chars().take(180).collect()
}

/// 展平嵌套 error JSON 为可匹配字符串（对齐 Python _error_text_parts）
fn flatten_error_parts(v: &Value) -> Vec<String> {
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

fn error_text_parts_of(body: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    v.get("error").map(flatten_error_parts).unwrap_or_default()
}

/// 顶层 "code" 字段（Python upstream_code）
fn top_level_code(body: &str) -> Option<String> {
    let v = serde_json::from_str::<Value>(body).ok()?;
    v.get("code")?
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn joined_candidates(body: &str, parts: &[String], code: Option<&str>) -> String {
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

/// SuperGrok / 周度 Build 余额耗尽（对齐 Python is_build_usage_balance_exhausted）
fn is_build_usage_balance_exhausted(
    status: u16,
    body: &str,
    parts: &[String],
    code: Option<&str>,
) -> bool {
    let joined = joined_candidates(body, parts, code);
    if joined.contains("build_usage_balance_exhausted")
        || joined.contains("grok build usage balance exhausted")
    {
        return true;
    }
    if joined.contains("usage balance exhausted") && joined.contains("build") {
        return true;
    }
    // cli-chat-proxy 的裸 402 基本都是该信号
    status == 402
}

/// 付费 API / 团队月度消费上限（对齐 Python is_spending_limit_exhausted）
fn is_spending_limit_exhausted(body: &str, parts: &[String], code: Option<&str>) -> bool {
    const MARKERS: [&str; 4] = [
        "monthly spending limit",
        "used all available credits",
        "personal-team-blocked:spending-limit",
        "spending-limit",
    ];
    let joined = joined_candidates(body, parts, code);
    if joined.contains("permission-denied") {
        // permission-denied 本身太宽，必须伴随 spending 标记
        return MARKERS.iter().any(|m| joined.contains(m));
    }
    MARKERS.iter().any(|m| joined.contains(m))
}

fn is_chat_endpoint_denied(status: u16, body: &str) -> bool {
    // 403 且 body 表明 chat endpoint 被永久拒绝
    if status != 403 {
        return false;
    }
    let lower = body.to_ascii_lowercase();
    if lower.contains("access to the chat endpoint is denied")
        || lower.contains("chat_endpoint_denied")
    {
        return true;
    }
    for part in body.split(['"', '\'', ',', '{', '}']) {
        let n = part
            .trim()
            .trim_matches(|c: char| matches!(c, '.' | '!' | ' ' | '\t' | '\r' | '\n'));
        if n.eq_ignore_ascii_case("access denied") {
            return true;
        }
    }
    let normalized = body
        .trim()
        .trim_matches(|c: char| matches!(c, '.' | '!' | ' ' | '\t' | '\r' | '\n'));
    normalized.eq_ignore_ascii_case("access denied")
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

/// probe 是否判定 Build 周池耗尽（402 或 balance markers）
fn balance_exhausted_probe(status_code: Option<u16>, code: Option<ProbeCode>) -> bool {
    status_code == Some(402) || code == Some(ProbeCode::BuildBalanceExhausted)
}

/// 月 included 美元额度展示：(已用%, "月 $剩余 / $上限")，保留美分（对齐 CPA）
fn monthly_quota_display(
    used_cents: Option<f64>,
    limit_cents: Option<f64>,
) -> Option<(f64, String)> {
    let (Some(used), Some(limit)) = (used_cents, limit_cents) else {
        return None;
    };
    if limit <= 0.0 {
        return None;
    }
    let pct = (used / limit * 100.0).clamp(0.0, 100.0);
    let quota = format!("月 ${:.2} / ${:.2}", (limit - used) / 100.0, limit / 100.0);
    Some((pct, quota))
}

fn format_quota(remaining: Option<i64>, limit: Option<i64>) -> String {
    match (remaining, limit) {
        (Some(r), Some(l)) => format!("{r} / {l}"),
        (Some(r), None) => format!("{r} / --"),
        (None, Some(l)) => format!("-- / {l}"),
        _ => "--".into(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

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
        let top = top_level_code(body2);
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

    #[test]
    fn classify_probe_keeps_rate_limit_distinct() {
        let (status, usable, detail) = classify_probe(
            Some(429),
            None,
            &AccountPlan::SuperGrok,
            Some("上游限流".into()),
            false,
            Some(2.0),
        );
        assert_eq!(status, AccountStatus::RateLimited);
        assert!(!usable);
        assert_eq!(detail.as_deref(), Some("上游限流"));

        let (status, usable, detail) =
            classify_probe(Some(402), None, &AccountPlan::SuperGrok, None, false, None);
        assert_eq!(status, AccountStatus::Exhausted);
        assert!(!usable);
        assert_eq!(detail.as_deref(), Some("Build 余额不足"));
    }

    #[test]
    fn weekly_percent_prefers_credit_total() {
        // 星光实际案例：周总量 100% 耗尽（402），GrokBuild 分项只有 2%
        let payload = serde_json::json!({
            "config": {
                "creditUsagePercent": 100.0,
                "productUsage": [
                    {"product": "Api", "usagePercent": 96.0},
                    {"product": "GrokBuild", "usagePercent": 2.0},
                    {"product": "GrokChat", "usagePercent": 2.0}
                ],
                "currentPeriod": {"type": "USAGE_PERIOD_TYPE_WEEKLY"}
            }
        });
        let p = parse_billing_payload(&payload);
        assert_eq!(p.credit_usage_percent, Some(100.0));
        assert_eq!(p.api_usage_percent, Some(96.0));
        assert_eq!(p.build_usage_percent, Some(2.0));
        assert_eq!(p.chat_usage_percent, Some(2.0));
        assert!(p.has_product_usage);

        let usage = p;
        // 展示必须取周总量 100%，不能取 GrokBuild 分项 2%
        assert_eq!(usage.weekly_percent(), Some(100.0));
        let note = usage.breakdown_note().expect("breakdown note");
        assert!(note.contains("Api 96%"));
        assert!(note.contains("Build 2%"));
        assert!(note.contains("Chat 2%"));

        // 无总量时先兜 Build 分项
        let only_build = BillingUsage {
            build_usage_percent: Some(42.0),
            ..Default::default()
        };
        assert_eq!(only_build.weekly_percent(), Some(42.0));
    }

    #[test]
    fn balance_exhausted_forces_full_quota() {
        // 402 或 balance markers → 判定周池已空
        assert!(balance_exhausted_probe(Some(402), None));
        assert!(balance_exhausted_probe(
            Some(429),
            Some(ProbeCode::BuildBalanceExhausted)
        ));
        // 付费裸 429（限流）与正常 200 不触发
        assert!(!balance_exhausted_probe(Some(429), None));
        assert!(!balance_exhausted_probe(Some(200), None));
        assert!(!balance_exhausted_probe(
            Some(429),
            Some(ProbeCode::FreeUsageExhausted)
        ));
    }

    #[test]
    fn monthly_quota_keeps_cents_and_labels_month() {
        // CPA 案例：10700/15000 美分 → 月 $43.00 / $150.00
        let (pct, quota) = monthly_quota_display(Some(10700.0), Some(15000.0)).unwrap();
        assert!((pct - 71.333).abs() < 0.01);
        assert_eq!(quota, "月 $43.00 / $150.00");
        // 亚美元精度不丢：$43.50 不再被抹成 $44
        let (_, quota2) = monthly_quota_display(Some(10650.0), Some(15000.0)).unwrap();
        assert_eq!(quota2, "月 $43.50 / $150.00");
        // 无数据 / 0 上限 → None
        assert!(monthly_quota_display(None, Some(15000.0)).is_none());
        assert!(monthly_quota_display(Some(1.0), Some(0.0)).is_none());
    }

    #[test]
    fn refresh_and_probe_sample_auth() {
        // 需要真实 auth JSON：GBQ_SAMPLE=/path/to/auth.json cargo test --features ssr -- --nocapture
        let Some(path) = std::env::var("GBQ_SAMPLE").ok().map(PathBuf::from) else {
            eprintln!("skip: set GBQ_SAMPLE to a sample auth JSON path");
            return;
        };
        let content = std::fs::read_to_string(&path).expect("sample auth");
        let file = AuthUpload {
            filename: path.file_name().unwrap().to_string_lossy().into_owned(),
            content,
        };
        let result = actix_web::rt::System::new()
            .block_on(async { check_one(shared_client(), file, true).await });
        eprintln!(
            "status={:?} label={} usable={} refreshed={} detail={:?} quota={} updated={}",
            result.status,
            result.status_label,
            result.usable,
            result.refreshed,
            result.detail,
            result.quota,
            result.updated_content.is_some()
        );
        if result.refreshed {
            let c = result
                .updated_content
                .as_deref()
                .expect("refreshed should return updated_content");
            let v: serde_json::Value = serde_json::from_str(c).expect("updated json");
            let at = v.get("access_token").and_then(|x| x.as_str()).unwrap_or("");
            let rt = v
                .get("refresh_token")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            eprintln!("updated access_len={} refresh_len={}", at.len(), rt.len());
            assert!(!at.is_empty(), "updated access_token empty");
            assert!(!rt.is_empty(), "updated refresh_token empty");
            assert_ne!(result.status, AccountStatus::Expired);
            assert_ne!(result.status, AccountStatus::RefreshFailed);
        } else {
            assert_eq!(result.status, AccountStatus::RefreshFailed);
            let d = result.detail.unwrap_or_default();
            assert!(
                d.contains("refresh_token") || d.contains("刷新"),
                "detail should explain refresh failure: {d}"
            );
        }
    }
}
