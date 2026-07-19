//! Server-side account probe orchestration.
//!
//! Submodules (by responsibility):
//! - [`client`] — shared reqwest client
//! - [`http`] — generic GET/JSON/header helpers
//! - [`markers`] — versioned upstream error marker tables
//! - [`auth`] — auth JSON, OAuth refresh, JWT, headers
//! - [`probe`] — `/v1/responses` probe
//! - [`billing`] — plan + weekly/monthly usage
//! - [`classify`] — status / quota presentation

mod auth;
mod billing;
mod classify;
mod client;
mod http;
mod markers;
mod probe;

use auth::{
    humanize_refresh_err, jwt_claim_tier, jwt_expired, normalize_base_url, resolve_auth_record,
    serialize_auth, try_refresh,
};
use billing::{classify_plan, fetch_billing, fetch_plan};
use classify::{
    ResultContext, balance_exhausted_probe, classify_probe, format_quota, join_notes,
    monthly_quota_display,
};
pub use client::shared_client;
use probe::probe_responses;

use crate::check::{AccountPlan, AccountStatus, AuthUpload, CheckResult};

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
    if want_billing && let Some(billing) = fetch_billing(client, &auth, &base_url).await {
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::check::AuthUpload;

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
