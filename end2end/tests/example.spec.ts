import { test, expect } from "@playwright/test";

const BASE = process.env.GBQ_BASE_URL ?? "http://127.0.0.1:3737";

test("homepage renders the quota checker", async ({ page }) => {
  await page.goto(`${BASE}/`, { waitUntil: "networkidle" });

  await expect(page).toHaveTitle("Grok Build 额度检测");
  await expect(page.getByRole("heading", { name: "额度批量检测" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "账号列表" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "额度表格" })).toBeVisible();
});
