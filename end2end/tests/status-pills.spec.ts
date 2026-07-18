import { test, expect } from "@playwright/test";
import path from "node:path";
import fs from "node:fs";
import os from "node:os";
import zlib from "node:zlib";

const BASE = process.env.GBQ_BASE_URL ?? "http://127.0.0.1:3737";

function delay(ms: number): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, ms);
  return promise;
}

// 最小 zip 解析：EOCD -> central directory -> inflateRaw，验证导出内容真实可读
function readZipEntries(buf: Buffer): Map<string, Buffer> {
  let eocd = -1;
  for (let i = buf.length - 22; i >= Math.max(0, buf.length - 22 - 65536); i--) {
    if (buf.readUInt32LE(i) === 0x06054b50) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0) throw new Error("EOCD not found");
  const count = buf.readUInt16LE(eocd + 10);
  let off = buf.readUInt32LE(eocd + 16);
  const out = new Map<string, Buffer>();
  for (let n = 0; n < count; n++) {
    if (buf.readUInt32LE(off) !== 0x02014b50) throw new Error("bad central header");
    const method = buf.readUInt16LE(off + 10);
    const compSize = buf.readUInt32LE(off + 20);
    const nameLen = buf.readUInt16LE(off + 28);
    const extraLen = buf.readUInt16LE(off + 30);
    const commentLen = buf.readUInt16LE(off + 32);
    const localOff = buf.readUInt32LE(off + 42);
    const name = buf.subarray(off + 46, off + 46 + nameLen).toString("utf8");
    const lNameLen = buf.readUInt16LE(localOff + 26);
    const lExtraLen = buf.readUInt16LE(localOff + 28);
    const dataStart = localOff + 30 + lNameLen + lExtraLen;
    const raw = buf.subarray(dataStart, dataStart + compSize);
    out.set(name, method === 8 ? zlib.inflateRawSync(raw) : raw);
    off += 46 + nameLen + extraLen + commentLen;
  }
  return out;
}

function readZipEntryYear(buf: Buffer, target: string): number {
  let eocd = -1;
  for (let i = buf.length - 22; i >= Math.max(0, buf.length - 22 - 65536); i--) {
    if (buf.readUInt32LE(i) === 0x06054b50) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0) throw new Error("EOCD not found");
  const count = buf.readUInt16LE(eocd + 10);
  let off = buf.readUInt32LE(eocd + 16);
  for (let n = 0; n < count; n++) {
    if (buf.readUInt32LE(off) !== 0x02014b50) throw new Error("bad central header");
    const nameLen = buf.readUInt16LE(off + 28);
    const extraLen = buf.readUInt16LE(off + 30);
    const commentLen = buf.readUInt16LE(off + 32);
    const name = buf.subarray(off + 46, off + 46 + nameLen).toString("utf8");
    if (name === target) {
      return 1980 + (buf.readUInt16LE(off + 14) >> 9);
    }
    off += 46 + nameLen + extraLen + commentLen;
  }
  throw new Error(`ZIP entry not found: ${target}`);
}

type MockStatus =
  | "Ok"
  | "Exhausted"
  | "RateLimited"
  | "SpendingLimited"
  | "RefreshFailed"
  | "ChatDenied"
  | "AuthFailed"
  | "Expired"
  | "NetworkError";

type MockPlan =
  | "Free"
  | "SuperGrokLite"
  | "SuperGrok"
  | "SuperGrokHeavy"
  | "PaidOther"
  | "Unknown";

const LABEL: Record<MockStatus, string> = {
  Ok: "可用",
  Exhausted: "额度耗尽",
  RateLimited: "限流",
  SpendingLimited: "消费上限",
  RefreshFailed: "刷新失败",
  ChatDenied: "访问拒绝",
  AuthFailed: "鉴权失败",
  Expired: "Token 过期",
  NetworkError: "网络错误",
};

const PLAN_LABEL: Record<MockPlan, string> = {
  Free: "Free",
  SuperGrokLite: "Lite",
  SuperGrok: "Super",
  SuperGrokHeavy: "Heavy",
  PaidOther: "付费",
  Unknown: "未知",
};

function mockResult(
  filename: string,
  status: MockStatus,
  plan: MockPlan = "Free",
  refreshed = false,
) {
  const rem =
    status === "Ok" ? 180000 : status === "Exhausted" ? 0 : status === "ChatDenied" ? 50000 : null;
  const lim =
    status === "Ok" || status === "Exhausted" || status === "ChatDenied" ? 200000 : null;
  const detail =
    status === "RefreshFailed"
      ? "refresh_token 已失效(被吊销或轮换),请重新登录拿新 auth,或改用上次导出的新文件"
      : status === "ChatDenied"
        ? "上游永久拒绝 chat 接口"
        : status === "AuthFailed"
          ? "access_token 无效"
          : status === "NetworkError"
            ? "请求超时"
            : status === "RateLimited"
              ? "触发限流（HTTP 429），额度未耗尽"
              : status === "SpendingLimited"
                ? "已达月度消费上限或团队额度"
                : null;

  const usage_percent =
    status === "RateLimited"
      ? 2
      : status === "SpendingLimited"
        ? 100
        : plan === "SuperGrok" || plan === "SuperGrokLite" || plan === "SuperGrokHeavy"
          ? status === "Exhausted"
            ? 100
            : status === "Ok"
              ? 42.5
              : null
          : null;

  return {
    account: filename.replace(/\.json$/, "@example.com"),
    filename,
    status,
    status_label: LABEL[status],
    plan,
    plan_label: PLAN_LABEL[plan],
    quota:
      usage_percent != null
        ? `周 ${usage_percent.toFixed(0)}% 已用`
        : rem != null && lim != null
          ? `日 ${rem} / ${lim}`
          : "--",
    usable: status === "Ok",
    remaining_tokens: usage_percent != null ? Math.round(100 - usage_percent) : rem,
    limit_tokens: usage_percent != null ? 100 : lim,
    remaining_requests: null,
    limit_requests: null,
    usage_percent,
    http_status: null,
    detail: status === "Expired" ? "鉴权被拒（HTTP 401），token 未刷新" : detail,
    refreshed,
    updated_content: refreshed
      ? JSON.stringify({
          email: `${filename}@example.com`,
          access_token: "refreshed-access",
          refresh_token: "refreshed-rotated",
        })
      : null,
  };
}

const CASES: Array<{ file: string; status: MockStatus; plan: MockPlan }> = [
  { file: "ok.json", status: "Ok", plan: "Free" },
  { file: "exhausted.json", status: "Exhausted", plan: "SuperGrok" },
  { file: "rate_limited.json", status: "RateLimited", plan: "SuperGrok" },
  { file: "spending.json", status: "SpendingLimited", plan: "SuperGrokHeavy" },
  { file: "refresh_fail.json", status: "RefreshFailed", plan: "Unknown" },
  { file: "chat_denied.json", status: "ChatDenied", plan: "PaidOther" },
  { file: "auth_fail.json", status: "AuthFailed", plan: "SuperGrokLite" },
  { file: "token_expired.json", status: "Expired", plan: "SuperGrok" },
  { file: "network.json", status: "NetworkError", plan: "SuperGrokHeavy" },
];

test("status pills render as colored tags including 刷新失败", async ({ page }) => {
  await page.route("**/api/check_auth_file*", async (route) => {
    const post = route.request().postData() ?? "";
    const m = post.match(/file(?:%5B|\[)filename(?:%5D|\])=([^&]+)/i);
    const filename = m ? decodeURIComponent(m[1]) : "unknown.json";
    const hit = CASES.find((c) => c.file === filename);
    const wantsRefresh = /(?:^|&)refresh=true(?:&|$)/.test(post);
    let status = hit?.status ?? "NetworkError";
    let plan = hit?.plan ?? "Unknown";
    let refreshed = false;
    // refresh=true：token_expired / ok 换新成功（对齐服务端手动/自动刷新）
    if (wantsRefresh && (filename === "token_expired.json" || filename === "ok.json")) {
      status = "Ok";
      refreshed = true;
    }
    await delay(250);
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(mockResult(filename, status, plan, refreshed)),
    });
  });

  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "gbq-pills-"));
  const paths = CASES.map(({ file }) => {
    const p = path.join(tmp, file);
    fs.writeFileSync(
      p,
      JSON.stringify({
        email: `${file}@example.com`,
        access_token: "mock",
        refresh_token: "mock",
      }),
    );
    return p;
  });

  await page.setViewportSize({ width: 1440, height: 1100 });
  await page.goto(BASE + "/", { waitUntil: "networkidle" });
  await expect(page).toHaveTitle(/额度/);

  await page.locator('input[type="file"]').setInputFiles(paths);

  // 账号列表计数从 0 -> 9
  await expect(page.getByRole("heading", { name: "账号列表" })).toBeVisible();
  await expect(page.getByText("refresh_fail.json")).toBeVisible({ timeout: 15_000 });
  await expect(page.getByRole("button", { name: /开始检测/ })).toBeEnabled();

  // 开启「自动刷新 Token」：ok / token_expired 会在检测时自动换新
  const switchEl = page.getByRole("switch", { name: "自动刷新 Token" });
  await expect(switchEl).toHaveCSS("border-top-width", "0px");
  await switchEl.click();

  await page.getByRole("button", { name: /开始检测/ }).click();
  await expect(page.getByText("正在检测账号", { exact: true })).toBeVisible();
  fs.mkdirSync(path.resolve(__dirname, "../../tmp/gbq-fixtures"), { recursive: true });
  await page.screenshot({
    path: path.resolve(__dirname, "../../tmp/gbq-fixtures/checking-state.png"),
    fullPage: true,
  });

  await expect(page.getByText("刷新失败").first()).toBeVisible({ timeout: 15_000 });
  await expect(page.getByText("可用").first()).toBeVisible();
  await expect(page.getByText("额度耗尽").first()).toBeVisible();
  await expect(page.getByText("限流").first()).toBeVisible();
  await expect(page.getByText("消费上限").first()).toBeVisible();
  await expect(page.getByText("访问拒绝").first()).toBeVisible();
  await expect(page.getByText("鉴权失败").first()).toBeVisible();
  await expect(page.getByText("网络错误").first()).toBeVisible();

  // 表头四列：账号 / 类型 / 状态 / 额度用量
  const table = page.locator("#results-table");
  await expect(table.getByText("账号", { exact: true })).toBeVisible();
  await expect(table.getByText("类型", { exact: true })).toBeVisible();
  await expect(table.getByText("状态", { exact: true })).toBeVisible();
  await expect(table.getByText("额度用量", { exact: true })).toBeVisible();

  // 类型标签（短标签：Lite / Super / Heavy）
  const visiblePlan = (label: string) =>
    table
      .locator("span")
      .filter({ hasText: new RegExp(`^${label}$`) })
      .locator("visible=true")
      .first();
  await expect(visiblePlan("Free")).toBeVisible();
  await expect(visiblePlan("Super")).toBeVisible();
  await expect(visiblePlan("Lite")).toBeVisible();
  await expect(visiblePlan("Heavy")).toBeVisible();
  await expect(visiblePlan("付费")).toBeVisible();
  await expect(visiblePlan("未知")).toBeVisible();

  // 仅 Ok / Exhausted / RateLimited 展示额度；访问拒绝等不展示
  // 付费额度标「周」，Free 标「日」
  await expect(table.getByText("周 100% 已用").first()).toBeVisible();
  await expect(table.getByText("周 2% 已用").first()).toBeVisible();
  await expect(table.getByText(/日 180\.00K\s*\/\s*200\.00K/).first()).toBeVisible();
  await expect(table.getByText("50.00K / 200.00K")).toHaveCount(0);
  // 关键：2% 已用 必须挂在「限流」而不是「额度耗尽」
  const rateRow = table.locator('div[data-filename="rate_limited.json"]');
  await expect(rateRow.getByText("限流", { exact: true })).toBeVisible();
  await expect(rateRow.getByText("周 2% 已用")).toBeVisible();
  await expect(rateRow.getByText("额度耗尽", { exact: true })).toHaveCount(0);
  // 消费上限行展示额度条（与耗尽/限流同组）
  const spendRow = table.locator('div[data-filename="spending.json"]');
  await expect(spendRow.getByText("消费上限", { exact: true })).toBeVisible();
  await expect(spendRow.getByText("周 100% 已用")).toBeVisible();

  // macOS 风格状态悬浮提示
  const refreshPill = page.locator("span.rounded-full", { hasText: "刷新失败" }).first();
  await expect(refreshPill).toBeVisible();
  await refreshPill.hover();
  const tooltip = page.locator('[role="tooltip"]', {
    hasText: "refresh_token 已失效",
  });
  await expect(tooltip).toBeVisible();

  // 首行浮框向下弹：不被表头 / 滚动容器顶边裁剪
  const firstPill = page.locator("span.rounded-full", { hasText: "鉴权失败" }).first();
  await firstPill.hover();
  const firstTip = page.locator('[role="tooltip"]', {
    hasText: "access_token 无效",
  });
  await expect(firstTip).toBeVisible();
  const pillBox = await firstPill.boundingBox();
  const tipBox = await firstTip.boundingBox();
  expect(pillBox).not.toBeNull();
  expect(tipBox).not.toBeNull();
  expect(tipBox!.y).toBeGreaterThan(pillBox!.y + pillBox!.height);

  // 先滚到底部再回顶，确保所有行都渲染过；截整页右侧结果区
  await table.locator("div.overflow-y-auto").evaluate((el) => {
    el.scrollTop = el.scrollHeight;
  });
  await page.waitForTimeout(150);
  await table.locator("div.overflow-y-auto").evaluate((el) => {
    el.scrollTop = 0;
  });

  const outDir = path.resolve(__dirname, "../../tmp/gbq-fixtures");
  fs.mkdirSync(outDir, { recursive: true });
  const shot = path.join(outDir, "status-pills-mock.png");
  // 整页截图，避免 #results-table 高度被 flex 压扁只拍到 2 行
  await page.screenshot({ path: shot, fullPage: true });
  const tableShot = path.join(outDir, "status-pills-table-full.png");
  await table.screenshot({ path: tableShot });

  const box = await refreshPill.boundingBox();
  expect(box).not.toBeNull();
  expect(box!.height).toBeGreaterThanOrEqual(18);
  expect(box!.width).toBeGreaterThanOrEqual(60);

  const color = await refreshPill.evaluate((el) => getComputedStyle(el).color);
  expect(color).toMatch(/rgb\(\s*215,\s*0,\s*21\s*\)/);

  const bg = await refreshPill.evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(bg).toMatch(/rgba?\(\s*255,\s*69,\s*58/);

  console.log(`screenshot: ${shot}`);
  console.log(`refresh pill box=${JSON.stringify(box)} color=${color} bg=${bg}`);
});

test("手动刷新 Token + 清空 guard + 逐行下载", async ({ page }) => {
  await page.route("**/api/check_auth_file*", async (route) => {
    const post = route.request().postData() ?? "";
    const m = post.match(/file(?:%5B|\[)filename(?:%5D|\])=([^&]+)/i);
    const filename = m ? decodeURIComponent(m[1]) : "unknown.json";
    const wantsRefresh = /(?:^|&)refresh=true(?:&|$)/.test(post);
    const fixed = wantsRefresh && filename === "token_expired.json";
    if (fixed) {
      await delay(450);
    }
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(
        mockResult(filename, fixed ? "Ok" : "Expired", "SuperGrok", fixed),
      ),
    });
  });

  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "gbq-refresh-"));
  const p = path.join(tmp, "token_expired.json");
  fs.writeFileSync(
    p,
    JSON.stringify({
      email: "token_expired@example.com",
      access_token: "old",
      refresh_token: "old",
    }),
  );

  await page.setViewportSize({ width: 1440, height: 1100 });
  await page.goto(BASE + "/", { waitUntil: "networkidle" });
  await page.locator('input[type="file"]').setInputFiles([p]);
  await expect(page.getByText("token_expired.json")).toBeVisible({ timeout: 15_000 });

  // 默认不自动刷新：401/过期只报「Token 过期」，不静默换新
  await expect(page.getByRole("switch", { name: "自动刷新 Token" })).toHaveAttribute(
    "aria-checked",
    "false",
  );
  await page.getByRole("button", { name: /开始检测/ }).click();

  const table = page.locator("#results-table");
  const row = table.locator('div[data-filename="token_expired.json"]');
  await expect(row.getByText("Token 过期", { exact: true })).toBeVisible({ timeout: 15_000 });

  // 手动刷新按钮带计数；点击后行变「可用」+「已刷新」tag
  const refreshBtn = page.getByRole("button", { name: /刷新 Token · 1/ });
  await expect(refreshBtn).toBeVisible();
  await refreshBtn.click();
  await expect(page.getByRole("button", { name: "刷新中" })).toBeVisible();
  await expect(page.getByText("正在刷新 Token", { exact: true })).toBeVisible();
  await expect(row.getByRole("button", { name: "重试此账号" })).toBeHidden();
  fs.mkdirSync(path.resolve(__dirname, "../../tmp/gbq-fixtures"), { recursive: true });
  await page.screenshot({
    path: path.resolve(__dirname, "../../tmp/gbq-fixtures/refreshing-state.png"),
    fullPage: true,
  });
  await expect(row.getByText("可用", { exact: true })).toBeVisible({ timeout: 15_000 });
  await expect(row.getByText("已刷新", { exact: true })).toBeVisible();
  await expect(page.getByText(/已刷新 1 个 token/)).toBeVisible();

  // 清空 guard：新文件未落盘前拒绝清空
  await page.getByRole("button", { name: "清空", exact: true }).click();
  await expect(page.getByText(/未落盘/)).toBeVisible();
  await expect(page.getByText("token_expired.json")).toBeVisible();

  // 逐行下载解除 guard（按钮必须无 UA 黑框）
  const dlBtn = row.getByRole("button", { name: "下载此账号文件" });
  await expect(dlBtn).toHaveCSS("border-top-width", "0px");
  await row.hover();
  const [download] = await Promise.all([
    page.waitForEvent("download"),
    dlBtn.click(),
  ]);
  expect(download.suggestedFilename()).toBe("token_expired.json");

  await page.getByRole("button", { name: "清空", exact: true }).click();
  await expect(page.getByText("token_expired.json")).toHaveCount(0);
  await expect(page.getByRole("button", { name: /开始检测/ })).toBeDisabled();
});

test("导出 ZIP 真实落盘：条目可读且含刷新后 token", async ({ page }) => {
  // 强制走 a[download] fallback（正是之前 revoke race 掐死导出的路径）
  await page.addInitScript(() => {
    delete (window as unknown as Record<string, unknown>).showSaveFilePicker;
  });
  await page.route("**/api/check_auth_file*", async (route) => {
    const post = route.request().postData() ?? "";
    const m = post.match(/file(?:%5B|\[)filename(?:%5D|\])=([^&]+)/i);
    const filename = m ? decodeURIComponent(m[1]) : "unknown.json";
    const hit = CASES.find((c) => c.file === filename);
    const wantsRefresh = /(?:^|&)refresh=true(?:&|$)/.test(post);
    const fixed = wantsRefresh && filename === "token_expired.json";
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(
        mockResult(filename, fixed ? "Ok" : hit?.status ?? "Expired", hit?.plan ?? "Unknown", fixed),
      ),
    });
  });

  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "gbq-zip-"));
  const p1 = path.join(tmp, "ok.json");
  const p2 = path.join(tmp, "token_expired.json");
  fs.writeFileSync(p1, JSON.stringify({ email: "ok@example.com", access_token: "a", refresh_token: "r" }));
  fs.writeFileSync(p2, JSON.stringify({ email: "token_expired@example.com", access_token: "old", refresh_token: "old" }));

  await page.setViewportSize({ width: 1440, height: 1100 });
  await page.goto(BASE + "/", { waitUntil: "networkidle" });
  await page.locator('input[type="file"]').setInputFiles([p1, p2]);
  await expect(page.getByText("token_expired.json")).toBeVisible({ timeout: 15_000 });

  await page.getByRole("switch", { name: "自动刷新 Token" }).click();
  await page.getByRole("button", { name: /开始检测/ }).click();
  await expect(page.getByText(/已刷新 1 个 token/)).toBeVisible({ timeout: 15_000 });

  // 导出 ZIP：必须真实产出可解析的 zip，且过期账号条目是刷新后的内容
  const [zipDownload] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("button", { name: /导出 ZIP/ }).click(),
  ]);
  expect(zipDownload.suggestedFilename()).toMatch(/^grok-auth-all-.+\.zip$/);
  const zipPath = await zipDownload.path();
  expect(zipPath).toBeTruthy();
  const zipBytes = fs.readFileSync(zipPath!);
  const entries = readZipEntries(zipBytes);
  expect([...entries.keys()].sort()).toEqual(["ok.json", "token_expired.json"]);
  expect(readZipEntryYear(zipBytes, "ok.json")).toBeGreaterThan(1980);
  const refreshedAuth = JSON.parse(entries.get("token_expired.json")!.toString("utf8"));
  expect(refreshedAuth.access_token).toBe("refreshed-access");
  expect(refreshedAuth.refresh_token).toBe("refreshed-rotated");

  // 导出成功 -> guard 解除 -> 清空放行
  await page.getByRole("button", { name: "清空", exact: true }).click();
  await expect(page.getByText("token_expired.json")).toHaveCount(0);
});

test("网络错误不标已刷新 + 行内重试", async ({ page }) => {
  let networkHits = 0;
  await page.route("**/api/check_auth_file*", async (route) => {
    const post = route.request().postData() ?? "";
    const m = post.match(/file(?:%5B|\[)filename(?:%5D|\])=([^&]+)/i);
    const filename = m ? decodeURIComponent(m[1]) : "unknown.json";
    if (filename === "network.json") {
      networkHits += 1;
      // 第一次：模拟「token 已换但 probe 网络失败」——服务端应 refreshed=false
      // 第二次（行内重试）：恢复可用
      if (networkHits === 1) {
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify(
            mockResult(filename, "NetworkError", "SuperGrokHeavy", false),
          ),
        });
        return;
      }
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(mockResult(filename, "Ok", "SuperGrokHeavy", true)),
      });
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(mockResult(filename, "Ok", "Free", false)),
    });
  });

  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "gbq-retry-"));
  const p = path.join(tmp, "network.json");
  fs.writeFileSync(
    p,
    JSON.stringify({
      email: "network@example.com",
      access_token: "a",
      refresh_token: "r",
    }),
  );

  await page.setViewportSize({ width: 1440, height: 1100 });
  await page.goto(BASE + "/", { waitUntil: "networkidle" });
  await page.locator('input[type="file"]').setInputFiles([p]);
  await expect(page.getByText("network.json")).toBeVisible({ timeout: 15_000 });

  await page.getByRole("button", { name: /开始检测/ }).click();

  const table = page.locator("#results-table");
  const row = table.locator('div[data-filename="network.json"]');
  await expect(row.getByText("网络错误", { exact: true })).toBeVisible({ timeout: 15_000 });
  // 关键：网络失败行绝不能出现「已刷新」绿标
  await expect(row.getByText("已刷新", { exact: true })).toHaveCount(0);

  const retryBtn = row.getByRole("button", { name: "重试此账号" });
  await expect(retryBtn).toBeVisible();
  await row.hover();
  await retryBtn.click();

  await expect(row.getByText("可用", { exact: true })).toBeVisible({ timeout: 15_000 });
  await expect(row.getByText("已刷新", { exact: true })).toBeVisible();
  await expect(page.getByText(/已刷新 1 个 token/)).toBeVisible();
  expect(networkHits).toBeGreaterThanOrEqual(2);
});
