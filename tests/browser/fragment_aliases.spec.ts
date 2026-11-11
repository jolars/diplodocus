import { expect, test } from "@playwright/test";
import { spawn, type ChildProcess } from "node:child_process";
import { once } from "node:events";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const oldTab = "Eunoia.label_boxes-Tuple{AbstractDict, AbstractDict}";

for (const style of ["file", "clean"]) {
  test.describe(`${style} legacy fragments`, () => {
    let workspace: string;
    let server: ChildProcess;
    let origin: string;
    const guide = `/guide${style === "file" ? ".html" : ""}`;

    test.beforeAll(async () => {
      const binary = process.env.DIPLODOCUS_BROWSER_BINARY;
      if (!binary) throw new Error("Run npm run site-test or npm run site-capture.");
      workspace = await mkdtemp(join(tmpdir(), "diplodocus-fragment-aliases-"));
      await mkdir(join(workspace, "docs"));
      await writeFile(join(workspace, "diplodocus.yaml"), `project:
  name: Legacy fragments
presentation:
  canonical-url-style: ${style}
  site-url: https://example.test/manual/
  redirects:
    /old: guide.html
  fragment-aliases:
    guide.html:
      Legacy.Title: { heading: Guide }
      Old.Installation: { anchor: installation }
      "${oldTab}": { heading: Second tab }
repositories:
  - id: docs
    path: .
content:
  - id: docs
    owner: project
    repository: docs
    path: docs
    mount: ''
    format: qmd
`);
      await writeFile(join(workspace, "docs/index.qmd"), "# Home\n\n[Read guide](guide.qmd)\n");
      const paragraphs = Array.from({ length: 25 }, (_, i) => `Paragraph ${i}: retained documentation content.`).join("\n\n");
      await writeFile(join(workspace, "docs/guide.qmd"), `# Guide

## Installation {#installation}

${paragraphs}

::: {.panel-tabset}

## First tab

First panel content.

## Second tab

Second panel content.

:::

${paragraphs}

[Home](index.qmd)
`);
      server = spawn(binary, ["serve", "--config", join(workspace, "diplodocus.yaml"), "--output", join(workspace, "site"), "--port", "0"], { stdio: ["ignore", "pipe", "pipe"] });
      server.stdout!.resume();
      origin = await new Promise<string>((resolve, reject) => {
        let output = "";
        const timeout = setTimeout(() => reject(new Error(`Preview did not start: ${output}`)), 30_000);
        server.stderr!.on("data", chunk => {
          output += chunk.toString();
          const match = output.match(/Serving (http:\/\/127\.0\.0\.1:\d+)/);
          if (match) {
            clearTimeout(timeout);
            resolve(match[1]);
          }
        });
        server.once("error", error => { clearTimeout(timeout); reject(error); });
        server.once("exit", code => { clearTimeout(timeout); reject(new Error(`Preview exited (${code}): ${output}`)); });
      });
    });

    test.afterAll(async () => {
      if (server && server.exitCode === null && server.signalCode === null) {
        server.kill("SIGTERM");
        await once(server, "exit");
      }
      if (workspace) await rm(workspace, { recursive: true, force: true });
    });

    test("redirects, client navigation, history, and inactive tabs retain aliases", async ({ page }, info) => {
      const errors: string[] = [];
      page.on("pageerror", error => errors.push(error.message));
      await page.goto(`${origin}/old#${encodeURIComponent(oldTab)}`);
      await expect(page).toHaveURL(`${origin}${guide}#${encodeURIComponent(oldTab)}`);
      await expect(page.getByRole("tab", { name: "Second tab" })).toHaveAttribute("aria-selected", "true");
      await expect(page.getByText("Second panel content.", { exact: true })).toBeVisible();
      const panel = page.locator(".tabset-panel").filter({ hasText: "Second panel content." });

      await page.goto(origin);
      const document = await page.evaluateHandle(() => window.document);
      await page.evaluate(hash => {
        const link = window.document.createElement("a");
        link.href = `/old#${hash}`;
        window.document.body.append(link);
        link.click();
        link.remove();
      }, encodeURIComponent(oldTab));
      await expect(page.getByText("Second panel content.", { exact: true })).toBeVisible();
      await expect(panel).toBeFocused();
      expect(await document.evaluate(value => value === window.document)).toBe(true);
      await page.goBack();
      await expect(page.getByRole("heading", { level: 1, name: "Home", exact: true })).toBeVisible();
      await page.goForward();
      await expect(page.getByText("Second panel content.", { exact: true })).toBeVisible();
      expect(await document.evaluate(value => value === window.document)).toBe(true);

      await page.evaluate(() => { location.hash = "Old.Installation"; });
      await expect(page.locator("h2#installation")).toBeFocused();
      await expect.poll(() => page.locator("h2#installation").evaluate(element => Math.abs(
        element.getBoundingClientRect().top - parseFloat(getComputedStyle(element).scrollMarginTop),
      ))).toBeLessThan(3);
      await page.evaluate(() => { location.hash = "Legacy.Title"; });
      await expect(page.getByRole("heading", { level: 1, name: "Guide", exact: true })).toBeFocused();
      await expect(page.locator("h1")).toHaveAttribute("tabindex", "-1");
      expect(await page.locator(".tabset-panel > :first-child").evaluateAll(elements => elements.every(element => element.matches("h2.tabset-heading")))).toBe(true);
      expect(errors).toEqual([]);
      if (process.env.DIPLODOCUS_BROWSER_CAPTURE) {
        await page.screenshot({ path: join(process.env.DIPLODOCUS_BROWSER_ARTIFACTS!, `fragment-aliases-${style}-${info.project.name}.png`), fullPage: true });
      }
    });

    test("aliases work without JavaScript", async ({ browser }, info) => {
      const context = await browser.newContext({
        javaScriptEnabled: false,
        viewport: info.project.use.viewport,
        isMobile: info.project.use.isMobile,
        hasTouch: info.project.use.hasTouch,
      });
      try {
        const page = await context.newPage();
        await page.goto(`${origin}/old#${encodeURIComponent(oldTab)}`);
        await expect(page.getByText("Second panel content.", { exact: true })).toBeVisible();
        expect(await page.evaluate(id => document.getElementById(id)?.parentElement?.textContent, oldTab)).toBe("Second tab");
        await expect(page.locator('aside.page-toc a[href="#installation"]')).toHaveCount(1);
        const position = await page.evaluate(id => {
          const target = document.getElementById(id)!;
          return Math.abs(target.getBoundingClientRect().top - parseFloat(getComputedStyle(target).scrollMarginTop));
        }, oldTab);
        expect(position).toBeLessThan(3);
      } finally {
        await context.close();
      }
    });
  });
}
