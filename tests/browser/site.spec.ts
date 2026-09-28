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
  ["/comparing-predictions.html", "Comparing predictions", "guide"],
  ["/packages/python/", "Tiny Stats for Python", "python-package"],
  ["/packages/r/", "Tiny Stats for R", "r-package"],
]) {
  test(`${name} has one title and fits the viewport`, async ({ page }, info) => {
    await page.goto(path);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(title);
    await expect(page.getByRole("navigation")).toBeVisible();
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
  await expect(results.getByRole("link", { name: "mean_squared_error · Tiny Stats for R", exact: true })).toBeVisible();
  await python.click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("tinystats.mean_squared_error");
});

test("equivalent APIs link in both directions", async ({ page }, info) => {
  await page.goto("/comparing-predictions.html");
  await page.getByRole("main").getByRole("link", { name: "pystats::tinystats.mean_squared_error", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("tinystats.mean_squared_error");
  await fitsViewport(page);
  await capture(page, info, "python-api");
  await page.locator("section").filter({ has: page.getByRole("heading", { name: "Same API in", exact: true }) })
    .getByRole("link", { name: "Tiny Stats for R: mean_squared_error", exact: true }).click();
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
