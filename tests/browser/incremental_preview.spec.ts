import { expect, test } from "@playwright/test";
import { spawn } from "node:child_process";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

test("plain serve refreshes the browser after an authored save", async ({ page }, info) => {
  const binary = process.env.DIPLODOCUS_BROWSER_BINARY;
  if (!binary) throw new Error("Run npm run site-test or npm run site-capture.");
  const workspace = await mkdtemp(join(tmpdir(), "diplodocus-incremental-preview-"));
  await mkdir(join(workspace, "docs"));
  const source = join(workspace, "docs/index.md");
  await writeFile(source, "# First preview\n");
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
    await writeFile(source, "# Updated preview\n\nA source save refreshes this page.\n");
    await expect(page.locator("h1")).toHaveText("Updated preview", { timeout: 15_000 });
    await expect(page.getByText("A source save refreshes this page.")).toBeVisible();
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
