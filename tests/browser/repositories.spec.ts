import { expect, test, type Page, type TestInfo } from "@playwright/test";
import { spawnSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { extname, join } from "node:path";

let workspace: string;

test.beforeAll(async () => {
  workspace = await mkdtemp(join(tmpdir(), "diplodocus-repositories-"));
  await mkdir(join(workspace, "docs"));
  await writeFile(join(workspace, "docs/index.md"), "# Home\n\n[Guide](guide.md)\n");
  await writeFile(join(workspace, "docs/guide.md"), "# Guide\n\n[Home](index.md)\n");
  for (const [mode, selection] of [["all", "{}"], ["single", "{repositories: [main]}"], ["hidden", "{repositories: []}"]]) {
    await writeFile(join(workspace, "diplodocus.yaml"), `project:
  name: Repository links
presentation:
  header: ${selection}
repositories:
  - {id: main, path: ., url: 'https://github.com/example/main'}
  - {id: other, path: ., url: 'https://github.com/example/other'}
content:
  - id: guide
    owner: project
    repository: main
    path: docs
    mount: ''
    format: gfm
`);
    const built = spawnSync(process.env.DIPLODOCUS_BROWSER_BINARY!, [
      "build", "--config", join(workspace, "diplodocus.yaml"), "--output", join(workspace, mode),
    ], { encoding: "utf8" });
    expect(built.status, built.error?.message ?? built.stderr).toBe(0);
  }
});

test.afterAll(async () => {
  if (workspace) await rm(workspace, { recursive: true, force: true });
});

async function open(page: Page, mode = "all") {
  const origin = `http://127.0.0.1:${process.env.DIPLODOCUS_PREVIEW_PORT}/repository-links/`;
  await page.route(`${origin}**`, async route => {
    let path = new URL(route.request().url()).pathname.slice("/repository-links/".length);
    if (!path || path.endsWith("/")) path += "index.html";
    try {
      const body = await readFile(join(workspace, mode, path));
      const types: Record<string, string> = {
        ".html": "text/html", ".css": "text/css", ".js": "text/javascript", ".json": "application/json",
      };
      await route.fulfill({ body, contentType: types[extname(path)] ?? "application/octet-stream" });
    } catch {
      await route.fulfill({ status: 404, body: "Missing artifact" });
    }
  });
  await page.goto(origin);
}

async function capture(page: Page, info: TestInfo) {
  if (!process.env.DIPLODOCUS_BROWSER_CAPTURE) return;
  const directory = join(process.env.DIPLODOCUS_BROWSER_ARTIFACTS!, "screenshots", info.project.name);
  await mkdir(directory, { recursive: true });
  const path = join(directory, "repository-menu.png");
  await page.screenshot({ path, animations: "disabled" });
  await info.attach("repository-menu", { path, contentType: "image/png" });
}

test("multiple repositories have named links and dismiss with Escape", async ({ page }, info) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await open(page);
  const mobile = info.project.name === "mobile";
  const menu = page.locator(mobile ? ".mobile-menu" : ".repository-menu");
  const summary = menu.locator(":scope > summary");
  await expect(page.locator(".header-actions > .repository-link")).toHaveCount(0);
  await summary.focus();
  await page.keyboard.press("Enter");
  const links = menu.getByRole("navigation", { name: "Repositories", exact: true }).getByRole("link");
  await expect(links).toHaveText(["example/main", "example/other"]);
  await expect(links.first()).toHaveAttribute("href", "https://github.com/example/main");
  await expect(links.last()).toHaveAttribute("href", "https://github.com/example/other");
  await expect(links.first()).toBeVisible();
  await expect(links.last()).toBeVisible();
  await page.keyboard.press("Tab");
  await expect(mobile ? menu.getByRole("link", { name: "Guide", exact: true }) : links.first()).toBeFocused();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await capture(page, info);
  await page.keyboard.press("Escape");
  await expect(menu).not.toHaveAttribute("open", "");
  await expect(summary).toBeFocused();
  await summary.click();
  await page.getByRole("heading", { name: "Home", exact: true }).click({ position: { x: 2, y: 2 } });
  await expect(menu).not.toHaveAttribute("open", "");
  await summary.click();
  await page.getByRole("main").getByRole("link", { name: "Guide", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Guide", exact: true })).toBeVisible();
  await expect(menu).not.toHaveAttribute("open", "");
  expect(errors).toEqual([]);
});

test("a selected repository stays a direct link", async ({ page }, info) => {
  await open(page, "single");
  await expect(page.locator(".repository-menu")).toHaveCount(0);
  if (info.project.name === "mobile") await page.locator(".mobile-menu > summary").click();
  const link = page.locator("header").getByRole("link", { name: "main", exact: true });
  await expect(link).toBeVisible();
  await expect(link).toHaveAttribute("href", "https://github.com/example/main");
});

test("an empty selection hides header repository links", async ({ page }) => {
  await open(page, "hidden");
  await expect(page.locator(".repository-link, .repository-menu, .mobile-repositories")).toHaveCount(0);
});

test("repository links work without JavaScript", async ({ browser }, info) => {
  const context = await browser.newContext({ javaScriptEnabled: false, viewport: info.project.use.viewport });
  const page = await context.newPage();
  try {
    await open(page);
    const menu = page.locator(info.project.name === "mobile" ? ".mobile-menu" : ".repository-menu");
    await menu.locator(":scope > summary").click();
    await expect(menu.getByRole("link", { name: "example/other", exact: true })).toBeVisible();
  } finally {
    await context.close();
  }
});
