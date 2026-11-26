import { expect, test } from "@playwright/test";
import { spawn } from "node:child_process";
import { mkdir, mkdtemp, rename, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

test("plain serve updates Markdown in place and reloads after a title change", async ({ page }, info) => {
  const binary = process.env.DIPLODOCUS_BROWSER_BINARY;
  if (!binary) throw new Error("Run npm run site-test or npm run site-capture.");
  const workspace = await mkdtemp(join(tmpdir(), "diplodocus-incremental-preview-"));
  await mkdir(join(workspace, "docs"));
  const source = join(workspace, "docs/index.md");
  const padding = "\n\nLook at how the preview updates while scrolling.".repeat(80);
  const body = "# First preview\n\nThe original marker text." + padding;
  await writeFile(source, body);
  await writeFile(join(workspace, "diplodocus.yaml"), "project:\n  name: Incremental preview\nrepositories:\n  - id: docs\n    path: .\ncontent:\n  - id: guide\n    owner: project\n    repository: docs\n    path: docs\n    mount: ''\n    format: gfm\n");
  const child = spawn(binary, ["serve", "--port", "0"], {
    cwd: workspace,
    stdio: ["ignore", "ignore", "pipe"],
  });
  let log = "";
  child.stderr.on("data", data => { log += data.toString(); });
  const closed = new Promise<void>(resolve => child.once("close", () => resolve()));
  try {
    await expect.poll(() => log.match(/Serving (http:\/\/127\.0\.0\.1:\d+\/)/)?.[1], {
      timeout: 15_000,
      message: "The isolated preview must become ready.",
    }).toBeTruthy();
    const address = log.match(/Serving (http:\/\/127\.0\.0\.1:\d+\/)/)![1];
    const errors: string[] = [];
    page.on("pageerror", error => errors.push(error.message));
    await page.goto(address);
    await expect(page.locator("h1")).toHaveText("First preview");
    await expect(page.locator("html")).toHaveAttribute("data-diplodocus-router", "true");
    await expect(stat(join(workspace, "site"))).rejects.toMatchObject({ code: "ENOENT" });
    await page.evaluate(() => {
      Reflect.set(window, "originalPreviewDocument", document);
      scrollTo(0, 500);
    });
    const search = page.locator("#search");
    await page.getByRole("button", { name: /^Search/ }).click();
    await search.fill("previewquartz");
    await expect(page.locator(".search-dialog [role=status]")).toContainText(/no results/i);
    const before = await page.evaluate(() => ({ y: scrollY, history: history.length }));
    await writeFile(source, body.replace("original marker text", "previewquartz"));
    await expect(page.getByText("The previewquartz.", { exact: true })).toBeVisible();
    await expect(page.locator("#search-results a").first()).toContainText("First preview");
    await expect(search).toBeFocused();
    expect(await page.evaluate(() => ({
      sameDocument: Reflect.get(window, "originalPreviewDocument") === document,
      y: scrollY,
      history: history.length,
    }))).toEqual({ sameDocument: true, ...before });

    const temporary = source + ".save";
    await writeFile(temporary, body.replace("original marker text", "atomic marker"));
    await rename(temporary, source);
    await expect(page.getByText("The atomic marker.", { exact: true })).toBeVisible();
    await expect(page.locator(".search-dialog [role=status]")).toContainText(/no results/i);
    expect(await page.evaluate(() => Reflect.get(window, "originalPreviewDocument") === document)).toBe(true);

    await writeFile(source, "# Updated preview\n\nA source save refreshes this page.\n");
    await expect(page.locator("h1")).toHaveText("Updated preview", { timeout: 15_000 });
    await expect(page.getByText("A source save refreshes this page.")).toBeVisible();
    expect(await page.evaluate(() => Reflect.get(window, "originalPreviewDocument"))).toBeUndefined();
    expect(errors).toEqual([]);
    if (process.env.DIPLODOCUS_BROWSER_CAPTURE) {
      const path = info.outputPath("incremental-preview.png");
      await page.screenshot({ path, fullPage: true });
      await info.attach("incremental-preview", { path, contentType: "image/png" });
    }
  } finally {
    child.kill("SIGTERM");
    const kill = setTimeout(() => child.kill("SIGKILL"), 10_000);
    await closed;
    clearTimeout(kill);
    await rm(workspace, { recursive: true, force: true });
  }
});
