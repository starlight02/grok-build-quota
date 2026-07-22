mod accounts;
mod results;

use std::collections::{HashMap, HashSet};

use accounts::AccountsPanel;
use futures::{StreamExt, stream};
use leptos::{prelude::*, task::spawn_local};
use results::ResultsPanel;
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::window;

use super::{
    export::{
        build_auth_zip, bytes_to_zip_blob, download_blob, export_results_image, extract_email,
        json_download_blob, read_file_text, save_blob_picker_or_download,
    },
    style::network_error_result,
};
use crate::check::{
    AccountStatus, AuthUpload, CHECK_WORKERS, CheckResult, CheckSummary, check_auth_file,
};
#[derive(Clone)]
struct SelectedFile {
    name: String,
    content: String,
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

#[component]
pub(crate) fn HomePage() -> impl IntoView {
    let selected = RwSignal::new(Vec::<SelectedFile>::new());
    let checking = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    let summary = RwSignal::new(Option::<CheckSummary>::None);
    let copy_msg = RwSignal::new(Option::<String>::None);
    let filter = RwSignal::new(StatusFilter::All);
    // 401/过期时是否自动刷新 token（默认关，避免静默换新后原文件失效）
    let auto_refresh = RwSignal::new(false);
    let refreshing = RwSignal::new(false);
    // 正在逐行重试的文件名（网络错误 / 失败行）
    let retrying = RwSignal::new(Option::<String>::None);
    // 已落盘的文件名（导出 ZIP / 逐行下载后置位）：清空 guard 用
    let saved_files = RwSignal::new(HashSet::<String>::new());

    let on_files = Callback::new(move |file_list: web_sys::FileList| {
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
    });

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

    let clear_files = Callback::new(move |_| {
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
    });

    let remove_file = Callback::new(move |name: String| {
        selected.update(|list| list.retain(|f| f.name != name));
    });

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

    let run_check = Callback::new(move |_| {
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
    });

    // 手动刷新可换新的行：Token 过期 / 刷新失败 / 网络错误（常驻入口，失败后仍可再点）
    let refresh_expired = Callback::new(move |_| {
        let Some(data) = summary.get_untracked() else {
            return;
        };
        let targets: HashSet<String> = data
            .results
            .iter()
            .filter(|r| {
                matches!(
                    r.status,
                    AccountStatus::Expired
                        | AccountStatus::RefreshFailed
                        | AccountStatus::NetworkError
                )
            })
            .map(|r| r.filename.clone())
            .collect();
        if targets.is_empty() {
            return;
        }
        let uploads: Vec<AuthUpload> = selected
            .get_untracked()
            .into_iter()
            .filter(|f| targets.contains(&f.name))
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
                // 手动刷新入口：强制 allow_refresh=true
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
    });

    // 逐行下载该账号的 auth JSON（优先刷新后的内存内容）
    let download_one = Callback::new(move |filename: String| {
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
    });

    // 逐行重试：强制尝试刷新（用户显式点重试，不跟自动刷新开关）
    let retry_one = Callback::new(move |filename: String| {
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
        let fname = upload.filename.clone();
        retrying.set(Some(fname.clone()));
        error.set(None);
        spawn_local(async move {
            let result = match check_auth_file(upload, true).await {
                Ok(r) => r,
                Err(err) => network_error_result(fname.clone(), err.to_string()),
            };
            apply_result(result);
            retrying.set(None);
            refresh_notice();
        });
    });

    let copy_image = Callback::new(move |_| {
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
    });

    let export_zip = Callback::new(move |_| {
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
    });

    view! {
        <div class="relative flex h-svh flex-col overflow-hidden bg-[#f5f5f7] font-sans text-[#1d1d1f] antialiased max-lg:h-auto max-lg:min-h-svh max-lg:overflow-y-auto">
            <div class="relative mx-auto flex h-full w-full max-w-6xl min-h-0 flex-1 flex-col gap-3 px-3 py-3 sm:gap-4 sm:px-5 sm:py-4 md:gap-4 md:py-5">
                <div class="gbq-panel flex min-h-15 shrink-0 items-center justify-between gap-4 rounded-[22px] px-4 sm:px-5">
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
                    <div class="gbq-chip flex items-center gap-2 rounded-full px-3 py-1.5 text-[11px] font-600 text-[#6e6e73]">
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
                    <div class="gbq-chip flex items-center gap-2 rounded-full px-3 py-2 text-[11px] text-[#6e6e73]">
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

                <div class="grid min-h-0 flex-1 gap-4 md:gap-5 lg:grid-cols-[minmax(300px,380px)_minmax(0,1fr)] lg:items-stretch">
                    <AccountsPanel
                        selected=selected
                        checking=checking
                        auto_refresh=auto_refresh
                        on_files=on_files
                        on_clear=clear_files
                        on_remove=remove_file
                        on_run_check=run_check
                    />
                    <ResultsPanel
                        summary=summary
                        checking=checking
                        refreshing=refreshing
                        retrying=retrying
                        copy_msg=copy_msg
                        filter=filter
                        on_refresh=refresh_expired
                        on_export=export_zip
                        on_copy_image=copy_image
                        on_retry=retry_one
                        on_download=download_one
                    />
                </div>
                <footer class="flex shrink-0 flex-wrap items-center justify-center gap-x-2 gap-y-1 px-3 pb-0.5 pt-1 text-center text-[10px] text-[#86868b]">
                    <span>"POST /v1/responses 探针"</span>
                    <span class="h-0.5 w-0.5 rounded-full bg-[#aeaeb2]"></span>
                    <span>"账号 / 类型 / 状态 / 额度"</span>
                    <span class="h-0.5 w-0.5 rounded-full bg-[#aeaeb2]"></span>
                    <a
                        href="https://github.com/starlight02/grok-build-quota"
                        target="_blank"
                        rel="noopener noreferrer"
                        class="inline-flex items-center gap-1 font-medium text-[#6e6e73] transition hover:text-[#1d1d1f] hover:underline"
                    >
                        <svg class="h-3 w-3 fill-current" viewBox="0 0 24 24">
                            <path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z" />
                        </svg>
                        <span>"starlight02/grok-build-quota"</span>
                    </a>
                </footer>
            </div>
        </div>
    }
}
