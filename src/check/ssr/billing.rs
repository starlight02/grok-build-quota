//! Account plan detection and billing usage (weekly / monthly).

use serde_json::Value;

use super::{
    auth::{AuthRecord, build_headers},
    http::{get_json, json_string},
};
use crate::check::{AccountPlan, QuotaPeriod};

#[derive(Default)]
pub struct BillingUsage {
    pub quota_period: QuotaPeriod,
    pub build_usage_percent: Option<f64>,
    pub credit_usage_percent: Option<f64>,
    pub api_usage_percent: Option<f64>,
    pub chat_usage_percent: Option<f64>,
    pub monthly_limit_cents: Option<f64>,
    pub monthly_used_cents: Option<f64>,
    pub has_product_usage: bool,
}

impl BillingUsage {
    /// 当前额度池总用量：402/耗尽由它决定，GrokBuild 分项只是参考。
    pub fn quota_percent(&self) -> Option<f64> {
        self.credit_usage_percent.or(self.build_usage_percent)
    }

    /// 当前周期用量分项（Api/Build/Chat），有内容时进 detail 解释总量构成。
    pub fn breakdown_note(&self) -> Option<String> {
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
            Some(format!("周期用量分解：{}", parts.join(" · ")))
        }
    }

    fn merge_credit(&mut self, other: Self) {
        self.quota_period = other.quota_period;
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

fn norm_label(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-')
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn parse_period(value: Option<&Value>) -> QuotaPeriod {
    let raw = value
        .and_then(|period| {
            period
                .as_str()
                .or_else(|| period.get("type").and_then(Value::as_str))
        })
        .unwrap_or_default()
        .to_ascii_uppercase();
    if raw.contains("DAILY") || raw.ends_with("_DAY") {
        QuotaPeriod::Daily
    } else if raw.contains("WEEKLY") || raw.ends_with("_WEEK") {
        QuotaPeriod::Weekly
    } else if raw.contains("MONTHLY") || raw.ends_with("_MONTH") {
        QuotaPeriod::Monthly
    } else if raw.contains("ROLLING") {
        QuotaPeriod::Rolling
    } else {
        QuotaPeriod::Unknown
    }
}

pub fn classify_plan(
    tier_display: Option<&str>,
    subscription_tiers: Option<&str>,
    jwt_tier: Option<&str>,
    has_product_usage: bool,
    credit_usage_percent: Option<f64>,
    monthly_limit: Option<f64>,
) -> AccountPlan {
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
        if matches!(tiers.as_str(), "free" | "grokfree" | "none" | "null") {
            return AccountPlan::Free;
        }
        if tiers.contains("heavy") {
            return AccountPlan::SuperGrokHeavy;
        }
        if tiers.contains("lite") {
            return AccountPlan::SuperGrokLite;
        }
        if tiers.contains("supergrok") || tiers.contains("grokpro") || tiers.contains("pro") {
            return AccountPlan::SuperGrok;
        }
        return AccountPlan::PaidOther;
    }

    if has_product_usage {
        return AccountPlan::PaidOther;
    }
    if credit_usage_percent.is_some() {
        return AccountPlan::PaidOther;
    }
    // 月额度金额只用于确认这是付费套餐，不再用固定金额猜 Lite/Super/Heavy。
    // xAI 调整 included 金额时仍能保持兼容。
    if monthly_limit.is_some_and(|limit| limit > 0.0) {
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
    if let Some(obj) = v.as_object()
        && let Some(inner) = obj.get("val")
    {
        return billing_val(inner);
    }
    None
}

/// billing 解析结果：当前周期总量 + Api/Build/Chat 分项 + 月度美分额度
pub fn parse_billing_payload(data: &Value) -> BillingUsage {
    let cfg = data.get("config").filter(|c| c.is_object()).unwrap_or(data);
    let mut out = BillingUsage {
        quota_period: parse_period(
            cfg.get("currentPeriod")
                .or_else(|| cfg.get("current_period"))
                .or_else(|| cfg.get("usagePeriod"))
                .or_else(|| cfg.get("usage_period")),
        ),
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

/// 类型探测结果：plan + 原始标签（billing 细化时保留以消歧 Lite/Heavy）
pub struct PlanInfo {
    pub plan: AccountPlan,
    pub tier_display: Option<String>,
    pub subscription_tiers: Option<String>,
}

pub async fn fetch_plan(
    client: &reqwest::Client,
    auth: &AuthRecord,
    base_url: &str,
    jwt_tier: Option<String>,
    cli_version: &str,
) -> PlanInfo {
    let Ok(headers) = build_headers(auth, cli_version) else {
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

pub async fn fetch_billing(
    client: &reqwest::Client,
    auth: &AuthRecord,
    base_url: &str,
    cli_version: &str,
) -> Option<BillingUsage> {
    let headers = build_headers(auth, cli_version).ok()?;
    let base = base_url.trim_end_matches('/');

    let mut out = BillingUsage::default();

    // 周期额度：/billing?format=credits 提供总量、周期类型和产品分项。
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(p.quota_period, QuotaPeriod::Weekly);

        let usage = p;
        // 展示必须取周期总量 100%，不能取 GrokBuild 分项 2%
        assert_eq!(usage.quota_percent(), Some(100.0));
        let note = usage.breakdown_note().expect("breakdown note");
        assert!(note.contains("Api 96%"));
        assert!(note.contains("Build 2%"));
        assert!(note.contains("Chat 2%"));

        // 无总量时先兜 Build 分项
        let only_build = BillingUsage {
            build_usage_percent: Some(42.0),
            ..Default::default()
        };
        assert_eq!(only_build.quota_percent(), Some(42.0));
    }

    #[test]
    fn parses_dynamic_quota_periods() {
        for (raw, expected) in [
            ("USAGE_PERIOD_TYPE_DAILY", QuotaPeriod::Daily),
            ("USAGE_PERIOD_TYPE_WEEKLY", QuotaPeriod::Weekly),
            ("USAGE_PERIOD_TYPE_MONTHLY", QuotaPeriod::Monthly),
            ("USAGE_PERIOD_TYPE_ROLLING", QuotaPeriod::Rolling),
            ("USAGE_PERIOD_TYPE_UNSPECIFIED", QuotaPeriod::Unknown),
        ] {
            let payload = serde_json::json!({
                "config": {
                    "creditUsagePercent": 12.0,
                    "currentPeriod": {"type": raw}
                }
            });
            assert_eq!(parse_billing_payload(&payload).quota_period, expected);
        }
    }

    #[test]
    fn plan_classification_does_not_depend_on_fixed_allowance_amounts() {
        assert_eq!(
            classify_plan(
                Some("SuperGrok Heavy"),
                None,
                None,
                true,
                Some(10.0),
                Some(999_999.0)
            ),
            AccountPlan::SuperGrokHeavy
        );
        assert_eq!(
            classify_plan(None, None, None, false, None, Some(12_345.0)),
            AccountPlan::PaidOther
        );
    }
}
