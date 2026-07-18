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
    /// 触发限流（HTTP 429），但 billing 额度未必归零
    RateLimited,
    /// 付费 API / 团队月度消费上限或额度耗尽（spending-limit）
    SpendingLimited,
    AuthFailed,
    /// token 可能仍有效，但上游永久拒绝 chat
    ChatDenied,
    Disabled,
    Expired,
    /// access 已过期且 refresh_token 刷新失败（吊销/无效）
    RefreshFailed,
    Invalid,
    NetworkError,
    Error,
}

impl AccountStatus {
    pub fn as_label(&self) -> &'static str {
        match self {
            Self::Ok => "可用",
            Self::Exhausted => "额度耗尽",
            Self::RateLimited => "限流",
            Self::SpendingLimited => "消费上限",
            Self::AuthFailed => "鉴权失败",
            Self::ChatDenied => "访问拒绝",
            Self::Disabled => "已禁用",
            Self::Expired => "Token 过期",
            Self::RefreshFailed => "刷新失败",
            Self::Invalid => "无效文件",
            Self::NetworkError => "网络错误",
            Self::Error => "检测失败",
        }
    }

    pub fn tone(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Exhausted | Self::RateLimited | Self::SpendingLimited => "warn",
            Self::ChatDenied => "denied",
            Self::RefreshFailed => "warn",
            Self::AuthFailed | Self::Disabled | Self::Expired | Self::Invalid | Self::Error => {
                "bad"
            }
            Self::NetworkError => "mute",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AccountPlan {
    Free,
    SuperGrokLite,
    SuperGrok,
    SuperGrokHeavy,
    PaidOther,
    Unknown,
}

impl AccountPlan {
    pub fn as_label(&self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::SuperGrokLite => "Lite",
            Self::SuperGrok => "Super",
            Self::SuperGrokHeavy => "Heavy",
            Self::PaidOther => "付费",
            Self::Unknown => "未知",
        }
    }

    pub fn is_paid(&self) -> bool {
        matches!(
            self,
            Self::SuperGrokLite | Self::SuperGrok | Self::SuperGrokHeavy | Self::PaidOther
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CheckResult {
    pub account: String,
    pub filename: String,
    pub status: AccountStatus,
    pub status_label: String,
    /// 账号类型：Free / SuperGrok / ...
    pub plan: AccountPlan,
    pub plan_label: String,
    pub quota: String,
    pub usable: bool,
    /// Free：真实 token 剩余；付费周额度：剩余百分比点数(0–100)；
    /// 付费月 fallback / 无数据：None。勿当绝对 token 消费
    pub remaining_tokens: Option<i64>,
    /// 语义随 remaining_tokens：token 上限、固定 100，或 None
    pub limit_tokens: Option<i64>,
    pub remaining_requests: Option<i64>,
    pub limit_requests: Option<i64>,
    /// 付费账号周限额已用百分比（creditUsagePercent 优先，Build 分项兜底）；Free 通常为 None
    pub usage_percent: Option<f64>,
    pub http_status: Option<u16>,
    pub detail: Option<String>,
    /// 本轮是否执行了 OAuth refresh
    pub refreshed: bool,
    /// 刷新成功后合并新 token 的完整 auth JSON（浏览器内存更新用，服务端不落盘）
    pub updated_content: Option<String>,
}

impl CheckResult {
    #[allow(clippy::too_many_arguments)]
    #[cfg_attr(not(feature = "ssr"), allow(dead_code))]
    pub(crate) fn make(
        account: impl Into<String>,
        filename: impl Into<String>,
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
    ) -> Self {
        Self {
            account: account.into(),
            filename: filename.into(),
            status_label: status.as_label().into(),
            status,
            plan: AccountPlan::Unknown,
            plan_label: AccountPlan::Unknown.as_label().into(),
            quota: quota.into(),
            usable,
            remaining_tokens,
            limit_tokens,
            remaining_requests,
            limit_requests,
            usage_percent: None,
            http_status,
            detail,
            refreshed,
            updated_content,
        }
    }

    #[cfg_attr(not(feature = "ssr"), allow(dead_code))]
    pub(crate) fn with_plan(mut self, plan: AccountPlan) -> Self {
        self.plan_label = plan.as_label().into();
        self.plan = plan;
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CheckSummary {
    pub total: usize,
    pub usable: usize,
    pub exhausted: usize,
    pub failed: usize,
    pub results: Vec<CheckResult>,
}

/// 客户端并发探测上限
pub const CHECK_WORKERS: usize = 8;
