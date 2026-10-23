import { expect, test } from "@playwright/test";
import { spawnSync } from "node:child_process";
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { extname, join } from "node:path";

const prefix = "/manual/v1/";
const publicRoot = `https://docs.example.test${prefix}`;
const output = new Map<string, string>();
let workspace: string;

test.beforeAll(async () => {
  const binary = process.env.DIPLODOCUS_BROWSER_BINARY;
  if (!binary) throw new Error("Run npm run site-test or npm run site-capture.");
  workspace = await mkdtemp(join(tmpdir(), "diplodocus-navigation-urls-"));
  await mkdir(join(workspace, "docs/guides"), { recursive: true });
  await mkdir(join(workspace, "docs/images"));
  await mkdir(join(workspace, "docs/reference"));
  // Static Rd extraction supplies supported math without executing documentation.
  await mkdir(join(workspace, "r/R"), { recursive: true });
  await mkdir(join(workspace, "r/man"));
  await Promise.all([
    writeFile(join(workspace, "docs/index.qmd"), "# Home\n\n[Read nested guide](guides/nested.qmd)\n"),
    writeFile(join(workspace, "docs/reference/index.qmd"), "# Reference\n\n[Commands](cli.qmd)\n"),
    writeFile(join(workspace, "docs/reference/cli.qmd"), "# Commands\n\nCLI options.\n"),
    writeFile(join(workspace, "docs/guides/nested.qmd"), "# Nested guide\n\nSee [`rmath::equation`].\n\n[Home](../index.qmd)\n\n![Chart](../images/chart.svg)\n"),
    writeFile(join(workspace, "docs/images/chart.svg"), '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 40 20"><path fill="green" d="M0 20 20 0 40 20Z"/></svg>'),
    writeFile(join(workspace, "r/DESCRIPTION"), "Package: navigationmath\nVersion: 1.0.0\nTitle: Navigation Math\nDescription: Static math fixture.\nLicense: MIT\nAuthor: Fixture Authors\nMaintainer: Fixture Authors <fixtures@example.test>\n"),
    writeFile(join(workspace, "r/R/equation.R"), "equation <- function(x) x\n"),
    writeFile(join(workspace, "r/NAMESPACE"), "export(equation)\n"),
    writeFile(join(workspace, "r/man/equation.Rd"), "\\name{equation}\n\\alias{equation}\n\\title{Mathematics}\n\\usage{equation(x)}\n\\arguments{\\item{x}{A value.}}\n\\description{A mathematical equation.}\n\\details{\\deqn{x^2 + y^2 = z^2}}\n\\value{A value.}\n"),
    copyFile("assets/brand/favicon-96x96.png", join(workspace, "logo.png")),
  ]);
  for (const style of ["file", "clean"]) {
    const configuration = join(workspace, "diplodocus.yaml");
    await writeFile(configuration, `project:
  name: "URL navigation"
presentation:
  site-url: "${publicRoot}"
  canonical-url-style: "${style}"
  description: "Nested documentation"
  sections:
    - label: "Reference"
      path: "reference/index.html"
  home:
    tagline: "Home"
    description: "A shared home for the documentation."
    guide: "guides/nested.html"
    actions:
      - label: "Get started"
        path: "guides/nested.html"
    features:
      - title: "Nested documentation"
        description: "Explore guides without moving the navbar."
        path: "guides/nested.html"
  logo:
    repository: "docs"
    path: "logo.png"
  open-graph:
    repository: "docs"
    path: "logo.png"
    alt: "Documentation logo"
repositories:
  - id: "docs"
    path: "."
packages:
  - id: "rmath"
    name: "Navigation math"
    slug: "r"
    ecosystem: "r"
    repository: "docs"
    path: "r"
    metadata-path: "DESCRIPTION"
    targets:
      - id: "api"
        extractor: "r"
        path: "."
        role: "public-api"
content:
  - id: "guide"
    owner: "project"
    repository: "docs"
    path: "docs"
    mount: ""
    format: "qmd"
`);
    const destination = join(workspace, style);
    const built = spawnSync(binary, ["build", "--config", configuration, "--output", destination], { encoding: "utf8" });
    expect(built.status, built.error?.message ?? built.stderr).toBe(0);
    output.set(style, destination);
  }
});

test.afterAll(async () => {
  if (workspace) await rm(workspace, { recursive: true, force: true });
});

for (const style of ["file", "clean"]) {
  test(`${style} URLs preserve nested assets and metadata under a hosting prefix`, async ({ page }, info) => {
    const openSection = async (name: string) => {
      if (info.project.name === "mobile") await page.locator(".mobile-menu > summary").click();
      await page.locator("header .global-nav:visible").getByRole("link", { name, exact: true }).click();
    };
    const port = process.env.DIPLODOCUS_PREVIEW_PORT;
    if (!port) throw new Error("The preview server did not report its port.");
    const origin = `http://127.0.0.1:${port}`;
    const suffix = style === "file" ? ".html" : "";
    const homePath = `${prefix}${style === "file" ? "index.html" : ""}`;
    const nestedPath = `${prefix}guides/nested${suffix}`;
    const errors: string[] = [];
    const missing: string[] = [];
    page.on("pageerror", error => errors.push(error.message));
    let releaseStyle!: () => void;
    let styleRequested!: () => void;
    const stylesheetReady = new Promise<void>(resolve => { releaseStyle = resolve; });
    const stylesheetRequested = new Promise<void>(resolve => { styleRequested = resolve; });
    await page.route(`${origin}${prefix}**`, async route => {
      let path = decodeURIComponent(new URL(route.request().url()).pathname.slice(prefix.length));
      if (path.endsWith("/katex.min.css")) {
        styleRequested();
        await stylesheetReady;
      }
      if (!path || path.endsWith("/")) path += "index.html";
      else if (!extname(path)) path += ".html";
      const contentTypes: Record<string, string> = {
        ".html": "text/html", ".css": "text/css", ".js": "text/javascript",
        ".json": "application/json", ".svg": "image/svg+xml", ".png": "image/png",
        ".woff2": "font/woff2", ".woff": "font/woff", ".ttf": "font/ttf",
      };
      try {
        const body = await readFile(join(output.get(style)!, path));
        await route.fulfill({ body, contentType: contentTypes[extname(path)] ?? "application/octet-stream" });
      } catch {
        missing.push(path);
        await route.fulfill({ status: 404, body: "Missing generated artifact" });
      }
    });
    try {
      await page.goto(`${origin}${homePath}`);
      await expect(page.locator("body")).toHaveAttribute("data-layout", "home");
      await expect(page.locator(".nav-disclosure")).not.toBeVisible();
      await expect(page.getByRole("heading", { level: 1 })).toHaveCount(1);
      await expect(page.getByRole("main").getByRole("link", { name: "Read nested guide" })).toBeVisible();
      await expect(page.locator('link[href$="katex.min.css"]')).toHaveCount(0);
      const identity = await page.evaluateHandle(() => ({
        document,
        elements: [...document.querySelectorAll("header, header a, #search")],
      }));
      const navbar = page.locator("header .desktop-sections:visible a, #search, .header-actions > .repository-link:visible");
      const positions = await navbar.evaluateAll(elements => elements.map(element => {
        const { x, y, width, height } = element.getBoundingClientRect();
        return { x, y, width, height };
      }));
      const stableNavbar = async () => {
        expect(await identity.evaluate(value => value.document === document && value.elements.every(element => element.isConnected))).toBe(true);
        const boxes = await navbar.evaluateAll(elements => elements.map(element => {
          const { x, y, width, height } = element.getBoundingClientRect();
          return { x, y, width, height };
        }));
        expect(boxes).toEqual(positions);
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
      };
      await stableNavbar();
      await expect(page.locator(".brand")).toBeVisible();
      const logoSource = await page.locator(".brand-logo").getAttribute("src");
      await openSection("Guide");
      await expect(page).toHaveURL(`${origin}${nestedPath}`);
      await expect(page.locator("body")).toHaveAttribute("data-layout", "docs");
      await stableNavbar();
      await expect(page).toHaveTitle("Nested guide · URL navigation");
      await expect(page.locator('link[rel="canonical"]')).toHaveAttribute("href", `${publicRoot}guides/nested${suffix}`);
      await expect(page.locator('link[rel="alternate"][type="text/markdown"]')).toHaveAttribute("href", `${origin}${prefix}guides/nested.md`);
      await expect(page.locator('meta[property="og:url"]')).toHaveAttribute("content", `${publicRoot}guides/nested${suffix}`);
      await expect(page.locator('meta[property="og:title"]')).toHaveAttribute("content", "Nested guide · URL navigation");
      await expect(page.locator('meta[name="twitter:title"]')).toHaveAttribute("content", "Nested guide · URL navigation");
      await expect(page.locator(".brand")).toHaveAttribute("href", `${origin}${homePath}`);
      await expect(page.locator(".brand-logo")).toHaveAttribute("src", logoSource!);
      await expect(page.locator(".contextual-nav").getByRole("link", { name: "Overview", exact: true, includeHidden: true })).toHaveAttribute("href", `${origin}${nestedPath}`);
      await openSection("Reference");
      await expect(page.getByRole("heading", { level: 1 })).toHaveText("Reference");
      await expect(page.locator('.desktop-sections [aria-current="true"]')).toHaveText("Reference");
      await expect(page.locator(".contextual-nav a")).toHaveText(["Overview", "Commands"]);
      await page.getByRole("main").getByRole("link", { name: "Commands", exact: true }).click();
      await expect(page.getByRole("heading", { level: 1 })).toHaveText("Commands");
      await expect(page.locator('.desktop-sections [aria-current="true"]')).toHaveText("Reference");
      await stableNavbar();
      await openSection("Guide");
      await expect(page.getByRole("heading", { level: 1 })).toHaveText("Nested guide");
      await expect(page.locator(".contextual-nav")).not.toContainText("Commands");
      const chart = page.getByRole("img", { name: "Chart" });
      expect(await chart.getAttribute("src")).toMatch(new RegExp(`^${origin}${prefix}assets/[^/]+\\.svg$`));
      await expect.poll(() => chart.evaluate((image: HTMLImageElement) => image.naturalWidth)).toBeGreaterThan(0);
      const mathLink = page.getByRole("main").getByRole("link", { name: "rmath::equation", exact: true });
      const mathPath = new URL((await mathLink.getAttribute("href"))!, page.url()).pathname;
      const publicMathUrl = `${publicRoot}${mathPath.slice(prefix.length)}`;
      await mathLink.click();
      await stylesheetRequested;
      await expect(page.getByRole("heading", { level: 1 })).toHaveText("Nested guide");
      await expect(page).toHaveURL(`${origin}${nestedPath}`);
      releaseStyle();
      await expect(page).toHaveURL(`${origin}${mathPath}`);
      await expect(page.locator(".math-display .katex")).toHaveCount(1);
      expect(await page.locator('link[href$="katex.min.css"]').evaluate((link: HTMLLinkElement) => link.sheet !== null)).toBe(true);
      await expect(page.locator('link[rel="canonical"]')).toHaveAttribute("href", publicMathUrl);
      await expect(page.locator('meta[property="og:url"]')).toHaveAttribute("content", publicMathUrl);
      await page.locator(".brand").click();
      await expect(page).toHaveURL(`${origin}${homePath}`);
      await expect(page.getByRole("heading", { level: 1 })).toHaveText("Home");
      await expect(page.locator("body")).toHaveAttribute("data-layout", "home");
      await stableNavbar();
      await page.getByRole("link", { name: "Get started", exact: true }).click();
      await expect(page).toHaveURL(`${origin}${nestedPath}`);
      await stableNavbar();
      await page.goBack();
      await expect(page.locator("body")).toHaveAttribute("data-layout", "home");
      await stableNavbar();
      await page.goForward();
      await expect(page.locator("body")).toHaveAttribute("data-layout", "docs");
      await stableNavbar();
      await page.reload();
      await expect(page.locator("body")).toHaveAttribute("data-layout", "docs");
      await page.locator(".brand").click();
      await expect(page.locator("body")).toHaveAttribute("data-layout", "home");
      await page.reload();
      await expect(page.locator("body")).toHaveAttribute("data-layout", "home");
      expect(missing, "Generated assets resolve within the hosting prefix").toEqual([]);
      expect(errors, "Client navigation JavaScript errors").toEqual([]);
    } finally {
      releaseStyle();
    }
  });

  test(`${style} landing and guide navigation work without JavaScript`, async ({ browser }, info) => {
    const context = await browser.newContext({ javaScriptEnabled: false, viewport: info.project.use.viewport });
    const page = await context.newPage();
    const origin = `http://127.0.0.1:${process.env.DIPLODOCUS_PREVIEW_PORT}`;
    const homePath = `${prefix}${style === "file" ? "index.html" : ""}`;
    const nestedPath = `${prefix}guides/nested${style === "file" ? ".html" : ""}`;
    try {
      await page.route(`${origin}${prefix}**`, async route => {
        let path = decodeURIComponent(new URL(route.request().url()).pathname.slice(prefix.length));
        if (!path || path.endsWith("/")) path += "index.html";
        else if (!extname(path)) path += ".html";
        const types: Record<string, string> = {
          ".html": "text/html", ".css": "text/css", ".png": "image/png", ".svg": "image/svg+xml",
        };
        await route.fulfill({
          body: await readFile(join(output.get(style)!, path)),
          contentType: types[extname(path)] ?? "application/octet-stream",
        });
      });
      await page.goto(`${origin}${homePath}`);
      await expect(page.locator("body")).toHaveAttribute("data-layout", "home");
      if (info.project.name === "mobile") await page.locator(".mobile-menu > summary").click();
      await expect(page.locator("header .global-nav:visible")).toBeVisible();
      await expect(page.locator(".brand")).toBeVisible();
      await page.getByRole("link", { name: "Get started", exact: true }).click();
      await expect(page).toHaveURL(`${origin}${nestedPath}`);
      await expect(page.locator("body")).toHaveAttribute("data-layout", "docs");
      await expect(page.getByRole("heading", { level: 1 })).toHaveText("Nested guide");
      await expect(page.locator(".brand")).toBeVisible();
      await page.locator(".brand").click();
      await expect(page).toHaveURL(`${origin}${homePath}`);
      await expect(page.locator("body")).toHaveAttribute("data-layout", "home");
    } finally {
      await context.close();
    }
  });
}
