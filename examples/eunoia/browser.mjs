import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { chromium } from "@playwright/test";

const [baseURL, output] = process.argv.slice(2);
if (!baseURL || !output) throw new Error("Usage: node examples/eunoia/browser.mjs URL SCREENSHOTS");
const browser = await chromium.launch({ headless: true, executablePath: chromium.executablePath() });
try {
  for (const [name, viewport] of [
    ["desktop", { width: 1440, height: 1000 }],
    ["mobile", { width: 390, height: 844 }],
  ]) {
    const context = await browser.newContext({ baseURL, viewport });
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    page.on("console", message => {
      if (message.type() === "error") errors.push(message.text());
    });
    await mkdir(join(output, name), { recursive: true });
    for (const [path, screenshot] of [
      ["/docs/reference/packages/", "sql-index"],
      ["/docs/reference/python/euler/", "python-euler"],
      ["/docs/reference/r/euler/", "r-euler"],
      ["/docs/reference/r/plot.euler/", "r-plot"],
    ]) {
      assert.equal((await page.goto(path)).status(), 200);
      await page.locator(".diplodocus-content h1").waitFor();
      await page.locator(".diplodocus-content h1").scrollIntoViewIfNeeded();
      assert.equal(await page.locator("h1").count(), 1);
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth), path);
      if (screenshot !== "sql-index") {
        assert.ok(await page.locator(".diplodocus-content pre.syntax-code").count());
        assert.ok(await page.locator('.source-links a[href^="https://github.com/"]').count());
      }
      await page.screenshot({ path: join(output, name, `${screenshot}.png`) });
      if (screenshot === "r-plot") {
        const figures = page.locator(".diplodocus-content img");
        assert.ok(await figures.count());
        await figures.first().scrollIntoViewIfNeeded();
        await page.waitForFunction(() => [...document.querySelectorAll(".diplodocus-content img")].every(image => image.complete && image.naturalWidth > 0));
        await page.screenshot({ path: join(output, name, "r-figure.png") });
      }
    }
    await page.goto("/docs/reference/python/euler/");
    await page.locator('.diplodocus-content a[href="/docs/reference/r/euler/"]').first().click();
    await page.waitForURL("**/docs/reference/r/euler/");
    assert.equal(new URL(page.url()).pathname, "/docs/reference/r/euler/");
    await page.locator('.diplodocus-content a[href="/docs/reference/python/euler/"]').first().click();
    await page.waitForURL("**/docs/reference/python/euler/");
    assert.equal(new URL(page.url()).pathname, "/docs/reference/python/euler/");

    await page.goto("/docs/concepts/fitter-pipeline/");
    const initial = page.getByRole("button", { name: /Initial layout/ });
    await initial.waitFor();
    await initial.click();
    await page.waitForTimeout(500);
    assert.ok(await page.locator("main svg").count());
    await page.screenshot({ path: join(output, name, "existing-pipeline.png") });
    assert.deepEqual(errors, []);
    await context.close();
  }
  const staticContext = await browser.newContext({ baseURL, javaScriptEnabled: false });
  const staticPage = await staticContext.newPage();
  await staticPage.goto("/docs/reference/python/euler/");
  assert.ok(await staticPage.locator(".diplodocus-content pre.syntax-code").count());
  const database = await staticContext.request.get("/_diplodocus/documentation.sqlite");
  assert.equal(database.status(), 200);
  assert.equal((await database.body()).subarray(0, 16).toString(), "SQLite format 3\0");
  await staticContext.close();
  console.log("Desktop, mobile, cross-language links, figures, source links, existing Svelte component, static rendering, and SQLite download passed.");
} finally {
  await browser.close();
}
