import { expect, test, type Page, type TestInfo } from "@playwright/test";
import { spawnSync } from "node:child_process";
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { extname, join } from "node:path";

const prefix = "/manual/v1/";
let workspace: string;

test.beforeAll(async () => {
  workspace = await mkdtemp(join(tmpdir(), "diplodocus-api-navigation-"));
  await cp("tests/fixtures/api-navigation", workspace, { recursive: true });
  const configuration = join(workspace, "diplodocus.yaml");
  const source = await readFile(configuration, "utf8");
  for (const style of ["file", "clean", "links"]) {
    await writeFile(configuration, source.replace("presentation:\n", `presentation:
  site-url: https://docs.example.test${prefix}
  canonical-url-style: ${style === "links" ? "file" : style}
${style === "links" ? "  header:\n    api: links\n" : ""}
`));
    const built = spawnSync(process.env.DIPLODOCUS_BROWSER_BINARY!, [
      "build", "--config", configuration, "--output", join(workspace, style),
    ], { encoding: "utf8" });
    expect(built.status, built.error?.message ?? built.stderr).toBe(0);
  }
});

test.afterAll(async () => {
  if (workspace) await rm(workspace, { recursive: true, force: true });
});

async function open(page: Page, style = "file") {
  const origin = `http://127.0.0.1:${process.env.DIPLODOCUS_PREVIEW_PORT}${prefix}`;
  await page.route(`${origin}**`, async route => {
    let path = new URL(route.request().url()).pathname.slice(prefix.length);
    if (!path || path.endsWith("/")) path += "index.html";
    else if (!extname(path)) path += ".html";
    try {
      const body = await readFile(join(workspace, style, path));
      const types: Record<string, string> = {
        ".html": "text/html", ".css": "text/css", ".js": "text/javascript", ".json": "application/json",
      };
      await route.fulfill({ body, contentType: types[extname(path)] ?? "application/octet-stream" });
    } catch {
      await route.fulfill({ status: 404, body: "Missing artifact" });
    }
  });
  await page.goto(origin);
  return origin;
}

async function capture(page: Page, info: TestInfo, name: string) {
  if (!process.env.DIPLODOCUS_BROWSER_CAPTURE) return;
  const directory = join(process.env.DIPLODOCUS_BROWSER_ARTIFACTS!, "screenshots", info.project.name);
  await mkdir(directory, { recursive: true });
  const path = join(directory, `${name}.png`);
  await page.screenshot({ path, animations: "disabled" });
  await info.attach(name, { path, contentType: "image/png" });
}

for (const style of ["file", "clean"]) {
  test(`${style} API dropdown preserves package sidebars and header navigation`, async ({ page }, info) => {
    const errors: string[] = [];
    page.on("pageerror", error => errors.push(error.message));
    const origin = await open(page, style);
    const mobile = info.project.name === "mobile";
    const menu = page.locator(mobile ? ".mobile-menu" : ".api-menu");
    const summary = menu.locator(":scope > summary");
    const links = menu.locator(mobile ? ".mobile-api a" : ".api-menu-panel a");
    const identity = await page.evaluateHandle(() => ({
      document, header: document.querySelector("header"),
    }));
    await summary.focus();
    await page.keyboard.press("Enter");
    await expect(links).toHaveText(["R", "Julia", "Rust"]);
    await expect(links.first()).toBeVisible();
    await expect(links.last()).toBeVisible();
    await expect(page.locator("header").getByText("Hidden Rust tools", { exact: true })).toHaveCount(0);
    await capture(page, info, `api-dropdown-${style}-light`);
    await page.emulateMedia({ colorScheme: "dark" });
    await capture(page, info, `api-dropdown-${style}-dark`);
    await page.emulateMedia({ colorScheme: "light" });

    for (const [label, slug, heading] of [
      ["Rust", "rust", "Rust tools"], ["Julia", "julia", "Julia tools"], ["R", "r", "R tools"],
    ]) {
      if (!await menu.evaluate(element => (element as HTMLDetailsElement).open)) await summary.click();
      const link = links.filter({ hasText: new RegExp(`^${label}$`) });
      // Visibility alone does not detect a popup clipped by its scrolling navbar.
      expect(await link.evaluate(element => {
        const { x, y, width, height } = element.getBoundingClientRect();
        return element.contains(document.elementFromPoint(x + width / 2, y + height / 2));
      })).toBe(true);
      await link.click();
      await expect(page).toHaveURL(`${origin}packages/${slug}/${style === "file" ? "index.html" : ""}`);
      await expect(page.getByRole("heading", { level: 1 })).toHaveText(heading);
      await expect(page.locator(".contextual-nav")).toHaveAttribute("data-section", slug);
      await expect(page.locator('.desktop-sections a[aria-current="true"]')).toHaveText(label);
      await expect(page.locator('.mobile-sections a[aria-current="true"]')).toHaveText(label);
      if (!mobile) {
        expect(await summary.evaluate(element => getComputedStyle(element).backgroundColor))
          .toBe(await link.evaluate(element => getComputedStyle(element).backgroundColor));
      }
      await expect(menu).not.toHaveAttribute("open", "");
      expect(await identity.evaluate(value => value.document === document
        && value.header === document.querySelector("header"))).toBe(true);
    }

    if (mobile) await summary.click();
    await page.locator("header .global-nav:visible").getByRole("link", { name: "Guide", exact: true }).click();
    await expect(page.locator('.desktop-sections a[aria-current="true"]')).toHaveText("Guide");
    await expect(menu).not.toHaveAttribute("open", "");
    await page.goBack();
    await expect(page.locator('.desktop-sections a[aria-current="true"]')).toHaveText("R");
    await expect(menu).not.toHaveAttribute("open", "");
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    expect(errors).toEqual([]);
  });
}

test("API dropdown supports keyboard dismissal and stays visible beside scrolling tabs", async ({ page }, info) => {
  test.skip(info.project.name === "mobile", "The API disclosure appears on desktop.");
  await open(page);
  const menu = page.locator(".api-menu");
  const summary = menu.locator("summary");
  await summary.focus();
  await page.keyboard.press("Enter");
  await page.keyboard.press("Tab");
  await expect(menu.getByRole("link", { name: "R", exact: true })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(menu).not.toHaveAttribute("open", "");
  await expect(summary).toBeFocused();

  await summary.click();
  await page.getByRole("heading", { level: 1 }).click({ position: { x: 2, y: 2 } });
  await expect(menu).not.toHaveAttribute("open", "");
  await summary.click();
  const repositories = page.locator(".repository-menu");
  await repositories.locator("summary").click();
  await expect(menu).not.toHaveAttribute("open", "");
  await expect(repositories).toHaveAttribute("open", "");
  await summary.click();
  await expect(repositories).not.toHaveAttribute("open", "");

  await page.setViewportSize({ width: 800, height: 900 });
  const tabs = page.locator(".desktop-sections > ul");
  expect(await tabs.evaluate(element => element.scrollWidth > element.clientWidth)).toBe(true);
  expect(await tabs.evaluate(element => {
    const bounds = element.getBoundingClientRect();
    const guide = element.querySelector("a")!.getBoundingClientRect();
    return guide.x >= bounds.x && guide.right <= bounds.right;
  })).toBe(true);
  const rust = menu.getByRole("link", { name: "Rust", exact: true });
  await expect(rust).toBeVisible();
  expect(await rust.evaluate(element => {
    const { x, y, width, height } = element.getBoundingClientRect();
    return element.contains(document.elementFromPoint(x + width / 2, y + height / 2));
  })).toBe(true);
  await capture(page, info, "api-dropdown-tablet");
  await page.setViewportSize({ width: 761, height: 900 });
  await expect(menu).toHaveAttribute("open", "");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.locator(".desktop-sections").getByRole("link", { name: "Reference", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Reference");
  await summary.click();
  await expect(rust).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(menu).not.toHaveAttribute("open", "");
  await page.locator(".mobile-menu > summary").click();
  await expect(page.locator(".mobile-api").getByRole("link", { name: "Rust", exact: true })).toBeVisible();
});

test("separate API tabs can be selected explicitly", async ({ page }, info) => {
  const origin = await open(page, "links");
  await expect(page.locator(".api-menu, .mobile-api")).toHaveCount(0);
  if (info.project.name === "mobile") await page.locator(".mobile-menu > summary").click();
  const navigation = page.locator("header .global-nav:visible");
  await expect(navigation.getByRole("link")).toHaveText(["Guide", "Reference", "R", "Julia", "Rust"]);
  await navigation.getByRole("link", { name: "Rust", exact: true }).click();
  await expect(page).toHaveURL(`${origin}packages/rust/index.html`);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Rust tools");
});

test("API navigation works without JavaScript", async ({ browser }, info) => {
  const context = await browser.newContext({ javaScriptEnabled: false, viewport: info.project.use.viewport });
  try {
    const page = await context.newPage();
    const origin = await open(page, "clean");
    const menu = page.locator(info.project.name === "mobile" ? ".mobile-menu" : ".api-menu");
    await menu.locator(":scope > summary").click();
    await menu.getByRole("link", { name: "Rust", exact: true }).click();
    await expect(page).toHaveURL(`${origin}packages/rust/`);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Rust tools");
    await expect(page.locator('.desktop-sections a[aria-current="true"]')).toHaveText("Rust");
  } finally {
    await context.close();
  }
});
