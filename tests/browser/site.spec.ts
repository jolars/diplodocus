import { expect, test as base, type Page, type TestInfo } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

const test = base.extend<{ browserErrors: string[] }>({
  baseURL: async ({}, use) => {
    const port = process.env.DIPLODOCUS_PREVIEW_PORT;
    if (!port) throw new Error("The preview server did not report its port.");
    await use(`http://127.0.0.1:${port}`);
  },
  browserErrors: [async ({ page }, use, info) => {
    const errors: string[] = [];
    page.on("pageerror", error => errors.push(error.message));
    page.on("console", message => {
      // Chromium requests this optional icon even when the page declares none.
      if (message.location().url.endsWith("/favicon.ico") && message.text().includes("404")) return;
      if (message.type() === "error") errors.push(`${message.location().url}: ${message.text()}`);
    });
    await use(errors);
    if (errors.length) {
      await info.attach("browser-errors", { body: errors.join("\n"), contentType: "text/plain" });
    }
    expect(errors, "Browser console and JavaScript errors").toEqual([]);
  }, { auto: true }],
});

async function capture(page: Page, info: TestInfo, name: string) {
  if (!process.env.DIPLODOCUS_BROWSER_CAPTURE) return;
  const directory = join(process.env.DIPLODOCUS_BROWSER_ARTIFACTS!, "screenshots", info.project.name);
  await mkdir(directory, { recursive: true });
  const path = join(directory, `${name}.png`);
  // Chromium can finish navigation before its first composited frame is ready.
  await page.evaluate(() => new Promise<void>(resolve => {
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
  }));
  await page.screenshot({ path, fullPage: true, animations: "disabled" });
  await info.attach(name, { path, contentType: "image/png" });
}

async function fitsViewport(page: Page) {
  await expect.poll(() => page.evaluate(() =>
    document.documentElement.scrollWidth <= document.documentElement.clientWidth,
  )).toBe(true);
}

for (const [path, title, name] of [
  ["/", "Tiny Stats", "home"],
  ["/guides/comparing-predictions.html", "Comparing predictions", "guide"],
  ["/packages/python/", "Tiny Stats for Python", "python-package"],
  ["/packages/r/", "Tiny Stats for R", "r-package"],
  ["/getting-started/quick-start.html", "Quick start", "quick-start"],
  ["/guides/choosing-a-metric.html", "Choosing a metric", "choosing-a-metric"],
  ["/packages/python/guides/evaluating-models.html", "Evaluating models in Python", "python-guide"],
]) {
  test(`${name} has one title and fits the viewport`, async ({ page }, info) => {
    await page.goto(path);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(title);
    await expect(page.getByRole("navigation", { name: "Documentation" })).toBeVisible();
    await fitsViewport(page);
    if (name === "guide") {
      const image = page.getByRole("img");
      await expect(image).toBeVisible();
      await expect.poll(() => image.evaluate((element: HTMLImageElement) => element.naturalWidth)).toBeGreaterThan(0);
    }
    await capture(page, info, name);
  });
}

test("search finds both APIs and follows a result", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("searchbox").fill("mean_squared_error");
  const results = page.locator("#search-results");
  const python = results.getByRole("link", { name: "tinystats.mean_squared_error · Tiny Stats for Python", exact: true });
  await expect(python).toBeVisible();
  await expect(python.locator("code")).toHaveText("tinystats.mean_squared_error");
  await expect(results.getByRole("link", { name: "mean_squared_error · Tiny Stats for R", exact: true })).toBeVisible();
  await python.click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("tinystats.mean_squared_error");
});

test("package accordions keep navigation stable across pages", async ({ page }, info) => {
  await page.goto("/");
  const navigation = page.getByRole("navigation", { name: "Documentation" });
  const documentation = navigation.getByRole("group", { name: "Documentation" });
  const packages = navigation.getByRole("group", { name: "Reference" });
  const guides = navigation.getByRole("group", { name: "Guides", exact: true });
  if (info.project.name === "mobile") {
    await expect(navigation.getByText("Browse documentation", { exact: true })).toBeVisible();
    await expect(documentation.getByRole("link", { name: "Overview" })).not.toBeVisible();
    await navigation.getByText("Browse documentation", { exact: true }).click();
  }
  await expect(documentation.getByRole("link", { name: "Overview" })).toBeVisible();
  await expect(guides.getByRole("link", { name: "Comparing predictions" })).toBeVisible();
  const python = packages.getByText("Python", { exact: true }).locator("..");
  const r = packages.getByText("R", { exact: true }).locator("..");
  await expect(python.locator(":scope > ul > li > a").filter({ hasText: /^Overview$/ })).not.toBeVisible();
  await expect(r.getByRole("link", { name: "Overview" })).not.toBeVisible();
  await python.locator(":scope > summary").click();
  await python.getByText("tinystats", { exact: true }).click();
  await expect(python.getByRole("link", { name: "mean_squared_error", exact: true })).toBeVisible();
  const functionCode = python.getByRole("link", { name: "mean_squared_error", exact: true }).locator("code");
  await expect(functionCode).toHaveText("mean_squared_error");
  await expect(python.locator("summary code")).toHaveText("tinystats");
  const proseFont = await guides.getByRole("link", { name: "Comparing predictions" }).evaluate(element => getComputedStyle(element).fontFamily);
  expect(await functionCode.evaluate(element => getComputedStyle(element).fontFamily)).not.toBe(proseFont);
  await expect(r.getByRole("link", { name: "mean_squared_error", exact: true })).not.toBeVisible();
  await capture(page, info, "navigation");
  await python.locator(":scope > summary").click();
  await expect(python.locator(":scope > ul > li > a").filter({ hasText: /^Overview$/ })).not.toBeVisible();
  await r.locator(":scope > summary").click();
  await expect(r.getByRole("link", { name: "mean_squared_error", exact: true })).toBeVisible();
  await expect(page.getByRole("main").getByRole("heading", { name: "Start with the guide" })).toBeVisible();

  await r.getByRole("link", { name: "Overview" }).click();
  if (info.project.name === "mobile") {
    await navigation.getByText("Browse documentation", { exact: true }).click();
  }
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Tiny Stats for R");
  await expect(r.getByRole("link", { name: "Overview" })).toBeVisible();
  await expect(r.getByRole("link", { name: "mean_squared_error", exact: true })).toBeVisible();
  await expect(python.locator(":scope > ul > li > a").filter({ hasText: /^Overview$/ })).not.toBeVisible();
});

test("equivalent APIs link in both directions", async ({ page }, info) => {
  await page.goto("/guides/comparing-predictions.html");
  await page.getByRole("main").getByRole("link", { name: "pystats::tinystats.mean_squared_error", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("tinystats.mean_squared_error");
  await expect(page.getByRole("heading", { level: 1 }).locator("code")).toHaveText("tinystats.mean_squared_error");
  await fitsViewport(page);
  await capture(page, info, "python-api");
  const sameApi = page.locator("section").filter({ has: page.getByRole("heading", { name: "Same API in", exact: true }) });
  await expect(sameApi.getByRole("link")).toHaveCount(1);
  await expect(sameApi.locator("code")).toHaveText("mean_squared_error");
  await sameApi.getByRole("link", { name: "Tiny Stats for R: mean_squared_error", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1, name: "mean_squared_error", exact: true })).toBeVisible();
  await fitsViewport(page);
  await capture(page, info, "r-api");
  await page.getByRole("link", { name: "Tiny Stats for Python: tinystats.mean_squared_error", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("tinystats.mean_squared_error");
});

test("search and result navigation work with the keyboard", async ({ page }) => {
  await page.goto("/");
  await page.keyboard.press("Tab");
  await expect(page.getByRole("link", { name: "Skip to content" })).toBeFocused();
  await page.keyboard.press("Tab");
  await page.keyboard.press("Tab");
  await expect(page.getByRole("searchbox")).toBeFocused();
  await page.keyboard.type("mean_squared_error");
  const result = page.locator("#search-results").getByRole("link").first();
  await expect(result).toBeVisible();
  const destination = await result.getAttribute("href");
  await page.keyboard.press("Tab");
  await expect(result).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(destination!);
});

test("preview reloads the page when its revision advances", async ({ page }) => {
  let revision = 0;
  await page.route("**/__diplodocus/revision", async route => {
    const body = String(revision);
    if (revision === 1) revision = 0;
    await route.fulfill({ status: 200, contentType: "text/plain", body });
  });
  await page.goto("/");
  let reloads = 0;
  page.on("load", () => reloads++);
  revision = 1;
  await expect.poll(() => reloads, { timeout: 5000 }).toBe(1);
});


test("shared guides, API trees, and page headings support a round trip", async ({ page }, info) => {
  await page.goto("/guides/choosing-a-metric.html");
  const navigation = page.getByRole("navigation", { name: "Documentation" });
  const openNavigation = async () => {
    if (info.project.name === "mobile") {
      await navigation.getByText("Browse documentation", { exact: true }).click();
    }
  };
  await openNavigation();
  await expect(navigation.getByRole("group", { name: "Getting started" })).toBeVisible();
  const guideLinks = await navigation.getByRole("group", { name: "Guides", exact: true }).getByRole("link").allTextContents();
  await page.getByRole("main").getByRole("link", { name: "pystats::tinystats.mean_absolute_error", exact: true }).click();
  await openNavigation();
  await expect(navigation.getByRole("group", { name: "Guides", exact: true }).getByRole("link")).toHaveText(guideLinks);
  const active = navigation.locator('[aria-current="page"]');
  await expect(active).toHaveText("mean_absolute_error");
  await expect(active).toBeVisible();
  const toc = page.getByRole("navigation", { name: "On this page" });
  if (info.project.name === "mobile") await toc.getByText("On this page", { exact: true }).click();
  await toc.getByRole("link", { name: "Parameters", exact: true }).click();
  await expect(page).toHaveURL(/#parameters$/);
  await expect(page.getByRole("main").getByRole("heading", { name: "Parameters", exact: true })).toBeInViewport();
  await navigation.getByRole("group", { name: "Guides", exact: true }).getByRole("link", { name: "Choosing a metric" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Choosing a metric");
});
