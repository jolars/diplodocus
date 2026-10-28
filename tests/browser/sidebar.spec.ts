import { expect, test as base, type Page, type TestInfo } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

const test = base.extend({
  baseURL: async ({}, use) => {
    const port = process.env.DIPLODOCUS_PREVIEW_PORT;
    if (!port) throw new Error("The preview server did not report its port.");
    await use(`http://127.0.0.1:${port}`);
  },
});

async function openMenu(page: Page, info: TestInfo) {
  if (info.project.name === "mobile") {
    await page.locator(".nav-disclosure > summary").click();
  }
}

async function capture(page: Page, info: TestInfo) {
  if (!process.env.DIPLODOCUS_BROWSER_CAPTURE) return;
  const directory = join(process.env.DIPLODOCUS_BROWSER_ARTIFACTS!, "screenshots", info.project.name);
  await mkdir(directory, { recursive: true });
  const path = join(directory, "nested-sidebar.png");
  await page.screenshot({ path, fullPage: true, animations: "disabled" });
  await info.attach("nested-sidebar", { path, contentType: "image/png" });
}

test("nested sidebar toggles, links, and state survive client navigation", async ({ page }, info) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto("/sidebar/index.html");
  const identity = await page.evaluateHandle(() => document);
  await openMenu(page, info);
  const nav = page.locator(".contextual-nav");
  const introduction = nav.getByRole("group", { name: "Introduction", exact: true }).locator("details");
  const advanced = nav.locator(".sidebar-disclosure").filter({ has: page.locator(":scope > summary", { hasText: "Advanced" }) });
  const more = nav.getByRole("group", { name: "More topics" }).locator("details");
  await expect(introduction).toHaveAttribute("open", "");
  await expect(advanced).not.toHaveAttribute("open");
  await expect(more).not.toHaveAttribute("open");
  await expect(nav.getByText("Default Theme", { exact: true })).toBeVisible();
  await expect(nav.getByRole("link", { name: "Nav", exact: true })).not.toBeVisible();

  await more.locator(":scope > summary").focus();
  await page.keyboard.press("Enter");
  await expect(nav.getByRole("link", { name: "Extra topic" })).toBeVisible();
  await introduction.locator(":scope > summary").click();
  await nav.getByRole("link", { name: "Overview", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Default theme");
  expect(await identity.evaluate(original => original === document)).toBe(true);
  await openMenu(page, info);
  await expect(introduction).not.toHaveAttribute("open");
  await expect(more).toHaveAttribute("open", "");
  await expect(nav.locator('[aria-current="page"]')).toHaveText("Overview");

  await advanced.getByRole("link", { name: "Advanced", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Advanced");
  await openMenu(page, info);
  await expect(advanced).toHaveAttribute("open", "");
  await nav.getByRole("link", { name: "Nav", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Nav");
  await openMenu(page, info);
  await expect(nav.locator('[aria-current="page"]')).toHaveText("Nav");
  await expect(advanced).toHaveAttribute("open", "");
  await capture(page, info);
  if (info.project.name === "mobile") await page.keyboard.press("Escape");
  await page.locator(".brand").click();
  await expect(page.locator(".sidebar-group")).toHaveCount(0);
  await page.goBack({ waitUntil: "commit" });
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Nav");
  await openMenu(page, info);
  await expect(more).toHaveAttribute("open", "");
  await expect(introduction).not.toHaveAttribute("open");
  await expect(advanced).toHaveAttribute("open", "");
  expect(errors).toEqual([]);
});

test("direct visits reveal collapsed ancestors", async ({ page }, info) => {
  await page.goto("/sidebar/sidebar.html");
  await openMenu(page, info);
  await expect(page.locator('.sidebar-disclosure[data-sidebar-key="1.0.1"]')).toHaveAttribute("open", "");
  await expect(page.locator('.contextual-nav [aria-current="page"]')).toBeVisible();
  await expect(page.locator('.contextual-nav [aria-current="page"]')).toHaveText("Sidebar");
});

test.describe("without JavaScript", () => {
  test.use({ javaScriptEnabled: false });

  test("native sidebar disclosures remain operable", async ({ page }) => {
    await page.goto("/sidebar/index.html");
    const nav = page.locator(".contextual-nav");
    const more = nav.getByRole("group", { name: "More topics" }).locator("details");
    await expect(nav.getByRole("link", { name: "Extra topic" })).not.toBeVisible();
    await more.locator("summary").click();
    await nav.getByRole("link", { name: "Extra topic" }).click();
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Extra topic");
    await expect(nav.locator('[aria-current="page"]')).toBeVisible();
  });
});
