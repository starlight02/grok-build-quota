use leptos::prelude::*;
use web_sys::{DragEvent, Event, FileList, HtmlInputElement};

#[component]
pub(super) fn AccountsPanel(
    selected: RwSignal<Vec<super::SelectedFile>>,
    checking: RwSignal<bool>,
    auto_refresh: RwSignal<bool>,
    on_files: Callback<FileList>,
    on_clear: Callback<()>,
    on_remove: Callback<String>,
    on_run_check: Callback<()>,
) -> impl IntoView {
    let drag_over = RwSignal::new(false);

    let on_input_change = move |ev: Event| {
        let input: HtmlInputElement = event_target(&ev);
        if let Some(files) = input.files() {
            on_files.run(files);
        }
        input.set_value("");
    };

    let on_drop = move |ev: DragEvent| {
        ev.prevent_default();
        drag_over.set(false);
        if let Some(dt) = ev.data_transfer()
            && let Some(files) = dt.files()
        {
            on_files.run(files);
        }
    };

    view! {
        // ─────────── 左栏：导入 + 账号列表 ───────────
        <section class="gbq-panel flex min-h-0 flex-col gap-3 overflow-hidden rounded-[28px] p-4 sm:p-5 lg:h-full">
            <div class="flex shrink-0 items-center justify-between">
                <div>
                    <div class="text-[10px] font-700 tracking-[0.14em] text-[#86868b]">
                        "01 / ACCOUNTS"
                    </div>
                    <h2 class="mb-0 mt-1.5 text-[18px] font-700 tracking-0">"账号列表"</h2>
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
                            if selected.get().is_empty() { "h-5 w-5" } else { "h-4 w-4" }
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
                <label class="gbq-button inline-flex min-h-9 shrink-0 cursor-pointer items-center justify-center rounded-[11px] px-4 text-[12px] font-650">
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
                                                class="gbq-button gbq-button-danger inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-full p-0 text-[#636366] outline-none active:scale-90 disabled:pointer-events-none disabled:opacity-40"
                                                aria-label="移除"
                                                disabled=move || checking.get()
                                                on:click=move |_| on_remove.run(name_del.clone())
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
                    <div class="text-[12px] font-650 text-[#3a3a3c]">"自动刷新 Token"</div>
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
                            "gbq-switch gbq-switch-on relative h-[22px] w-[38px] shrink-0 cursor-pointer outline-none transition-colors duration-200 disabled:cursor-not-allowed disabled:opacity-45"
                        } else {
                            "gbq-switch gbq-switch-off relative h-[22px] w-[38px] shrink-0 cursor-pointer outline-none transition-colors duration-200 disabled:cursor-not-allowed disabled:opacity-45"
                        }
                    }
                >
                    <span class=move || {
                        if auto_refresh.get() {
                            "gbq-switch-thumb absolute left-[2px] top-[2px] h-[18px] w-[18px] translate-x-[16px] rounded-full transition-transform duration-200"
                        } else {
                            "gbq-switch-thumb absolute left-[2px] top-[2px] h-[18px] w-[18px] translate-x-0 rounded-full transition-transform duration-200"
                        }
                    }></span>
                </button>
            </div>

            // 操作按钮（钉在左栏底部）
            <div class="flex shrink-0 gap-2">
                <button
                    class="gbq-button min-h-10 flex-1 rounded-[12px] px-4 text-[13px] font-650"
                    on:click=move |_| on_clear.run(())
                    disabled=move || checking.get() || selected.get().is_empty()
                >
                    "清空"
                </button>
                <button
                    class="gbq-button gbq-button-primary inline-flex min-h-10 flex-[1.6] items-center justify-center gap-2 rounded-[12px] px-5 text-[13px] font-650"
                    on:click=move |_| on_run_check.run(())
                    disabled=move || checking.get() || selected.get().is_empty()
                >
                    <span>
                        {move || { if checking.get() { "检测中" } else { "开始检测" } }}
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
    }
}
