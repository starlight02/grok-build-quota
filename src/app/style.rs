use crate::check::{AccountPlan, AccountStatus, CheckResult};

pub(crate) fn status_pill_class(status: &AccountStatus) -> &'static str {
    // 统一 tag：圆角胶囊 + 最小宽度，四字标签也能完整显示
    match status {
        AccountStatus::Ok => {
            "inline-flex h-5 min-w-[4.75rem] items-center justify-center rounded-full bg-[#34c759]/14 px-2.5 text-[11px] font-650 tracking-[-0.01em] text-[#248a3d]"
        }
        AccountStatus::Exhausted => {
            "inline-flex h-5 min-w-[4.75rem] items-center justify-center rounded-full bg-[#ff9f0a]/16 px-2.5 text-[11px] font-650 tracking-[-0.01em] text-[#9a6700]"
        }
        AccountStatus::RateLimited => {
            "inline-flex h-5 min-w-[4.75rem] items-center justify-center rounded-full bg-[#ff9f0a]/14 px-2.5 text-[11px] font-650 tracking-[-0.01em] text-[#c93400]"
        }
        AccountStatus::SpendingLimited => {
            "inline-flex h-5 min-w-[4.75rem] items-center justify-center rounded-full bg-[#ff9f0a]/16 px-2.5 text-[11px] font-650 tracking-[-0.01em] text-[#9a6700]"
        }
        AccountStatus::RefreshFailed => {
            "inline-flex h-5 min-w-[4.75rem] items-center justify-center rounded-full bg-[#ff453a]/12 px-2.5 text-[11px] font-650 tracking-[-0.01em] text-[#d70015]"
        }
        AccountStatus::ChatDenied => {
            "inline-flex h-5 min-w-[4.75rem] items-center justify-center rounded-full bg-[#af52de]/14 px-2.5 text-[11px] font-650 tracking-[-0.01em] text-[#7b2fad]"
        }
        AccountStatus::Expired => {
            "inline-flex h-5 min-w-[4.75rem] items-center justify-center rounded-full bg-[#8e8e93]/14 px-2.5 text-[11px] font-650 tracking-[-0.01em] text-[#636366]"
        }
        AccountStatus::NetworkError => {
            "inline-flex h-5 min-w-[4.75rem] items-center justify-center rounded-full bg-[#8e8e93]/14 px-2.5 text-[11px] font-650 tracking-[-0.01em] text-[#636366]"
        }
        _ => {
            "inline-flex h-5 min-w-[4.75rem] items-center justify-center rounded-full bg-[#ff453a]/12 px-2.5 text-[11px] font-650 tracking-[-0.01em] text-[#d70015]"
        }
    }
}

/// 额度条数据：渲染 gate / 剩余百分比 / 配色同源判定。
/// 只有 Ok / Exhausted / RateLimited / SpendingLimited 返回 Some，其余状态
/// 一律 None（不渲染条）——「有配色但没条」的死分支在构造上不可能。
#[derive(Clone, Copy)]
pub(crate) struct QuotaBar {
    /// 剩余百分比 0–100
    pub(crate) pct: f64,
    /// UnoCSS 填充类（DOM 条）
    pub(crate) fill_class: &'static str,
    /// 十六进制色（导出 canvas）
    pub(crate) hex: &'static str,
}

pub(crate) fn quota_bar(r: &CheckResult) -> Option<QuotaBar> {
    // 鉴权失败 / 访问拒绝 / 网络错误等不展示额度，避免误导；
    // 可用 / 耗尽 / 限流 / 消费上限 展示真实额度条
    let (fill_class, hex) = match r.status {
        AccountStatus::Ok => ("bg-[#34c759]", "#34c759"),
        AccountStatus::Exhausted | AccountStatus::RateLimited | AccountStatus::SpendingLimited => {
            ("bg-[#ff9f0a]", "#ff9f0a")
        }
        _ => return None,
    };
    let pct = if let Some(used) = r.usage_percent {
        (100.0 - used).clamp(0.0, 100.0)
    } else if let (Some(rem), Some(lim)) = (r.remaining_tokens, r.limit_tokens) {
        if lim <= 0 {
            return None;
        }
        (rem as f64 / lim as f64 * 100.0).clamp(0.0, 100.0)
    } else {
        return None;
    };
    Some(QuotaBar {
        pct,
        fill_class,
        hex,
    })
}

pub(crate) fn plan_pill_class(plan: &AccountPlan) -> &'static str {
    match plan {
        AccountPlan::Free => {
            "inline-flex h-5 shrink-0 items-center justify-center rounded-full bg-[#8e8e93]/12 px-2 text-[10px] font-650 tracking-[-0.01em] text-[#636366]"
        }
        AccountPlan::SuperGrokLite => {
            "inline-flex h-5 shrink-0 items-center justify-center rounded-full bg-[#5856d6]/10 px-2 text-[10px] font-650 tracking-[-0.01em] text-[#3634a3]"
        }
        AccountPlan::SuperGrok => {
            "inline-flex h-5 shrink-0 items-center justify-center rounded-full bg-[#5856d6]/14 px-2 text-[10px] font-650 tracking-[-0.01em] text-[#3634a3]"
        }
        AccountPlan::SuperGrokHeavy => {
            "inline-flex h-5 shrink-0 items-center justify-center rounded-full bg-[#af52de]/14 px-2 text-[10px] font-650 tracking-[-0.01em] text-[#7b2fad]"
        }
        AccountPlan::PaidOther => {
            "inline-flex h-5 shrink-0 items-center justify-center rounded-full bg-[#ff9f0a]/14 px-2 text-[10px] font-650 tracking-[-0.01em] text-[#9a6700]"
        }
        AccountPlan::Unknown => {
            "inline-flex h-5 shrink-0 items-center justify-center rounded-full bg-black/[0.05] px-2 text-[10px] font-650 tracking-[-0.01em] text-[#8e8e93]"
        }
    }
}

pub(crate) const QUOTA_SEGMENTS: usize = 20;

pub(crate) fn lit_segments(pct: f64) -> usize {
    // 0% => 0, 100% => 20; use ceil so any remaining lights at least 1
    if pct <= 0.0 {
        0
    } else if pct >= 100.0 {
        QUOTA_SEGMENTS
    } else {
        ((pct / 100.0) * QUOTA_SEGMENTS as f64).ceil() as usize
    }
}

pub(crate) fn seg_class(active: bool) -> &'static str {
    if active {
        "gbq-button gbq-button-tab gbq-button-tab-active flex items-center justify-center gap-1.5 rounded-[9px] px-2.5 py-1.5 text-[12px] font-600 outline-none"
    } else {
        "gbq-button gbq-button-tab flex items-center justify-center gap-1.5 rounded-[9px] px-2.5 py-1.5 text-[12px] font-600 text-[#6e6e73] outline-none"
    }
}

/// 紧凑数字：≥100 万→「X.XXM」，≥1000→「X.XXK」，其余原样。None→「--」。
pub(crate) fn fmt_num(v: Option<i64>) -> String {
    let Some(n) = v else {
        return "--".into();
    };
    let neg = n < 0;
    let a = n.unsigned_abs() as f64;
    let s = if a >= 1_000_000.0 {
        format!("{:.2}M", a / 1_000_000.0)
    } else if a >= 1_000.0 {
        format!("{:.2}K", a / 1_000.0)
    } else {
        n.unsigned_abs().to_string()
    };
    if neg { format!("-{s}") } else { s }
}

pub(crate) fn quota_display(r: &CheckResult) -> String {
    if !matches!(
        r.status,
        AccountStatus::Ok
            | AccountStatus::Exhausted
            | AccountStatus::RateLimited
            | AccountStatus::SpendingLimited
    ) {
        return "--".into();
    }
    if r.usage_percent.is_some() {
        return r.quota.clone();
    }
    match (r.remaining_tokens, r.limit_tokens) {
        (Some(rem), Some(lim)) if lim > 0 => {
            let nums = format!("{} / {}", fmt_num(Some(rem)), fmt_num(Some(lim)));
            // Free 是每日 token 窗口，标明「日」（周/月由服务端 quota 直出）
            if r.plan == AccountPlan::Free {
                format!("日 {nums}")
            } else {
                nums
            }
        }
        _ => {
            if r.quota.trim().is_empty() {
                "--".into()
            } else {
                r.quota.clone()
            }
        }
    }
}

/// server fn 失败时的合成网络错误行
pub(crate) fn network_error_result(name: String, err: String) -> CheckResult {
    CheckResult {
        account: name.clone(),
        filename: name,
        status: AccountStatus::NetworkError,
        status_label: AccountStatus::NetworkError.as_label().into(),
        plan: AccountPlan::Unknown,
        plan_label: AccountPlan::Unknown.as_label().into(),
        quota: "--".into(),
        usable: false,
        remaining_tokens: None,
        limit_tokens: None,
        remaining_requests: None,
        limit_requests: None,
        usage_percent: None,
        http_status: None,
        detail: Some(err),
        refreshed: false,
        updated_content: None,
    }
}
