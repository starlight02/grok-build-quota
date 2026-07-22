use std::collections::HashMap;

use js_sys::{Array, Object, Reflect};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, ClipboardItem, File, Url, window};

use super::style::{QUOTA_SEGMENTS, lit_segments, quota_bar, quota_display};
use crate::check::{AccountPlan, AccountStatus, CheckResult};

pub(crate) async fn read_file_text(file: &File) -> Result<String, String> {
    let js_val = JsFuture::from(file.text())
        .await
        .map_err(|_| "读取文件失败".to_string())?;
    js_val
        .as_string()
        .ok_or_else(|| "文件内容不是文本".to_string())
}

pub(crate) fn extract_email(content: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(content).ok()?;
    let email = v.get("email")?.as_str()?.trim();
    if email.is_empty() {
        None
    } else {
        Some(email.to_string())
    }
}

pub(crate) async fn export_results_image(
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
            "共 {} 个账号 · 可用 {} · 生成自 starlight02/grok-build-quota",
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

pub(crate) fn round_rect(
    ctx: &web_sys::CanvasRenderingContext2d,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    r: f64,
) {
    let r = r.min(w / 2.0).min(h / 2.0);
    ctx.begin_path();
    ctx.move_to(x + r, y);
    let _ = ctx.arc_to(x + w, y, x + w, y + h, r);
    let _ = ctx.arc_to(x + w, y + h, x, y + h, r);
    let _ = ctx.arc_to(x, y + h, x, y, r);
    let _ = ctx.arc_to(x, y, x + w, y, r);
    ctx.close_path();
}

pub(crate) async fn canvas_to_png_blob(
    canvas: &web_sys::HtmlCanvasElement,
) -> Result<Blob, String> {
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

pub(crate) async fn copy_blob_to_clipboard(blob: &Blob) -> Result<(), String> {
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
pub(crate) fn zip_entry_name(name: &str) -> String {
    name.rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .trim()
        .to_string()
}

pub(crate) fn build_auth_zip(files: &[(String, String)]) -> Result<Vec<u8>, String> {
    use std::io::{Cursor, Write};

    use zip::{CompressionMethod, DateTime, write::SimpleFileOptions};

    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let opts = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .last_modified_time(DateTime::default_for_write());
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

pub(crate) fn bytes_to_zip_blob(bytes: &[u8]) -> Result<Blob, String> {
    let u8arr = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
    u8arr.copy_from(bytes);
    let parts = Array::new();
    parts.push(&u8arr);
    let props = web_sys::BlobPropertyBag::new();
    props.set_type("application/zip");
    Blob::new_with_u8_array_sequence_and_options(&parts, &props)
        .map_err(|_| "创建 ZIP Blob 失败".to_string())
}

pub(crate) fn is_abort_error(err: &wasm_bindgen::JsValue) -> bool {
    err.dyn_ref::<js_sys::Error>()
        .map(|e| e.name() == "AbortError")
        .unwrap_or(false)
}

/// Chrome File System Access API: showSaveFilePicker + createWritable。
/// 用户取消 AbortError 不回退下载；API 不可用或写入失败时 fallback 到 a[download]。
pub(crate) async fn save_blob_picker_or_download(
    blob: &Blob,
    suggested_name: &str,
) -> Result<String, String> {
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
pub(crate) fn json_download_blob(content: &str) -> Result<Blob, String> {
    let parts = Array::of1(&JsValue::from_str(content));
    let props = web_sys::BlobPropertyBag::new();
    props.set_type("application/json");
    Blob::new_with_str_sequence_and_options(&parts, &props)
        .map_err(|_| "创建下载文件失败".to_string())
}

pub(crate) fn download_blob(blob: &Blob, filename: &str) -> Result<(), String> {
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
    let body = document
        .body()
        .ok_or_else(|| "body unavailable".to_string())?;
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::build_auth_zip;

    #[test]
    fn auth_zip_entries_have_non_epoch_timestamp() {
        let bytes = build_auth_zip(&[("account.json".into(), "{}".into())]).expect("build zip");
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("read zip");
        let entry = archive.by_name("account.json").expect("zip entry");
        let modified = entry.last_modified().expect("entry timestamp");

        assert_ne!(modified, zip::DateTime::DEFAULT);
    }
}
