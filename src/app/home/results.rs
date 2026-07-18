use leptos::prelude::*;

use super::{
    super::style::{
        QUOTA_SEGMENTS, lit_segments, plan_pill_class, quota_bar, quota_display, seg_class,
        status_pill_class,
    },
    StatusFilter,
};
use crate::check::{AccountStatus, CheckResult, CheckSummary};

#[component]
pub(super) fn ResultsPanel(
    summary: RwSignal<Option<CheckSummary>>,
    checking: RwSignal<bool>,
    refreshing: RwSignal<bool>,
    retrying: RwSignal<Option<String>>,
    copy_msg: RwSignal<Option<String>>,
    filter: RwSignal<StatusFilter>,
    on_refresh: Callback<()>,
    on_export: Callback<()>,
    on_copy_image: Callback<()>,
    on_retry: Callback<String>,
    on_download: Callback<String>,
) -> impl IntoView {
    let refreshable_count = move || {
        summary
            .get()
            .map(|s| {
                s.results
                    .iter()
                    .filter(|r| {
                        matches!(
                            r.status,
                            AccountStatus::Expired
                                | AccountStatus::RefreshFailed
                                | AccountStatus::NetworkError
                        )
                    })
                    .count()
            })
            .unwrap_or(0)
    };

    view! {
        <section class="gbq-panel flex min-h-0 flex-col overflow-hidden rounded-[28px] p-4 sm:p-6 lg:h-full">
            <div class="flex shrink-0 flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
                <div>
                    <div class="text-[10px] font-700 tracking-[0.14em] text-[#86868b]">
                        "02 / QUOTA"
                    </div>
                    <h2 class="mb-0 mt-1.5 text-[20px] font-700 tracking-0">"额度表格"</h2>
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
                        <button
                            type="button"
                            class="gbq-button inline-flex min-h-10 items-center justify-center gap-2 rounded-[12px] px-4 text-[13px] font-650 outline-none disabled:cursor-not-allowed disabled:opacity-45"
                            on:click=move |_| on_refresh.run(())
                            disabled=move || {
                                checking.get() || refreshing.get() || refreshable_count() == 0
                            }
                            title="用 refresh_token 换新：Token 过期 / 刷新失败 / 网络错误"
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
                                        let n = refreshable_count();
                                        if n > 0 {
                                            format!("刷新 Token · {n}")
                                        } else {
                                            "刷新 Token".into()
                                        }
                                    }
                                }}
                            </span>
                        </button>
                        <button
                            type="button"
                            class="gbq-button inline-flex min-h-10 items-center justify-center gap-2 rounded-[12px] px-4 text-[13px] font-650 outline-none disabled:cursor-not-allowed disabled:opacity-45"
                            on:click=move |_| on_export.run(())
                            disabled=move || checking.get()
                            title=move || {
                                format!(
                                    "导出当前「{}」筛选下的 auth JSON（含刷新后 token）",
                                    filter.get().export_label(),
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
                            class="gbq-button inline-flex min-h-10 items-center justify-center gap-2 rounded-[12px] px-4 text-[13px] font-650 outline-none disabled:cursor-not-allowed disabled:opacity-45"
                            on:click=move |_| on_copy_image.run(())
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
                        <div class="text-[11px] font-600 text-[#86868b]">"总账号"</div>
                        <div class="mt-2 font-mono text-[22px] font-700 leading-none">
                            {move || summary.get().map(|s| s.total).unwrap_or_default()}
                        </div>
                    </div>
                    <div class="border-b border-black/[0.04] p-3.5 sm:border-b-0 sm:border-r">
                        <div class="text-[11px] font-600 text-[#26734d]">"可用"</div>
                        <div class="mt-2 font-mono text-[22px] font-700 leading-none">
                            {move || { summary.get().map(|s| s.usable).unwrap_or_default() }}
                        </div>
                    </div>
                    <div class="border-r border-black/[0.04] p-3.5">
                        <div class="text-[11px] font-600 text-[#86868b]">"额度耗尽"</div>
                        <div class="mt-2 font-mono text-[22px] font-700 leading-none">
                            {move || { summary.get().map(|s| s.exhausted).unwrap_or_default() }}
                        </div>
                    </div>
                    <div class="p-3.5">
                        <div class="text-[11px] font-600 text-[#a33d43]">"其他"</div>
                        <div class="mt-2 font-mono text-[22px] font-700 leading-none">
                            {move || { summary.get().map(|s| s.failed).unwrap_or_default() }}
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

                <Show when=move || checking.get() || refreshing.get()>
                    <div
                        class="pointer-events-none absolute inset-x-3 top-3 z-20 flex justify-center"
                        role="status"
                        aria-live="polite"
                    >
                        <div class="gbq-glass relative z-10 w-full max-w-[27rem] overflow-hidden rounded-[18px] px-3.5 py-3 transition-opacity duration-200">
                            <div class="relative z-10 flex items-center gap-3">
                                <div class="relative grid h-8 w-8 shrink-0 place-items-center rounded-[10px] bg-[#f2f2f7] text-[#1d1d1f]">
                                    <span class="absolute h-5 w-5 animate-spin rounded-full border-2 border-[#d2d2d7] border-t-[#1d1d1f] motion-reduce:animate-none"></span>
                                    <span class="h-1.5 w-1.5 rounded-full bg-current"></span>
                                </div>
                                <div class="min-w-0 flex-1">
                                    <div class="flex items-baseline justify-between gap-3">
                                        <div class="truncate text-[13px] font-650 text-[#1d1d1f]">
                                            {move || {
                                                if checking.get() {
                                                    "正在检测账号"
                                                } else {
                                                    "正在刷新 Token"
                                                }
                                            }}
                                        </div>
                                        <div class="shrink-0 text-[10px] font-650 tracking-[0.04em] text-[#86868b]">
                                            {move || {
                                                if checking.get() {
                                                    summary
                                                        .get()
                                                        .map(|s| format!("{} / {}", s.results.len(), s.total))
                                                        .unwrap_or_else(|| "准备中".into())
                                                } else {
                                                    "实时回填".to_string()
                                                }
                                            }}
                                        </div>
                                    </div>
                                    <div class="mt-0.5 truncate text-[11px] font-500 text-[#86868b]">
                                        {move || {
                                            if checking.get() {
                                                "正在逐个探测额度，结果会实时出现在下方"
                                            } else {
                                                "正在安全换新过期账号，当前表格不会被打断"
                                            }
                                        }}
                                    </div>
                                </div>
                            </div>
                            <div class="relative z-10 mt-2.5 h-1 overflow-hidden rounded-full bg-black/[0.06]">
                                <div class=move || {
                                    if checking.get() {
                                        "h-full w-1/3 rounded-full bg-[#007aff] animate-pulse motion-reduce:animate-none"
                                    } else {
                                        "h-full w-1/2 rounded-full bg-[#34c759] animate-pulse motion-reduce:animate-none"
                                    }
                                }></div>
                            </div>
                        </div>
                    </div>
                </Show>
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
                        <div class="hidden shrink-0 grid-cols-[minmax(0,1.2fr)_56px_100px_minmax(0,1.9fr)] items-center gap-3 rounded-t-[16px] border-b border-black/[0.05] bg-[#f5f5f7]/92 px-3.5 py-2 text-[10px] font-700 tracking-[0.12em] text-[#8e8e93] backdrop-blur-md sm:grid">
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
                                    view! {
                                        <div
                                            class="group/row relative grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 border-b border-black/[0.04] px-3.5 py-2 transition last:border-b-0 hover:bg-[#f2f2f7]/75 sm:grid-cols-[minmax(0,1.2fr)_56px_100px_minmax(0,1.9fr)] sm:gap-3"
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
                                            <div class="col-span-2 flex min-w-0 items-center gap-2 sm:col-span-1">
                                                <div class="min-w-0 flex-1">
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
                                                <div class="flex shrink-0 items-center gap-1">
                                                    {can_retry
                                                        .then(|| {
                                                            let fname = fname_retry.clone();
                                                            view! {
                                                                <button
                                                                    type="button"
                                                                    class="gbq-button gbq-button-icon grid h-6 w-6 place-items-center rounded-full p-0 text-[#6e6e73] opacity-100 outline-none sm:opacity-0 sm:group-hover/row:opacity-100 disabled:cursor-not-allowed disabled:opacity-40"
                                                                    class:hidden=move || checking.get() || refreshing.get()
                                                                    class:opacity-100=move || {
                                                                        retrying.get().as_ref().is_some_and(|n| n == &fname_spin_a)
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
                                                                        retrying.get().as_ref().is_some_and(|n| n == &fname_spin_c)
                                                                            || checking.get() || refreshing.get()
                                                                    }
                                                                    on:click=move |_| on_retry.run(fname.clone())
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
                                                        class="gbq-button gbq-button-icon grid h-6 w-6 place-items-center rounded-full p-0 text-[#6e6e73] opacity-100 outline-none sm:opacity-0 sm:group-hover/row:opacity-100"
                                                        title="下载该账号的 auth JSON（含刷新后 token）"
                                                        aria-label="下载此账号文件"
                                                        on:click=move |_| on_download.run(fname_dl.clone())
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
                                        </div>
                                    }
                                }
                            />
                        </div>
                    </div>
                </Show>
            </div>
        </section>
    }
}
