use std::collections::{HashMap, HashSet};

use futures::{StreamExt, stream};
use js_sys::{Array, Object, Reflect};
use leptos::{prelude::*, task::spawn_local};
use leptos_meta::{Stylesheet, Title, provide_meta_context};
use leptos_router::{
    StaticSegment, WildcardSegment,
    components::{Route, Router, Routes},
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, ClipboardItem, DragEvent, Event, File, HtmlInputElement, Url, window};

use crate::check::{
    AccountPlan, AccountStatus, AuthUpload, CHECK_WORKERS, CheckResult, CheckSummary,
    check_auth_file,
};

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/grok-build-quota.css" />
        <Title text="Grok Build 额度检测" />
        <Router>
            <main class="min-h-screen">
                <Routes fallback=move || view! { <NotFound /> }>
                    <Route path=StaticSegment("") view=HomePage />
                    <Route path=WildcardSegment("any") view=NotFound />
                </Routes>
            </main>
        </Router>
    }
}

#[derive(Clone)]
struct SelectedFile {
    name: String,
    content: String,
}

fn status_pill_class(status: &AccountStatus) -> &'static str {
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
            "inline-flex h-5 min-w-[4.75rem] items-center justify-center rounded-full bg-[#ff9f0a]/16 px-2.5 text-[11px] font-650 tracking-[-0.01em] text-[#9a6700]"
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
struct QuotaBar {
    /// 剩余百分比 0–100
    pct: f64,
    /// UnoCSS 填充类（DOM 条）
    fill_class: &'static str,
    /// 十六进制色（导出 canvas）
    hex: &'static str,
}

fn quota_bar(r: &CheckResult) -> Option<QuotaBar> {
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

fn plan_pill_class(plan: &AccountPlan) -> &'static str {
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

const QUOTA_SEGMENTS: usize = 20;

fn lit_segments(pct: f64) -> usize {
    // 0% => 0, 100% => 20; use ceil so any remaining lights at least 1
    if pct <= 0.0 {
        0
    } else if pct >= 100.0 {
        QUOTA_SEGMENTS
    } else {
        ((pct / 100.0) * QUOTA_SEGMENTS as f64).ceil() as usize
    }
}

#[derive(Clone, Copy, PartialEq)]
enum StatusFilter {
    All,
    Usable,
    Exhausted,
    Other,
}

impl StatusFilter {
    fn matches(self, r: &CheckResult) -> bool {
        match self {
            StatusFilter::All => true,
            StatusFilter::Usable => r.usable,
            StatusFilter::Exhausted => r.status == AccountStatus::Exhausted,
            StatusFilter::Other => !r.usable && r.status != AccountStatus::Exhausted,
        }
    }

    fn export_slug(self) -> &'static str {
        match self {
            StatusFilter::All => "all",
            StatusFilter::Usable => "usable",
            StatusFilter::Exhausted => "exhausted",
            StatusFilter::Other => "other",
        }
    }

    fn export_label(self) -> &'static str {
        match self {
            StatusFilter::All => "全部",
            StatusFilter::Usable => "可用",
            StatusFilter::Exhausted => "耗尽",
            StatusFilter::Other => "其他",
        }
    }
}

fn seg_class(active: bool) -> &'static str {
    if active {
        "flex items-center justify-center gap-1.5 rounded-[9px] border-0 bg-white px-2.5 py-1.5 text-[12px] font-600 text-[#1d1d1f] shadow-[0_1px_3px_rgba(0,0,0,0.08),inset_0_1px_0_rgba(255,255,255,0.95)] outline-none transition"
    } else {
        "flex items-center justify-center gap-1.5 rounded-[9px] border-0 bg-transparent px-2.5 py-1.5 text-[12px] font-600 text-[#6e6e73] shadow-none outline-none transition hover:text-[#1d1d1f]"
    }
}

fn group_thousands(n: i64) -> String {
    let neg = n < 0;
    let digits = n.abs().to_string();
    let bytes = digits.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    if neg { format!("-{out}") } else { out }
}

fn fmt_num(v: Option<i64>) -> String {
    v.map(group_thousands).unwrap_or_else(|| "--".into())
}

fn quota_display(r: &CheckResult) -> String {
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
fn network_error_result(name: String, err: String) -> CheckResult {
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

#[component]
fn HomePage() -> impl IntoView {
    let selected = RwSignal::new(Vec::<SelectedFile>::new());
    let checking = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    let summary = RwSignal::new(Option::<CheckSummary>::None);
    let copy_msg = RwSignal::new(Option::<String>::None);
    let drag_over = RwSignal::new(false);
    let filter = RwSignal::new(StatusFilter::All);
    // 401/过期时是否自动刷新 token（默认关，避免静默换新后原文件失效）
    let auto_refresh = RwSignal::new(false);
    let refreshing = RwSignal::new(false);
    // 正在逐行重试的文件名（网络错误 / 失败行）
    let retrying = RwSignal::new(Option::<String>::None);
    // 已落盘的文件名（导出 ZIP / 逐行下载后置位）：清空 guard 用
    let saved_files = RwSignal::new(HashSet::<String>::new());


    let on_files = move |file_list: web_sys::FileList| {
        // FileList is live; clearing the <input> empties it. Snapshot File handles first.
        let mut files = Vec::with_capacity(file_list.length() as usize);
        for i in 0..file_list.length() {
            if let Some(file) = file_list.item(i) {
                files.push(file);
            }
        }
        spawn_local(async move {
            let mut next = selected.get_untracked();
            for file in files {
                let name = file.name();
                if !name.to_ascii_lowercase().ends_with(".json") {
                    continue;
                }
                match read_file_text(&file).await {
                    Ok(content) => {
                        if let Some(slot) = next.iter_mut().find(|f| f.name == name) {
                            slot.content = content;
                        } else {
                            next.push(SelectedFile { name, content });
                        }
                    }
                    Err(err) => error.set(Some(err)),
                }
            }
            next.sort_by(|a, b| a.name.cmp(&b.name));
            selected.set(next);
        });
    };

    let on_input_change = move |ev: Event| {
        let input: HtmlInputElement = event_target(&ev);
        if let Some(files) = input.files() {
            on_files(files);
        }
        input.set_value("");
    };

    let on_drop = move |ev: DragEvent| {
        ev.prevent_default();
        drag_over.set(false);
        if let Some(dt) = ev.data_transfer()
            && let Some(files) = dt.files()
        {
            on_files(files);
        }
    };

    // 关页面前兜底：有「已刷新未落盘」的 token 时弹浏览器原生离开确认
    // Effect 只在客户端运行；SSR 渲染组件体时 window() 会直接 panic（非 wasm target）
    Effect::new(move || {
        let handler = Closure::wrap(Box::new(move |ev: web_sys::BeforeUnloadEvent| {
            let unsaved = summary
                .get_untracked()
                .map(|s| {
                    let saved = saved_files.get_untracked();
                    s.results
                        .iter()
                        .any(|r| r.refreshed && !saved.contains(&r.filename))
                })
                .unwrap_or(false);
            if unsaved {
                ev.prevent_default();
                ev.set_return_value("");
            }
        }) as Box<dyn FnMut(_)>);
        if let Some(w) = window() {
            w.set_onbeforeunload(Some(handler.as_ref().unchecked_ref()));
        }
        handler.forget();
    });

    let clear_files = move |_| {
        // 有「已刷新但未落盘」的行时拒绝清空：刷新只在内存，清了原文件就废了
        let unsaved = summary
            .get_untracked()
            .map(|s| {
                let saved = saved_files.get_untracked();
                s.results
                    .iter()
                    .any(|r| r.refreshed && !saved.contains(&r.filename))
            })
            .unwrap_or(false);
        if unsaved {
            error.set(Some(
                "有账号的 token 已刷新但新文件未落盘。请先「导出 ZIP」或逐行下载，再清空。".into(),
            ));
            return;
        }
        selected.set(Vec::new());
        summary.set(None);
        error.set(None);
        copy_msg.set(None);
        saved_files.set(HashSet::new());
    };

    let remove_file = move |name: String| {
        selected.update(|list| list.retain(|f| f.name != name));
    };

    // 把一行结果并入 summary（插入或替换），刷新后的内容写回 selected
    let apply_result = move |result: CheckResult| {
        if let Some(content) = result.updated_content.clone() {
            let fname = result.filename.clone();
            selected.update(|list| {
                if let Some(slot) = list.iter_mut().find(|f| f.name == fname) {
                    slot.content = content;
                }
            });
        }
        summary.update(|slot| {
            let Some(s) = slot.as_mut() else { return };
            if let Some(pos) = s.results.iter().position(|r| r.filename == result.filename) {
                let old = s.results.remove(pos);
                match old.status {
                    AccountStatus::Ok => s.usable = s.usable.saturating_sub(1),
                    AccountStatus::Exhausted => s.exhausted = s.exhausted.saturating_sub(1),
                    _ => s.failed = s.failed.saturating_sub(1),
                }
            }
            match result.status {
                AccountStatus::Ok => s.usable += 1,
                AccountStatus::Exhausted => s.exhausted += 1,
                _ => s.failed += 1,
            }
            let pos = s.results.partition_point(|r| r.filename < result.filename);
            s.results.insert(pos, result);
        });
    };

    // 刷新只写浏览器内存：提示导出落盘，避免旧 RT 被轮换后二次失败
    let refresh_notice = move || {
        if let Some(s) = summary.get_untracked() {
            let refreshed_n = s.results.iter().filter(|r| r.refreshed).count();
            let refresh_fail_n = s
                .results
                .iter()
                .filter(|r| r.status == AccountStatus::RefreshFailed)
                .count();
            if refreshed_n > 0 {
                copy_msg.set(Some(format!(
                    "已刷新 {refreshed_n} 个 token（仅在浏览器内存）。请点「导出 ZIP」落盘，否则下次用旧文件会因 refresh_token 轮换失败。"
                )));
            } else if refresh_fail_n > 0 {
                copy_msg.set(Some(format!(
                    "{refresh_fail_n} 个账号刷新失败：refresh_token 可能已吊销/被其它工具轮换，需重新登录拿新 auth。"
                )));
            }
        }
    };

    let run_check = move |_| {
        let files = selected.get_untracked();
        if files.is_empty() {
            error.set(Some("请先选择 auth JSON 文件".into()));
            return;
        }
        checking.set(true);
        error.set(None);
        copy_msg.set(None);
        saved_files.set(HashSet::new());
        summary.set(Some(CheckSummary {
            total: files.len(),
            usable: 0,
            exhausted: 0,
            failed: 0,
            results: Vec::new(),
        }));

        let allow = auto_refresh.get_untracked();
        let uploads = files
            .into_iter()
            .map(|f| AuthUpload {
                filename: f.name,
                content: f.content,
            })
            .collect::<Vec<_>>();

        // 客户端并发探测，上限 CHECK_WORKERS
        spawn_local(async move {
            let mut pending = stream::iter(uploads.into_iter().map(|upload| async move {
                let name = upload.filename.clone();
                (name, check_auth_file(upload, allow).await)
            }))
            .buffer_unordered(CHECK_WORKERS);

            while let Some((name, res)) = pending.next().await {
                let result = match res {
                    Ok(r) => r,
                    Err(err) => network_error_result(name, err.to_string()),
                };
                apply_result(result);
            }

            checking.set(false);
            refresh_notice();
        });
    };

    // 手动刷新「Token 过期」的行（自动刷新关闭时的补救入口）
    let refresh_expired = move |_| {
        let Some(data) = summary.get_untracked() else { return };
        let expired: HashSet<String> = data
            .results
            .iter()
            .filter(|r| r.status == AccountStatus::Expired)
            .map(|r| r.filename.clone())
            .collect();
        if expired.is_empty() {
            return;
        }
        let uploads: Vec<AuthUpload> = selected
            .get_untracked()
            .into_iter()
            .filter(|f| expired.contains(&f.name))
            .map(|f| AuthUpload {
                filename: f.name,
                content: f.content,
            })
            .collect();
        if uploads.is_empty() {
            copy_msg.set(Some("找不到对应的本地文件内容".into()));
            return;
        }
        refreshing.set(true);
        error.set(None);
        copy_msg.set(None);
        spawn_local(async move {
            let mut pending = stream::iter(uploads.into_iter().map(|upload| async move {
                let name = upload.filename.clone();
                (name, check_auth_file(upload, true).await)
            }))
            .buffer_unordered(CHECK_WORKERS);

            while let Some((name, res)) = pending.next().await {
                let result = match res {
                    Ok(r) => r,
                    Err(err) => network_error_result(name, err.to_string()),
                };
                apply_result(result);
            }

            refreshing.set(false);
            refresh_notice();
        });
    };

    // 逐行下载该账号的 auth JSON（优先刷新后的内存内容）
    let download_one = move |filename: String| {
        let content = summary
            .get_untracked()
            .and_then(|s| s.results.into_iter().find(|r| r.filename == filename))
            .and_then(|r| r.updated_content)
            .or_else(|| {
                selected
                    .get_untracked()
                    .into_iter()
                    .find(|f| f.name == filename)
                    .map(|f| f.content)
            });
        let Some(content) = content else {
            copy_msg.set(Some("找不到对应的本地文件内容".into()));
            return;
        };
        match json_download_blob(&content).and_then(|b| download_blob(&b, &filename)) {
            Ok(()) => {
                let fname = filename.clone();
                saved_files.update(|set| {
                    set.insert(fname.clone());
                });
                copy_msg.set(Some(format!("已下载 {fname}")));
            }
            Err(err) => copy_msg.set(Some(err)),
        }
    };

    // 逐行重试：网络错误 / 失败行可单独再探一次（尊重自动刷新开关）
    let retry_one = move |filename: String| {
        if checking.get_untracked() || refreshing.get_untracked() {
            return;
        }
        if retrying.get_untracked().is_some() {
            return;
        }
        let Some(upload) = selected
            .get_untracked()
            .into_iter()
            .find(|f| f.name == filename)
            .map(|f| AuthUpload {
                filename: f.name,
                content: f.content,
            })
        else {
            copy_msg.set(Some("找不到对应的本地文件内容".into()));
            return;
        };
        let allow = auto_refresh.get_untracked();
        let fname = upload.filename.clone();
        retrying.set(Some(fname.clone()));
        error.set(None);
        spawn_local(async move {
            let result = match check_auth_file(upload, allow).await {
                Ok(r) => r,
                Err(err) => network_error_result(fname.clone(), err.to_string()),
            };
            apply_result(result);
            retrying.set(None);
            refresh_notice();
        });
    };

    // 「Token 过期」行数：驱动手动刷新按钮
    let expired_count = move || {
        summary
            .get()
            .map(|s| {
                s.results
                    .iter()
                    .filter(|r| r.status == AccountStatus::Expired)
                    .count()
            })
            .unwrap_or(0)
    };

    let copy_image = move |_| {
        let Some(data) = summary.get_untracked() else {
            copy_msg.set(Some("没有可复制的检测结果".into()));
            return;
        };
        // 完整账号来自本地上传的文件内容，不打码，不出服务端
        let emails: HashMap<String, String> = selected
            .get_untracked()
            .iter()
            .filter_map(|f| extract_email(&f.content).map(|e| (f.name.clone(), e)))
            .collect();
        spawn_local(async move {
            match export_results_image(&data.results, &emails).await {
                Ok(msg) => copy_msg.set(Some(msg)),
                Err(err) => copy_msg.set(Some(err)),
            }
        });
    };

    let export_zip = move |_| {
        let Some(data) = summary.get_untracked() else {
            copy_msg.set(Some("没有可导出的检测结果".into()));
            return;
        };
        let f = filter.get_untracked();
        let names: std::collections::HashSet<String> = data
            .results
            .iter()
            .filter(|r| f.matches(r))
            .map(|r| r.filename.clone())
            .collect();
        if names.is_empty() {
            copy_msg.set(Some(format!(
                "当前「{}」筛选下没有可导出账号",
                f.export_label()
            )));
            return;
        }

        // 使用内存中的文件内容（含 refresh 后 token）
        let files: Vec<(String, String)> = selected
            .get_untracked()
            .into_iter()
            .filter(|sf| names.contains(&sf.name))
            .map(|sf| (sf.name, sf.content))
            .collect();
        if files.is_empty() {
            copy_msg.set(Some("找不到对应的本地文件内容".into()));
            return;
        }

        let count = files.len();
        let tab = f.export_label();
        let suggested = format!(
            "grok-auth-{}-{}.zip",
            f.export_slug(),
            js_sys::Date::new_0()
                .to_iso_string()
                .as_string()
                .unwrap_or_else(|| "export".into())
                .chars()
                .take(10)
                .collect::<String>()
        );

        // 同步打 zip，尽量保留 click 的 user activation 给 showSaveFilePicker
        let blob = match build_auth_zip(&files).and_then(|b| bytes_to_zip_blob(&b)) {
            Ok(b) => b,
            Err(err) => {
                copy_msg.set(Some(err));
                return;
            }
        };

        spawn_local(async move {
            match save_blob_picker_or_download(&blob, &suggested).await {
                Ok(msg) if msg == "已取消导出" => copy_msg.set(Some(msg)),
                Ok(msg) => {
                    // 导出的文件已落盘：解除清空 guard
                    saved_files.update(|set| set.extend(names.iter().cloned()));
                    copy_msg.set(Some(format!("{msg} · {tab} · {count} 个文件")));
                }
                Err(err) => copy_msg.set(Some(err)),
            }
        });
    };

    view! {
        <div class="relative flex h-svh flex-col overflow-hidden bg-[#f5f5f7] font-sans text-[#1d1d1f] antialiased max-lg:h-auto max-lg:min-h-svh max-lg:overflow-y-auto">
            <div class="relative mx-auto flex h-full w-full max-w-6xl min-h-0 flex-1 flex-col gap-3 px-3 py-3 sm:gap-4 sm:px-5 sm:py-4 md:gap-4 md:py-5">
                <div class="flex min-h-15 shrink-0 items-center justify-between gap-4 rounded-[22px] border border-white/90 bg-white/62 px-4 shadow-[0_16px_50px_rgba(0,0,0,0.07),inset_0_1px_0_rgba(255,255,255,0.95)] ring-1 ring-black/4 backdrop-blur-3xl backdrop-saturate-150 sm:px-5">
                    <div class="flex min-w-0 items-center gap-3">
                        <div class="hidden items-center gap-1.5 sm:flex" aria-hidden="true">
                            <span class="h-2.5 w-2.5 rounded-full bg-[#ff5f57]"></span>
                            <span class="h-2.5 w-2.5 rounded-full bg-[#febc2e]"></span>
                            <span class="h-2.5 w-2.5 rounded-full bg-[#28c840]"></span>
                        </div>
                        <span class="hidden h-5 w-px bg-black/8 sm:block"></span>
                        <div class="grid h-9 w-9 shrink-0 place-items-center rounded-[12px] border border-white bg-[#ececef]/92 text-[14px] font-750 shadow-[0_5px_14px_rgba(0,0,0,0.08),inset_0_1px_0_white]">
                            "G"
                        </div>
                        <div class="min-w-0">
                            <div class="truncate text-[13px] font-700 tracking-0">"Grok Build"</div>
                            <div class="truncate text-[11px] text-[#86868b]">"额度检测台"</div>
                        </div>
                    </div>
                    <div class="flex items-center gap-2 rounded-full border border-black/5 bg-white/55 px-3 py-1.5 text-[11px] font-600 text-[#6e6e73] shadow-[inset_0_1px_0_white]">
                        <span class="h-1.5 w-1.5 rounded-full bg-[#34c759] shadow-[0_0_0_3px_rgba(52,199,89,0.12)]"></span>
                        <span>"Token 不离开服务端"</span>
                    </div>
                </div>

                <header class="flex shrink-0 flex-col gap-3 px-1 py-1 sm:px-3 md:flex-row md:items-end md:justify-between md:py-2">
                    <div class="max-w-3xl">
                        <div class="mb-2 text-[10px] font-700 uppercase tracking-[0.14em] text-[#86868b]">
                            "GROK BUILD QUOTA"
                        </div>
                        <h1 class="m-0 max-w-3xl text-[30px] font-750 leading-[1.12] tracking-0 sm:text-[34px]">
                            "额度批量检测"
                        </h1>
                        <p class="mb-0 mt-2 max-w-2xl text-[13px] leading-5 text-[#6e6e73] sm:text-[14px]">
                            "导入 CLIProxyAPI / Grok Build auth JSON，快速确认账号状态与剩余额度。"
                        </p>
                    </div>
                    <div class="flex items-center gap-2 rounded-full border border-white/95 bg-white/48 px-3 py-2 text-[11px] text-[#6e6e73] shadow-[0_8px_24px_rgba(0,0,0,0.04),inset_0_1px_0_white] ring-1 ring-black/3 backdrop-blur-2xl">
                        <span>"单次最多"</span>
                        <span class="font-mono font-700 text-[#1d1d1f]">"200"</span>
                        <span>"个文件"</span>
                    </div>
                </header>

                <Show when=move || error.get().is_some()>
                    <div class="rounded-[16px] border border-[#e7c4c7] bg-[#fff4f4]/92 px-4 py-3 text-[13px] font-600 text-[#9e353d] shadow-[0_10px_30px_rgba(0,0,0,0.04)]">
                        {move || error.get().unwrap_or_default()}
                    </div>
                </Show>

                // ══ 两栏工作台：左=账号(auth 文件)列表 / 右=额度表格 ══
                <div class="grid min-h-0 flex-1 gap-4 md:gap-5 lg:grid-cols-[minmax(300px,380px)_minmax(0,1fr)] lg:items-stretch">

                    // ─────────── 左栏：导入 + 账号列表 ───────────
                    <section class="flex min-h-0 flex-col gap-3 overflow-hidden rounded-[28px] border border-white/95 bg-white/58 p-4 shadow-[0_24px_70px_rgba(0,0,0,0.07),inset_0_1px_0_rgba(255,255,255,0.95)] ring-1 ring-black/4 backdrop-blur-3xl backdrop-saturate-150 sm:p-5 lg:h-full">
                        <div class="flex shrink-0 items-center justify-between">
                            <div>
                                <div class="text-[10px] font-700 tracking-[0.14em] text-[#86868b]">
                                    "01 / ACCOUNTS"
                                </div>
                                <h2 class="mb-0 mt-1.5 text-[18px] font-700 tracking-0">
                                    "账号列表"
                                </h2>
                            </div>
                            <span class="rounded-full border border-black/5 bg-white/60 px-2.5 py-1 font-mono text-[11px] font-700 text-[#3a3a3c] shadow-[inset_0_1px_0_white]">
                                {move || format!("{}", selected.get().len())}
                            </span>
                        </div>

                        // 拖拽导入区（有文件时收成紧凑条，避免撑高页面）
                        <div
                            class=move || {
                                let has_files = !selected.get().is_empty();
                                if drag_over.get() {
                                    if has_files {
                                        "flex shrink-0 items-center gap-3 rounded-[16px] border border-[#a1a1a6] bg-white/82 px-3 py-2.5 shadow-[0_0_0_4px_rgba(0,0,0,0.03),inset_0_1px_0_white] transition"
                                    } else {
                                        "flex shrink-0 flex-col items-center justify-center gap-3 rounded-[20px] border border-[#a1a1a6] bg-white/82 px-4 py-5 text-center shadow-[0_0_0_5px_rgba(0,0,0,0.035),inset_0_1px_0_white] transition"
                                    }
                                } else if has_files {
                                    "flex shrink-0 items-center gap-3 rounded-[16px] border border-dashed border-[#c7c7cc] bg-[#fbfbfd]/74 px-3 py-2.5 shadow-[inset_0_1px_0_white] transition hover:border-[#a1a1a6] hover:bg-white/82"
                                } else {
                                    "flex shrink-0 flex-col items-center justify-center gap-3 rounded-[20px] border border-dashed border-[#c7c7cc] bg-[#fbfbfd]/74 px-4 py-5 text-center shadow-[inset_0_1px_0_white] transition hover:border-[#a1a1a6] hover:bg-white/82"
                                }
                            }
                            on:dragover=move |ev| {
                                ev.prevent_default();
                                drag_over.set(true);
                            }
                            on:dragleave=move |_| drag_over.set(false)
                            on:drop=on_drop
                        >
                            <div
                                class=move || {
                                    if selected.get().is_empty() {
                                        "grid h-12 w-12 place-items-center rounded-[16px] border border-white bg-white/90 text-[#3a3a3c] shadow-[0_12px_32px_rgba(0,0,0,0.08),inset_0_1px_0_white] ring-1 ring-black/5"
                                    } else {
                                        "grid h-9 w-9 shrink-0 place-items-center rounded-[12px] border border-white bg-white/90 text-[#3a3a3c] shadow-[0_8px_20px_rgba(0,0,0,0.06),inset_0_1px_0_white] ring-1 ring-black/5"
                                    }
                                }
                                aria-hidden="true"
                            >
                                <svg
                                    class=move || {
                                        if selected.get().is_empty() {
                                            "h-5 w-5"
                                        } else {
                                            "h-4 w-4"
                                        }
                                    }
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="1.7"
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                >
                                    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
                                    <path d="m17 8-5-5-5 5"></path>
                                    <path d="M12 3v12"></path>
                                </svg>
                            </div>
                            <div class=move || {
                                if selected.get().is_empty() { "" } else { "min-w-0 flex-1" }
                            }>
                                <div class=move || {
                                    if selected.get().is_empty() {
                                        "text-[14px] font-700 tracking-0"
                                    } else {
                                        "truncate text-[13px] font-700 tracking-0"
                                    }
                                }>"拖入 auth JSON"</div>
                                <div class="mt-0.5 text-[11px] text-[#86868b]">
                                    "批量上传 · 自动跳过非 JSON"
                                </div>
                            </div>
                            <label class="inline-flex min-h-9 shrink-0 cursor-pointer items-center justify-center rounded-[11px] border border-black/6 bg-white/92 px-4 text-[12px] font-650 shadow-[0_7px_20px_rgba(0,0,0,0.07),inset_0_1px_0_white] transition hover:-translate-y-0.5 hover:bg-white">
                                <span>"选择文件"</span>
                                <input
                                    class="hidden"
                                    type="file"
                                    accept="application/json,.json"
                                    multiple
                                    on:change=on_input_change
                                />
                            </label>
                        </div>

                        // 已导入的 auth 文件列表：圆角 inset well + 上下淡出，避免硬裁切感
                        <div class="relative flex min-h-0 flex-1 flex-col overflow-hidden">
                            <Show
                                when=move || !selected.get().is_empty()
                                fallback=|| {
                                    view! {
                                        <div class="grid min-h-24 flex-1 place-items-center rounded-[18px] border border-dashed border-black/8 bg-[#f7f7f8]/45 px-4 text-center text-[12px] text-[#86868b] shadow-[inset_0_1px_0_white]">
                                            "尚未导入账号文件"
                                        </div>
                                    }
                                }
                            >
                                <div class="relative flex min-h-0 flex-1 flex-col overflow-hidden rounded-[18px] border border-black/[0.05] bg-[#eef0f3]/72 shadow-[inset_0_1px_0_rgba(255,255,255,0.9),inset_0_0_0_0.5px_rgba(255,255,255,0.35)] ring-1 ring-black/[0.02]">
                                    // 顶部淡出：把硬裁切变成柔和过渡
                                    <div
                                        class="pointer-events-none absolute inset-x-0 top-0 z-10 h-6 rounded-t-[18px] bg-gradient-to-b from-[#eef0f3] via-[#eef0f3]/75 to-transparent"
                                        aria-hidden="true"
                                    ></div>
                                    // 底部淡出
                                    <div
                                        class="pointer-events-none absolute inset-x-0 bottom-0 z-10 h-6 rounded-b-[18px] bg-gradient-to-t from-[#eef0f3] via-[#eef0f3]/75 to-transparent"
                                        aria-hidden="true"
                                    ></div>
                                    <div class="flex min-h-0 flex-1 flex-col gap-1.5 overflow-y-auto overscroll-contain px-1.5 py-2.5 [scrollbar-color:rgba(60,60,67,0.22)_transparent] [scrollbar-width:thin]">
                                        <For
                                            each=move || selected.get()
                                            key=|f| f.name.clone()
                                            children=move |f| {
                                                let name = f.name.clone();
                                                let name_del = f.name.clone();
                                                view! {
                                                    <div class="group flex shrink-0 items-center gap-2.5 rounded-[12px] border border-white/95 bg-white/88 px-2.5 py-2 shadow-[0_1px_0_rgba(255,255,255,0.98)_inset,0_1px_2px_rgba(0,0,0,0.03)] ring-1 ring-black/[0.03] transition hover:bg-white hover:shadow-[0_1px_0_rgba(255,255,255,1)_inset,0_4px_12px_rgba(0,0,0,0.05)]">
                                                        <span
                                                            class="grid h-7 w-7 shrink-0 place-items-center rounded-[9px] border border-black/[0.04] bg-gradient-to-b from-[#f7f7f8] to-[#ececef] text-[#8e8e93] shadow-[inset_0_1px_0_white]"
                                                            aria-hidden="true"
                                                        >
                                                            <svg
                                                                class="h-3.5 w-3.5"
                                                                viewBox="0 0 24 24"
                                                                fill="none"
                                                                stroke="currentColor"
                                                                stroke-width="1.7"
                                                                stroke-linecap="round"
                                                                stroke-linejoin="round"
                                                            >
                                                                <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path>
                                                                <path d="M14 2v6h6"></path>
                                                                <path d="M9 15h6"></path>
                                                            </svg>
                                                        </span>
                                                        <span
                                                            class="min-w-0 flex-1 truncate font-mono text-[11.5px] tracking-[-0.01em] text-[#3a3a3c]"
                                                            title=name.clone()
                                                        >
                                                            {name.clone()}
                                                        </span>
                                                        <button
                                                            type="button"
                                                            class="grid h-5 w-5 shrink-0 place-items-center rounded-full border-0 bg-[#e8e8ed] p-0 text-[#636366] shadow-[inset_0_0.5px_0_rgba(255,255,255,0.85)] outline-none transition duration-150 hover:bg-[#ff453a] hover:text-white hover:shadow-none active:scale-90 focus-visible:ring-2 focus-visible:ring-black/20 disabled:pointer-events-none disabled:opacity-40"
                                                            aria-label="移除"
                                                            disabled=move || checking.get()
                                                            on:click=move |_| remove_file(name_del.clone())
                                                        >
                                                            <svg
                                                                class="h-2.5 w-2.5"
                                                                viewBox="0 0 24 24"
                                                                fill="none"
                                                                stroke="currentColor"
                                                                stroke-width="3"
                                                                stroke-linecap="round"
                                                                stroke-linejoin="round"
                                                                aria-hidden="true"
                                                            >
                                                                <path d="M18 6 6 18"></path>
                                                                <path d="m6 6 12 12"></path>
                                                            </svg>
                                                        </button>
                                                    </div>
                                                }
                                            }
                                        />
                                    </div>
                                </div>
                            </Show>
                        </div>

                        // 自动刷新开关（默认关：刷新只写内存，避免原文件悄悄失效）
                        <div class="flex shrink-0 items-center justify-between gap-3 rounded-[14px] border border-black/5 bg-white/50 px-3 py-2.5 shadow-[inset_0_1px_0_white]">
                            <div class="min-w-0">
                                <div class="text-[12px] font-650 text-[#3a3a3c]">
                                    "自动刷新 Token"
                                </div>
                                <div class="mt-0.5 text-[10.5px] font-500 leading-tight text-[#86868b]">
                                    "401/过期时自动换新；新文件记得导出落盘"
                                </div>
                            </div>
                            <button
                                type="button"
                                role="switch"
                                aria-checked=move || auto_refresh.get().to_string()
                                aria-label="自动刷新 Token"
                                title="开启后，检测到 401/过期时自动用 refresh_token 换新"
                                disabled=move || checking.get()
                                on:click=move |_| auto_refresh.update(|v| *v = !*v)
                                class=move || {
                                    if auto_refresh.get() {
                                        "relative h-[22px] w-[38px] shrink-0 cursor-pointer rounded-full border-0 bg-[#34c759] shadow-[inset_0_0_0_0.5px_rgba(0,0,0,0.06),inset_0_1px_2px_rgba(0,0,0,0.10)] outline-none transition-colors duration-200 focus-visible:ring-2 focus-visible:ring-black/20 disabled:cursor-not-allowed disabled:opacity-45"
                                    } else {
                                        "relative h-[22px] w-[38px] shrink-0 cursor-pointer rounded-full border-0 bg-[#e9e9ea] shadow-[inset_0_0_0_0.5px_rgba(0,0,0,0.10)] outline-none transition-colors duration-200 focus-visible:ring-2 focus-visible:ring-black/20 disabled:cursor-not-allowed disabled:opacity-45"
                                    }
                                }
                            >
                                <span class=move || {
                                    if auto_refresh.get() {
                                        "absolute left-[2px] top-[2px] h-[18px] w-[18px] translate-x-[16px] rounded-full bg-white shadow-[0_2px_6px_rgba(0,0,0,0.18),0_0_0_0.5px_rgba(0,0,0,0.04)] transition-transform duration-200"
                                    } else {
                                        "absolute left-[2px] top-[2px] h-[18px] w-[18px] translate-x-0 rounded-full bg-white shadow-[0_2px_6px_rgba(0,0,0,0.18),0_0_0_0.5px_rgba(0,0,0,0.04)] transition-transform duration-200"
                                    }
                                }></span>
                            </button>
                        </div>

                        // 操作按钮（钉在左栏底部）
                        <div class="flex shrink-0 gap-2">
                            <button
                                class="min-h-10 flex-1 rounded-[12px] border border-black/6 bg-white/58 px-4 text-[13px] font-650 text-[#3a3a3c] shadow-[inset_0_1px_0_white] transition hover:bg-white disabled:cursor-not-allowed disabled:opacity-45"
                                on:click=clear_files
                                disabled=move || checking.get() || selected.get().is_empty()
                            >
                                "清空"
                            </button>
                            <button
                                class="inline-flex min-h-10 flex-[1.6] items-center justify-center gap-2 rounded-[12px] border border-black/6 bg-[#e5e5e8] px-5 text-[13px] font-650 text-[#1d1d1f] shadow-[0_8px_20px_rgba(0,0,0,0.08),inset_0_1px_0_rgba(255,255,255,0.85)] transition hover:-translate-y-0.5 hover:bg-[#dadade] disabled:cursor-not-allowed disabled:opacity-60"
                                on:click=run_check
                                disabled=move || checking.get() || selected.get().is_empty()
                            >
                                <span>
                                    {move || {
                                        if checking.get() { "检测中" } else { "开始检测" }
                                    }}
                                </span>
                                {move || {
                                    if checking.get() {
                                        view! {
                                            <span class="gbq-dots" aria-hidden="true">
                                                <i style="animation-delay:0ms"></i>
                                                <i style="animation-delay:-120ms"></i>
                                                <i style="animation-delay:-840ms"></i>
                                                <i style="animation-delay:-240ms"></i>
                                                <i style="animation-delay:-720ms"></i>
                                                <i style="animation-delay:-360ms"></i>
                                                <i style="animation-delay:-600ms"></i>
                                                <i style="animation-delay:-480ms"></i>
                                            </span>
                                        }
                                            .into_any()
                                    } else {
                                        view! {
                                            <svg
                                                class="h-3.5 w-3.5"
                                                viewBox="0 0 24 24"
                                                fill="none"
                                                stroke="currentColor"
                                                stroke-width="2"
                                                stroke-linecap="round"
                                                stroke-linejoin="round"
                                                aria-hidden="true"
                                            >
                                                <path d="M5 12h14"></path>
                                                <path d="m13 6 6 6-6 6"></path>
                                            </svg>
                                        }
                                            .into_any()
                                    }
                                }}
                            </button>
                        </div>

                        <div class="flex shrink-0 items-center gap-1.5 text-[11px] font-600 text-[#6e6e73]">
                            <svg
                                class="h-3.5 w-3.5 shrink-0"
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="1.8"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                                aria-hidden="true"
                            >
                                <path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"></path>
                                <path d="m9 12 2 2 4-4"></path>
                            </svg>
                            <span>"浏览器本地读取 · Token 不留存"</span>
                        </div>
                    </section>

                    // ─────────── 右栏：额度表格 ───────────
                    <section class="flex min-h-0 flex-col overflow-hidden rounded-[28px] border border-white/95 bg-white/58 p-4 shadow-[0_24px_70px_rgba(0,0,0,0.07),inset_0_1px_0_rgba(255,255,255,0.95)] ring-1 ring-black/4 backdrop-blur-3xl backdrop-saturate-150 sm:p-6 lg:h-full">
                        <div class="flex shrink-0 flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
                            <div>
                                <div class="text-[10px] font-700 tracking-[0.14em] text-[#86868b]">
                                    "02 / QUOTA"
                                </div>
                                <h2 class="mb-0 mt-1.5 text-[20px] font-700 tracking-0">
                                    "额度表格"
                                </h2>
                                <div class="mt-1.5 text-[12px] font-550 text-[#86868b]">
                                    {move || {
                                        summary
                                            .get()
                                            .map(|s| {
                                                if checking.get() {
                                                    format!(
                                                        "检测中 {}/{} · 可用 {} · 耗尽 {} · 其他 {}",
                                                        s.results.len(),
                                                        s.total,
                                                        s.usable,
                                                        s.exhausted,
                                                        s.failed,
                                                    )
                                                } else {
                                                    format!(
                                                        "共 {} · 可用 {} · 耗尽 {} · 其他 {}",
                                                        s.total,
                                                        s.usable,
                                                        s.exhausted,
                                                        s.failed,
                                                    )
                                                }
                                            })
                                            .unwrap_or_else(|| "导入账号后开始检测".into())
                                    }}
                                </div>
                            </div>
                            <Show when=move || summary.get().is_some()>
                                <div class="flex flex-wrap items-center gap-2 self-start sm:self-auto">
                                    <Show when=move || { expired_count() > 0 }>
                                        <button
                                            type="button"
                                            class="inline-flex min-h-10 items-center justify-center gap-2 rounded-[12px] border-0 bg-white/80 px-4 text-[13px] font-650 text-[#3a3a3c] shadow-[0_6px_16px_rgba(0,0,0,0.05),inset_0_1px_0_white] outline-none transition hover:-translate-y-0.5 hover:bg-white disabled:cursor-not-allowed disabled:opacity-45 disabled:hover:translate-y-0"
                                            on:click=refresh_expired
                                            disabled=move || checking.get() || refreshing.get()
                                            title="用 refresh_token 手动换新「Token 过期」的账号"
                                        >
                                            <svg
                                                class=move || {
                                                    if refreshing.get() {
                                                        "h-3.5 w-3.5 animate-spin"
                                                    } else {
                                                        "h-3.5 w-3.5"
                                                    }
                                                }
                                                viewBox="0 0 24 24"
                                                fill="none"
                                                stroke="currentColor"
                                                stroke-width="2"
                                                stroke-linecap="round"
                                                stroke-linejoin="round"
                                                aria-hidden="true"
                                            >
                                                <path d="M21 12a9 9 0 1 1-2.64-6.36"></path>
                                                <path d="M21 3v6h-6"></path>
                                            </svg>
                                            <span>
                                                {move || {
                                                    if refreshing.get() {
                                                        "刷新中".to_string()
                                                    } else {
                                                        format!("刷新 Token · {}", expired_count())
                                                    }
                                                }}
                                            </span>
                                        </button>
                                    </Show>
                                    <button
                                        type="button"
                                        class="inline-flex min-h-10 items-center justify-center gap-2 rounded-[12px] border-0 bg-white/80 px-4 text-[13px] font-650 text-[#3a3a3c] shadow-[0_6px_16px_rgba(0,0,0,0.05),inset_0_1px_0_white] outline-none transition hover:-translate-y-0.5 hover:bg-white disabled:cursor-not-allowed disabled:opacity-45 disabled:hover:translate-y-0"
                                        on:click=export_zip
                                        disabled=move || checking.get()
                                        title=move || {
                                            format!(
                                                "导出当前「{}」筛选下的 auth JSON（含刷新后 token）",
                                                filter.get().export_label()
                                            )
                                        }
                                    >
                                        <svg
                                            class="h-3.5 w-3.5"
                                            viewBox="0 0 24 24"
                                            fill="none"
                                            stroke="currentColor"
                                            stroke-width="1.8"
                                            stroke-linecap="round"
                                            stroke-linejoin="round"
                                            aria-hidden="true"
                                        >
                                            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
                                            <path d="m7 10 5 5 5-5"></path>
                                            <path d="M12 15V3"></path>
                                        </svg>
                                        <span>
                                            {move || {
                                                format!("导出 ZIP（{}）", filter.get().export_label())
                                            }}
                                        </span>
                                    </button>
                                    <button
                                        type="button"
                                        class="inline-flex min-h-10 items-center justify-center gap-2 rounded-[12px] border-0 bg-white/80 px-4 text-[13px] font-650 text-[#3a3a3c] shadow-[0_6px_16px_rgba(0,0,0,0.05),inset_0_1px_0_white] outline-none transition hover:-translate-y-0.5 hover:bg-white disabled:cursor-not-allowed disabled:opacity-45 disabled:hover:translate-y-0"
                                        on:click=copy_image
                                        disabled=move || checking.get()
                                    >
                                        <svg
                                            class="h-3.5 w-3.5"
                                            viewBox="0 0 24 24"
                                            fill="none"
                                            stroke="currentColor"
                                            stroke-width="1.8"
                                            stroke-linecap="round"
                                            stroke-linejoin="round"
                                            aria-hidden="true"
                                        >
                                            <rect width="14" height="14" x="8" y="8" rx="2"></rect>
                                            <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"></path>
                                        </svg>
                                        <span>"复制为图片"</span>
                                    </button>
                                </div>
                            </Show>
                        </div>

                        <Show when=move || copy_msg.get().is_some()>
                            <div class="mt-4 rounded-[14px] border border-[#c6e5d2] bg-[#f1fbf5] px-4 py-3 text-[13px] font-600 text-[#26734d]">
                                {move || copy_msg.get().unwrap_or_default()}
                            </div>
                        </Show>

                        // 汇总统计
                        <Show when=move || summary.get().is_some()>
                            <div class="mt-5 grid shrink-0 grid-cols-2 overflow-hidden rounded-[18px] bg-[#f2f2f4]/80 ring-1 ring-black/[0.03] sm:grid-cols-4">
                                <div class="border-b border-r border-black/[0.04] p-3.5 sm:border-b-0">
                                    <div class="text-[11px] font-600 text-[#86868b]">
                                        "总账号"
                                    </div>
                                    <div class="mt-2 font-mono text-[22px] font-700 leading-none">
                                        {move || summary.get().map(|s| s.total).unwrap_or_default()}
                                    </div>
                                </div>
                                <div class="border-b border-black/[0.04] p-3.5 sm:border-b-0 sm:border-r">
                                    <div class="text-[11px] font-600 text-[#26734d]">"可用"</div>
                                    <div class="mt-2 font-mono text-[22px] font-700 leading-none">
                                        {move || {
                                            summary.get().map(|s| s.usable).unwrap_or_default()
                                        }}
                                    </div>
                                </div>
                                <div class="border-r border-black/[0.04] p-3.5">
                                    <div class="text-[11px] font-600 text-[#8a5b12]">
                                        "额度耗尽"
                                    </div>
                                    <div class="mt-2 font-mono text-[22px] font-700 leading-none">
                                        {move || {
                                            summary.get().map(|s| s.exhausted).unwrap_or_default()
                                        }}
                                    </div>
                                </div>
                                <div class="p-3.5">
                                    <div class="text-[11px] font-600 text-[#a33d43]">"其他"</div>
                                    <div class="mt-2 font-mono text-[22px] font-700 leading-none">
                                        {move || {
                                            summary.get().map(|s| s.failed).unwrap_or_default()
                                        }}
                                    </div>
                                </div>
                            </div>
                        </Show>

                        // 状态筛选（仅有结果时）
                        <Show when=move || summary.get().is_some()>
                            <div class="mt-4 flex shrink-0 gap-1 rounded-[12px] border-0 bg-[#e9e9ec]/75 p-1 shadow-[inset_0_1px_2px_rgba(0,0,0,0.04)]">
                                {[
                                    (StatusFilter::All, "全部"),
                                    (StatusFilter::Usable, "可用"),
                                    (StatusFilter::Exhausted, "耗尽"),
                                    (StatusFilter::Other, "其他"),
                                ]
                                    .into_iter()
                                    .map(|(f, label)| {
                                        let count = move || {
                                            summary
                                                .get()
                                                .map(|s| s.results.iter().filter(|r| f.matches(r)).count())
                                                .unwrap_or(0)
                                        };
                                        view! {
                                            <button
                                                type="button"
                                                class=move || seg_class(filter.get() == f)
                                                on:click=move |_| filter.set(f)
                                            >
                                                <span>{label}</span>
                                                <span class="font-mono text-[10px] font-650 text-[#86868b]">
                                                    {count}
                                                </span>
                                            </button>
                                        }
                                    })
                                    .collect_view()}
                            </div>
                        </Show>

                        // 额度表格 / 空状态：圆角 well + 淡出 + sticky 表头
                        <div
                            class="relative mt-4 flex min-h-0 flex-1 flex-col overflow-hidden"
                            id="results-table"
                        >
                            <Show
                                when=move || summary.get().is_some()
                                fallback=|| {
                                    view! {
                                        <div class="grid min-h-0 flex-1 place-items-center rounded-[18px] border border-dashed border-black/[0.06] bg-[#f7f7f8]/40 px-6 text-center shadow-[inset_0_1px_0_white]">
                                            <div>
                                                <div
                                                    class="mx-auto grid h-12 w-12 place-items-center rounded-[16px] border-0 bg-white/70 text-[#c7c7cc] shadow-[inset_0_1px_0_white,0_4px_14px_rgba(0,0,0,0.04)]"
                                                    aria-hidden="true"
                                                >
                                                    <svg
                                                        class="h-6 w-6"
                                                        viewBox="0 0 24 24"
                                                        fill="none"
                                                        stroke="currentColor"
                                                        stroke-width="1.6"
                                                        stroke-linecap="round"
                                                        stroke-linejoin="round"
                                                    >
                                                        <path d="M3 3v18h18"></path>
                                                        <path d="m19 9-5 5-4-4-3 3"></path>
                                                    </svg>
                                                </div>
                                                <div class="mt-3 text-[14px] font-650 text-[#3a3a3c]">
                                                    "暂无额度数据"
                                                </div>
                                                <div class="mt-1 text-[12px] text-[#86868b]">
                                                    "在左侧导入 auth 文件并点击开始检测"
                                                </div>
                                            </div>
                                        </div>
                                    }
                                }
                            >
                                <div class="relative flex min-h-0 flex-1 flex-col overflow-hidden rounded-[16px] bg-white/55 shadow-[inset_0_1px_0_rgba(255,255,255,0.95)] ring-1 ring-black/[0.04]">
                                    <div class="hidden shrink-0 grid-cols-[minmax(0,1.2fr)_72px_100px_minmax(0,1.35fr)] items-center gap-3 rounded-t-[16px] border-b border-black/[0.05] bg-[#f5f5f7]/92 px-3.5 py-2 text-[10px] font-700 tracking-[0.12em] text-[#8e8e93] backdrop-blur-md sm:grid">
                                        <div>"账号"</div>
                                        <div>"类型"</div>
                                        <div>"状态"</div>
                                        <div>"额度用量"</div>
                                    </div>
                                    <div
                                        class="pointer-events-none absolute inset-x-0 bottom-0 z-10 h-5 rounded-b-[16px] bg-gradient-to-t from-white/90 to-transparent"
                                        aria-hidden="true"
                                    ></div>
                                    <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain [scrollbar-color:rgba(60,60,67,0.18)_transparent] [scrollbar-width:thin]">
                                        <For
                                            each=move || {
                                                let f = filter.get();
                                                let Some(s) = summary.get() else {
                                                    return Vec::new();
                                                };
                                                s.results
                                                    .into_iter()
                                                    .filter(|r| f.matches(r))
                                                    .collect::<Vec<_>>()
                                                    .into_iter()
                                                    .enumerate()
                                                    .collect::<Vec<_>>()
                                            }
                                            key=|item: &(usize, CheckResult)| {
                                                format!("{}:{}", item.1.filename, item.1.status_label)
                                            }
                                            children=move |(row_idx, r): (usize, CheckResult)| {
                                                let pill_class = status_pill_class(&r.status);
                                                let plan_class = plan_pill_class(&r.plan);
                                                let bar = quota_bar(&r);
                                                let pct_label = bar
                                                    .map(|b| format!("{:.0}%", b.pct))
                                                    .unwrap_or_else(|| "--".into());
                                                let lit = bar.map(|b| lit_segments(b.pct)).unwrap_or(0);
                                                let fill = bar.map(|b| b.fill_class).unwrap_or_default();
                                                let has_bar = bar.is_some();
                                                let detail = r.detail.clone().unwrap_or_default();
                                                let show_detail = !detail.is_empty();
                                                let refreshed = r.refreshed;
                                                let plan_label = r.plan_label.clone();
                                                let quota_text = quota_display(&r);
                                                let fname_dl = r.filename.clone();
                                                let fname_retry = r.filename.clone();
                                                let fname_spin_a = r.filename.clone();
                                                let fname_spin_b = r.filename.clone();
                                                let fname_spin_c = r.filename.clone();
                                                let fname_spin_d = r.filename.clone();
                                                let can_retry = matches!(
                                                    r.status,
                                                    AccountStatus::NetworkError
                                                        | AccountStatus::Error
                                                        | AccountStatus::AuthFailed
                                                        | AccountStatus::RefreshFailed
                                                        | AccountStatus::Expired
                                                        | AccountStatus::RateLimited
                                                );
                                                // 显式拷贝 Copy 闭包，避免 For children 被推断成 FnOnce
                                                let on_retry = retry_one;
                                                let on_download = download_one;
                                                view! {
                                                    <div
                                                        class="group/row relative grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 border-b border-black/[0.04] px-3.5 py-2 transition last:border-b-0 hover:bg-[#f2f2f7]/75 sm:grid-cols-[minmax(0,1.2fr)_72px_100px_minmax(0,1.35fr)] sm:gap-3"
                                                        data-filename=r.filename.clone()
                                                    >
                                                        <div class="min-w-0">
                                                            <div class="flex min-w-0 items-center gap-1.5">
                                                                <div
                                                                    class="truncate text-[12.5px] font-650 tracking-[-0.01em] text-[#1d1d1f]"
                                                                    title=r.account.clone()
                                                                >
                                                                    {r.account.clone()}
                                                                </div>
                                                                <Show when=move || refreshed>
                                                                    <span class="shrink-0 rounded-full bg-[#34c759]/14 px-1.5 py-0.5 text-[9px] font-700 tracking-[0.04em] text-[#248a3d]">
                                                                        "已刷新"
                                                                    </span>
                                                                </Show>
                                                            </div>
                                                            <div class="mt-1 sm:hidden">
                                                                <span class=plan_class>{plan_label.clone()}</span>
                                                            </div>
                                                        </div>
                                                        <div class="hidden sm:block">
                                                            <span class=plan_class>{plan_label}</span>
                                                        </div>
                                                        <div class="justify-self-end sm:justify-self-start">
                                                            <span class="group relative inline-flex">
                                                                <span class=pill_class>{r.status_label.clone()}</span>
                                                                <Show when=move || show_detail>
                                                                    <span
                                                                        // 首行浮框向下弹：向上会被滚动容器顶边 / 表头裁剪
                                                                        class=if row_idx == 0 {
                                                                            "pointer-events-none absolute left-1/2 top-[calc(100%+8px)] z-30 w-max max-w-[240px] -translate-x-1/2 scale-95 rounded-[10px] bg-[rgba(30,30,30,0.92)] px-2.5 py-1.5 text-left text-[11px] font-500 leading-snug tracking-[-0.01em] text-white opacity-0 shadow-[0_8px_28px_rgba(0,0,0,0.22),0_0_0_0.5px_rgba(255,255,255,0.08)] backdrop-blur-xl transition duration-150 group-hover:scale-100 group-hover:opacity-100"
                                                                        } else {
                                                                            "pointer-events-none absolute bottom-[calc(100%+8px)] left-1/2 z-30 w-max max-w-[240px] -translate-x-1/2 scale-95 rounded-[10px] bg-[rgba(30,30,30,0.92)] px-2.5 py-1.5 text-left text-[11px] font-500 leading-snug tracking-[-0.01em] text-white opacity-0 shadow-[0_8px_28px_rgba(0,0,0,0.22),0_0_0_0.5px_rgba(255,255,255,0.08)] backdrop-blur-xl transition duration-150 group-hover:scale-100 group-hover:opacity-100"
                                                                        }
                                                                        role="tooltip"
                                                                    >
                                                                        {detail.clone()}
                                                                        <span class=if row_idx == 0 {
                                                                            "absolute bottom-full left-1/2 h-0 w-0 -translate-x-1/2 border-x-[5px] border-b-[6px] border-x-transparent border-b-[rgba(30,30,30,0.92)]"
                                                                        } else {
                                                                            "absolute left-1/2 top-full h-0 w-0 -translate-x-1/2 border-x-[5px] border-t-[6px] border-x-transparent border-t-[rgba(30,30,30,0.92)]"
                                                                        }></span>
                                                                    </span>
                                                                </Show>
                                                            </span>
                                                        </div>
                                                        <div class="col-span-2 min-w-0 pr-20 sm:col-span-1">
                                                            {if has_bar {
                                                                view! {
                                                                    <div class="min-w-0">
                                                                        <div class="flex items-baseline justify-between gap-2 font-mono text-[11px] tabular-nums">
                                                                            <span class="min-w-0 truncate font-650 text-[#3a3a3c]">
                                                                                {quota_text.clone()}
                                                                            </span>
                                                                            <span class="shrink-0 text-[#8e8e93]">{pct_label}</span>
                                                                        </div>
                                                                        <div
                                                                            class="mt-1.5 flex w-full items-center gap-[3px]"
                                                                            aria-hidden="true"
                                                                        >
                                                                            {(0..QUOTA_SEGMENTS)
                                                                                .map(|i| {
                                                                                    let on = i < lit;
                                                                                    let cls = if on {
                                                                                        format!("h-[5px] min-w-0 flex-1 rounded-full {fill}")
                                                                                    } else {
                                                                                        "h-[5px] min-w-0 flex-1 rounded-full bg-black/[0.08]"
                                                                                            .to_string()
                                                                                    };
                                                                                    view! { <span class=cls></span> }
                                                                                })
                                                                                .collect_view()}
                                                                        </div>
                                                                    </div>
                                                                }
                                                                    .into_any()
                                                            } else {
                                                                view! {
                                                                    <span class="font-mono text-[11px] font-650 text-[#aeaeb2]">
                                                                        {quota_text.clone()}
                                                                    </span>
                                                                }
                                                                    .into_any()
                                                            }}
                                                        </div>
                                                        <div class="absolute right-2 top-1/2 z-10 flex -translate-y-1/2 items-center gap-1">
                                                            {can_retry.then(|| {
                                                                let fname = fname_retry.clone();
                                                                view! {
                                                                    <button
                                                                        type="button"
                                                                        class="grid h-6 w-6 place-items-center rounded-full border-0 bg-black/[0.045] p-0 text-[#6e6e73] opacity-100 outline-none transition hover:bg-black/[0.09] hover:text-[#1d1d1f] focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-black/20 disabled:cursor-not-allowed disabled:opacity-40 sm:opacity-0 sm:group-hover/row:opacity-100"
                                                                        class:opacity-100=move || {
                                                                            retrying
                                                                                .get()
                                                                                .as_ref()
                                                                                .is_some_and(|n| n == &fname_spin_a)
                                                                        }
                                                                        title=move || {
                                                                            if retrying
                                                                                .get()
                                                                                .as_ref()
                                                                                .is_some_and(|n| n == &fname_spin_b)
                                                                            {
                                                                                "重试中…".to_string()
                                                                            } else {
                                                                                "重试此账号".to_string()
                                                                            }
                                                                        }
                                                                        aria-label="重试此账号"
                                                                        disabled=move || {
                                                                            retrying
                                                                                .get()
                                                                                .as_ref()
                                                                                .is_some_and(|n| n == &fname_spin_c)
                                                                                || checking.get()
                                                                                || refreshing.get()
                                                                        }
                                                                        on:click=move |_| on_retry(fname.clone())
                                                                    >
                                                                        <svg
                                                                            class=move || {
                                                                                if retrying
                                                                                    .get()
                                                                                    .as_ref()
                                                                                    .is_some_and(|n| n == &fname_spin_d)
                                                                                {
                                                                                    "h-3 w-3 animate-spin"
                                                                                } else {
                                                                                    "h-3 w-3"
                                                                                }
                                                                            }
                                                                            viewBox="0 0 24 24"
                                                                            fill="none"
                                                                            stroke="currentColor"
                                                                            stroke-width="2.2"
                                                                            stroke-linecap="round"
                                                                            stroke-linejoin="round"
                                                                            aria-hidden="true"
                                                                        >
                                                                            <path d="M21 12a9 9 0 1 1-2.64-6.36"></path>
                                                                            <path d="M21 3v6h-6"></path>
                                                                        </svg>
                                                                    </button>
                                                                }
                                                            })}
                                                            <button
                                                                type="button"
                                                                class="grid h-6 w-6 place-items-center rounded-full border-0 bg-black/[0.045] p-0 text-[#6e6e73] opacity-100 outline-none transition hover:bg-black/[0.09] hover:text-[#1d1d1f] focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-black/20 sm:opacity-0 sm:group-hover/row:opacity-100"
                                                                title="下载该账号的 auth JSON（含刷新后 token）"
                                                                aria-label="下载此账号文件"
                                                                on:click=move |_| on_download(fname_dl.clone())
                                                            >
                                                                <svg
                                                                    class="h-3 w-3"
                                                                    viewBox="0 0 24 24"
                                                                    fill="none"
                                                                    stroke="currentColor"
                                                                    stroke-width="2.2"
                                                                    stroke-linecap="round"
                                                                    stroke-linejoin="round"
                                                                    aria-hidden="true"
                                                                >
                                                                    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
                                                                    <path d="m7 10 5 5 5-5"></path>
                                                                    <path d="M12 15V3"></path>
                                                                </svg>
                                                            </button>
                                                        </div>
                                                    </div>
                                                }
                                            }
                                        />
                                    </div>
                                </div>
                            </Show>
                        </div>
                    </section>
                </div>
                <footer class="flex shrink-0 flex-wrap items-center justify-center gap-x-2 gap-y-1 px-3 pb-0.5 pt-1 text-center text-[10px] text-[#86868b]">
                    <span>"POST /v1/responses 探针"</span>
                    <span class="h-0.5 w-0.5 rounded-full bg-[#aeaeb2]"></span>
                    <span>"账号 / 类型 / 状态 / 额度"</span>
                </footer>
            </div>
        </div>
    }
}

#[component]
fn NotFound() -> impl IntoView {
    #[cfg(feature = "ssr")]
    {
        let resp = expect_context::<leptos_actix::ResponseOptions>();
        resp.set_status(actix_web::http::StatusCode::NOT_FOUND);
    }

    view! {
        <div class="grid min-h-screen place-items-center bg-[#f5f5f7] p-6 font-sans text-[#1d1d1f]">
            <div class="rounded-[24px] border border-white bg-white/62 px-10 py-14 text-center shadow-[0_24px_70px_rgba(0,0,0,0.07)] ring-1 ring-black/4 backdrop-blur-3xl">
                <div class="text-[11px] font-700 tracking-[0.14em] text-[#86868b]">"404"</div>
                <h1 class="mb-0 mt-2 text-[28px] font-700 tracking-0">"页面不存在"</h1>
            </div>
        </div>
    }
}

async fn read_file_text(file: &File) -> Result<String, String> {
    let js_val = JsFuture::from(file.text())
        .await
        .map_err(|_| "读取文件失败".to_string())?;
    js_val
        .as_string()
        .ok_or_else(|| "文件内容不是文本".to_string())
}

fn extract_email(content: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(content).ok()?;
    let email = v.get("email")?.as_str()?.trim();
    if email.is_empty() {
        None
    } else {
        Some(email.to_string())
    }
}

async fn export_results_image(
    results: &[CheckResult],
    emails: &HashMap<String, String>,
) -> Result<String, String> {
    let window = window().ok_or_else(|| "window unavailable".to_string())?;
    let document = window
        .document()
        .ok_or_else(|| "document unavailable".to_string())?;

    let canvas = document
        .create_element("canvas")
        .map_err(|_| "create canvas failed".to_string())?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| "canvas cast failed".to_string())?;

    let row_h = 54.0;
    let header_h = 86.0;
    let pad_x = 36.0;
    let width = 1080.0;
    let height = header_h + 48.0 + row_h * results.len().max(1) as f64 + 36.0;
    canvas.set_width(width as u32);
    canvas.set_height(height as u32);

    let ctx = canvas
        .get_context("2d")
        .map_err(|_| "2d context failed".to_string())?
        .ok_or_else(|| "2d context missing".to_string())?
        .dyn_into::<web_sys::CanvasRenderingContext2d>()
        .map_err(|_| "2d cast failed".to_string())?;

    ctx.set_fill_style_str("#f4f1ea");
    ctx.fill_rect(0.0, 0.0, width, height);

    round_rect(&ctx, 18.0, 18.0, width - 36.0, height - 36.0, 28.0);
    ctx.set_fill_style_str("#fffaf3");
    ctx.fill();
    ctx.set_stroke_style_str("rgba(60,48,30,0.08)");
    ctx.set_line_width(1.0);
    ctx.stroke();

    ctx.set_fill_style_str("#1d1a16");
    ctx.set_font("700 28px SF Pro Display, -apple-system, BlinkMacSystemFont, sans-serif");
    let _ = ctx.fill_text("Grok Build 额度检测结果", pad_x, 58.0);

    ctx.set_fill_style_str("rgba(29,26,22,0.55)");
    ctx.set_font("500 14px SF Pro Text, -apple-system, BlinkMacSystemFont, sans-serif");
    let usable = results.iter().filter(|r| r.usable).count();
    let _ = ctx.fill_text(
        &format!(
            "共 {} 个账号 · 可用 {} · 生成自 grok-build-quota",
            results.len(),
            usable
        ),
        pad_x,
        84.0,
    );

    let y0 = header_h + 18.0;
    ctx.set_fill_style_str("rgba(29,26,22,0.08)");
    round_rect(&ctx, 28.0, y0, width - 56.0, 40.0, 14.0);
    ctx.fill();

    ctx.set_fill_style_str("rgba(29,26,22,0.55)");
    ctx.set_font("700 12px SF Pro Text, -apple-system, BlinkMacSystemFont, sans-serif");
    let _ = ctx.fill_text("账号", pad_x, y0 + 25.0);
    let _ = ctx.fill_text("类型", 450.0, y0 + 25.0);
    let _ = ctx.fill_text("状态", 560.0, y0 + 25.0);
    let _ = ctx.fill_text("额度用量", 700.0, y0 + 25.0);

    for (idx, item) in results.iter().enumerate() {
        let y = y0 + 48.0 + idx as f64 * row_h;

        if idx % 2 == 0 {
            ctx.set_fill_style_str("rgba(29,26,22,0.03)");
            round_rect(&ctx, 28.0, y, width - 56.0, row_h - 4.0, 12.0);
            ctx.fill();
        }

        let account = emails
            .get(&item.filename)
            .cloned()
            .unwrap_or_else(|| item.account.clone());
        ctx.set_fill_style_str("#1d1d1f");
        ctx.set_font("650 14px SF Pro Text, -apple-system, BlinkMacSystemFont, sans-serif");
        let _ = ctx.fill_text(&account, pad_x, y + 32.0);

        let (plan_bg, plan_fg) = match item.plan {
            AccountPlan::Free => ("rgba(142,142,147,0.14)", "#636366"),
            AccountPlan::SuperGrokLite => ("rgba(88,86,214,0.10)", "#3634a3"),
            AccountPlan::SuperGrok => ("rgba(88,86,214,0.14)", "#3634a3"),
            AccountPlan::SuperGrokHeavy => ("rgba(175,82,222,0.14)", "#7b2fad"),
            AccountPlan::PaidOther => ("rgba(255,159,10,0.14)", "#9a6700"),
            AccountPlan::Unknown => ("rgba(0,0,0,0.05)", "#8e8e93"),
        };
        let plan_w = 56.0;
        round_rect(&ctx, 450.0, y + 17.0, plan_w, 20.0, 10.0);
        ctx.set_fill_style_str(plan_bg);
        ctx.fill();
        ctx.set_fill_style_str(plan_fg);
        ctx.set_font("650 11px SF Pro Text, -apple-system, BlinkMacSystemFont, sans-serif");
        ctx.set_text_align("center");
        let _ = ctx.fill_text(&item.plan_label, 450.0 + plan_w / 2.0, y + 31.0);
        ctx.set_text_align("left");

        let (pill_bg, pill_fg) = match item.status {
            AccountStatus::Ok => ("rgba(52,199,89,0.14)", "#248a3d"),
            AccountStatus::Exhausted => ("rgba(255,159,10,0.16)", "#9a6700"),
            AccountStatus::RateLimited => ("rgba(255,159,10,0.14)", "#c93400"),
            AccountStatus::SpendingLimited => ("rgba(255,159,10,0.16)", "#9a6700"),
            AccountStatus::RefreshFailed => ("rgba(255,69,58,0.12)", "#d70015"),
            AccountStatus::ChatDenied => ("rgba(175,82,222,0.14)", "#7b2fad"),
            AccountStatus::Expired => ("rgba(255,159,10,0.16)", "#9a6700"),
            AccountStatus::NetworkError => ("rgba(142,142,147,0.14)", "#636366"),
            _ => ("rgba(255,69,58,0.12)", "#d70015"),
        };
        // 四字标签略加宽，与 UI tag 一致
        let pill_w = if item.status_label.chars().count() >= 4 {
            86.0
        } else {
            76.0
        };
        round_rect(&ctx, 560.0, y + 17.0, pill_w, 20.0, 10.0);
        ctx.set_fill_style_str(pill_bg);
        ctx.fill();
        ctx.set_fill_style_str(pill_fg);
        ctx.set_font("650 11px SF Pro Text, -apple-system, BlinkMacSystemFont, sans-serif");
        ctx.set_text_align("center");
        let _ = ctx.fill_text(&item.status_label, 560.0 + pill_w / 2.0, y + 31.0);
        ctx.set_text_align("left");

        if let Some(bar) = quota_bar(item) {
            let quota_line = quota_display(item);
            ctx.set_fill_style_str("#3a3a3c");
            ctx.set_font("650 12px SF Mono, ui-monospace, Menlo, monospace");
            let _ = ctx.fill_text(&quota_line, 700.0, y + 24.0);
            ctx.set_fill_style_str("#8e8e93");
            ctx.set_text_align("right");
            let _ = ctx.fill_text(&format!("{:.0}%", bar.pct), 1032.0, y + 24.0);
            ctx.set_text_align("left");

            let lit = lit_segments(bar.pct);
            let seg_gap = 3.0;
            let seg_w = (332.0 - seg_gap * (QUOTA_SEGMENTS as f64 - 1.0)) / QUOTA_SEGMENTS as f64;
            for i in 0..QUOTA_SEGMENTS {
                let x = 700.0 + i as f64 * (seg_w + seg_gap);
                round_rect(&ctx, x, y + 30.0, seg_w, 5.0, 2.5);
                if i < lit {
                    ctx.set_fill_style_str(bar.hex);
                } else {
                    ctx.set_fill_style_str("rgba(0,0,0,0.08)");
                }
                ctx.fill();
            }
        } else {
            ctx.set_fill_style_str("#aeaeb2");
            ctx.set_font("650 12px SF Mono, ui-monospace, Menlo, monospace");
            let _ = ctx.fill_text(&quota_display(item), 700.0, y + 33.0);
        }
    }

    let blob = canvas_to_png_blob(&canvas).await?;
    match copy_blob_to_clipboard(&blob).await {
        Ok(()) => Ok("已复制检测结果图片到剪贴板".into()),
        Err(msg) => {
            download_blob(&blob, "grok-build-quota.png")?;
            Ok(msg)
        }
    }
}

fn round_rect(ctx: &web_sys::CanvasRenderingContext2d, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0);
    ctx.begin_path();
    ctx.move_to(x + r, y);
    let _ = ctx.arc_to(x + w, y, x + w, y + h, r);
    let _ = ctx.arc_to(x + w, y + h, x, y + h, r);
    let _ = ctx.arc_to(x, y + h, x, y, r);
    let _ = ctx.arc_to(x, y, x + w, y, r);
    ctx.close_path();
}

async fn canvas_to_png_blob(canvas: &web_sys::HtmlCanvasElement) -> Result<Blob, String> {
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let cb = Closure::once(move |blob: Option<Blob>| {
            if let Some(blob) = blob {
                let _ = resolve.call1(&wasm_bindgen::JsValue::NULL, &blob);
            } else {
                let _ = reject.call1(
                    &wasm_bindgen::JsValue::NULL,
                    &wasm_bindgen::JsValue::from_str("toBlob failed"),
                );
            }
        });
        let _ = canvas.to_blob(cb.as_ref().unchecked_ref());
        cb.forget();
    });
    let value = JsFuture::from(promise)
        .await
        .map_err(|_| "导出 PNG 失败".to_string())?;
    value
        .dyn_into::<Blob>()
        .map_err(|_| "PNG blob cast failed".to_string())
}

async fn copy_blob_to_clipboard(blob: &Blob) -> Result<(), String> {
    let window = window().ok_or_else(|| "window unavailable".to_string())?;
    let clipboard = window.navigator().clipboard();

    let item_obj = Object::new();
    Reflect::set(&item_obj, &"image/png".into(), blob)
        .map_err(|_| "ClipboardItem payload failed".to_string())?;

    let item = ClipboardItem::new_with_record_from_str_to_blob_promise(&item_obj)
        .map_err(|_| "ClipboardItem unsupported".to_string())?;

    let items = Array::new();
    items.push(&item);
    JsFuture::from(clipboard.write(&items.into()))
        .await
        .map_err(|_| "当前环境无法写剪贴板，已改为下载 PNG".to_string())?;
    Ok(())
}
fn zip_entry_name(name: &str) -> String {
    name.rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .trim()
        .to_string()
}

fn build_auth_zip(files: &[(String, String)]) -> Result<Vec<u8>, String> {
    use std::io::{Cursor, Write};

    use zip::{CompressionMethod, write::SimpleFileOptions};

    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, content) in files {
            let entry = zip_entry_name(name);
            if entry.is_empty() {
                continue;
            }
            zip.start_file(&entry, opts)
                .map_err(|e| format!("zip start_file failed: {e}"))?;
            zip.write_all(content.as_bytes())
                .map_err(|e| format!("zip write failed: {e}"))?;
        }
        zip.finish()
            .map_err(|e| format!("zip finish failed: {e}"))?;
    }
    Ok(cursor.into_inner())
}

fn bytes_to_zip_blob(bytes: &[u8]) -> Result<Blob, String> {
    let u8arr = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
    u8arr.copy_from(bytes);
    let parts = Array::new();
    parts.push(&u8arr);
    let props = web_sys::BlobPropertyBag::new();
    props.set_type("application/zip");
    Blob::new_with_u8_array_sequence_and_options(&parts, &props)
        .map_err(|_| "创建 ZIP Blob 失败".to_string())
}

fn is_abort_error(err: &wasm_bindgen::JsValue) -> bool {
    err.dyn_ref::<js_sys::Error>()
        .map(|e| e.name() == "AbortError")
        .unwrap_or(false)
}

/// Chrome File System Access API: showSaveFilePicker + createWritable。
/// 用户取消 AbortError 不回退下载；API 不可用或写入失败时 fallback 到 a[download]。
async fn save_blob_picker_or_download(blob: &Blob, suggested_name: &str) -> Result<String, String> {
    let window = window().ok_or_else(|| "window unavailable".to_string())?;

    let picker = Reflect::get(&window, &"showSaveFilePicker".into())
        .ok()
        .and_then(|v| v.dyn_into::<js_sys::Function>().ok());

    if let Some(picker) = picker {
        let opts = Object::new();
        let _ = Reflect::set(&opts, &"suggestedName".into(), &suggested_name.into());

        let accept = Object::new();
        let exts = Array::of1(&".zip".into());
        let _ = Reflect::set(&accept, &"application/zip".into(), &exts);

        let type_obj = Object::new();
        let _ = Reflect::set(&type_obj, &"description".into(), &"ZIP archive".into());
        let _ = Reflect::set(&type_obj, &"accept".into(), &accept);

        let types = Array::of1(&type_obj);
        let _ = Reflect::set(&opts, &"types".into(), &types);

        match JsFuture::from(js_sys::Promise::resolve(
            &picker
                .call1(&window, &opts)
                .map_err(|_| "showSaveFilePicker 调用失败".to_string())?,
        ))
        .await
        {
            Ok(handle) => {
                let create_writable = Reflect::get(&handle, &"createWritable".into())
                    .ok()
                    .and_then(|v| v.dyn_into::<js_sys::Function>().ok())
                    .ok_or_else(|| "createWritable 不可用".to_string())?;
                let writable = JsFuture::from(js_sys::Promise::resolve(
                    &create_writable
                        .call0(&handle)
                        .map_err(|_| "createWritable 调用失败".to_string())?,
                ))
                .await
                .map_err(|_| "打开写入流失败".to_string())?;

                let write = Reflect::get(&writable, &"write".into())
                    .ok()
                    .and_then(|v| v.dyn_into::<js_sys::Function>().ok())
                    .ok_or_else(|| "writable.write 不可用".to_string())?;
                JsFuture::from(js_sys::Promise::resolve(
                    &write
                        .call1(&writable, blob)
                        .map_err(|_| "写入 ZIP 失败".to_string())?,
                ))
                .await
                .map_err(|_| "写入 ZIP 失败".to_string())?;

                let close = Reflect::get(&writable, &"close".into())
                    .ok()
                    .and_then(|v| v.dyn_into::<js_sys::Function>().ok())
                    .ok_or_else(|| "writable.close 不可用".to_string())?;
                JsFuture::from(js_sys::Promise::resolve(
                    &close
                        .call0(&writable)
                        .map_err(|_| "关闭写入流失败".to_string())?,
                ))
                .await
                .map_err(|_| "关闭写入流失败".to_string())?;

                return Ok(format!("已保存 ZIP：{suggested_name}"));
            }
            Err(err) if is_abort_error(&err) => {
                return Ok("已取消导出".into());
            }
            Err(_) => {
                // 权限/API 失败：fallback 下载
            }
        }
    }

    download_blob(blob, suggested_name)?;
    Ok(format!(
        "已下载 ZIP：{suggested_name}（浏览器不支持或未授权文件写入）"
    ))
}

/// 文本内容打包成 application/json Blob（逐行下载用）
fn json_download_blob(content: &str) -> Result<Blob, String> {
    let parts = Array::of1(&JsValue::from_str(content));
    let props = web_sys::BlobPropertyBag::new();
    props.set_type("application/json");
    Blob::new_with_str_sequence_and_options(&parts, &props)
        .map_err(|_| "创建下载文件失败".to_string())
}

fn download_blob(blob: &Blob, filename: &str) -> Result<(), String> {
    let window = window().ok_or_else(|| "window unavailable".to_string())?;
    let document = window
        .document()
        .ok_or_else(|| "document unavailable".to_string())?;
    let url =
        Url::create_object_url_with_blob(blob).map_err(|_| "object url failed".to_string())?;
    let anchor = document
        .create_element("a")
        .map_err(|_| "anchor failed".to_string())?;
    let _ = anchor.set_attribute("href", &url);
    let _ = anchor.set_attribute("download", filename);
    let body = document.body().ok_or_else(|| "body unavailable".to_string())?;
    body.append_child(&anchor)
        .map_err(|_| "anchor attach failed".to_string())?;
    if let Some(el) = anchor.dyn_ref::<web_sys::HtmlElement>() {
        el.click();
    }
    // 延迟回收：同步 remove/revoke 会掐死尚未启动的下载（Safari 必现、Chrome 偶发）
    let cleanup = Closure::once(move || {
        if let Some(node) = anchor.dyn_ref::<web_sys::Node>() {
            let _ = body.remove_child(node);
        }
        let _ = Url::revoke_object_url(&url);
    });
    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
        cleanup.as_ref().unchecked_ref(),
        30_000,
    );
    cleanup.forget();
    Ok(())
}
