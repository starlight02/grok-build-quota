//! Probe outcome → AccountStatus / quota display helpers.

use super::probe::ProbeCode;
use crate::check::{AccountPlan, AccountStatus, CheckResult};

pub struct ResultContext {
    account: String,
    filename: String,
}

impl ResultContext {
    pub fn new(account: String, filename: String) -> Self {
        Self { account, filename }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn make(
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

    pub fn empty(
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

    pub fn empty_with_plan(
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

pub fn classify_probe(
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

/// probe 是否判定 Build 周池耗尽（402 或 balance markers）
pub fn balance_exhausted_probe(status_code: Option<u16>, code: Option<ProbeCode>) -> bool {
    status_code == Some(402) || code == Some(ProbeCode::BuildBalanceExhausted)
}

/// 月 included 美元额度展示：(已用%, "月 $剩余 / $上限")，保留美分（对齐 CPA）
pub fn monthly_quota_display(
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

pub fn format_quota(remaining: Option<i64>, limit: Option<i64>) -> String {
    match (remaining, limit) {
        (Some(r), Some(l)) => format!("{r} / {l}"),
        (Some(r), None) => format!("{r} / --"),
        (None, Some(l)) => format!("-- / {l}"),
        _ => "--".into(),
    }
}

pub fn join_notes(primary: &str, notes: &[String]) -> String {
    if notes.is_empty() {
        primary.to_string()
    } else {
        format!("{primary} · {}", notes.join(" · "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
