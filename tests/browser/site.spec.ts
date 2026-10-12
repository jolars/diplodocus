import { expect, test as base, type Page, type TestInfo } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { createHash } from "node:crypto";

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
  ["/packages/julia/", "Tiny Stats for Julia", "julia-package"],
  ["/getting-started/quick-start.html", "Quick start", "quick-start"],
  ["/guides/choosing-a-metric.html", "Choosing a metric", "choosing-a-metric"],
  ["/packages/python/guides/evaluating-models.html", "Evaluating models in Python", "python-guide"],
]) {
  test(`${name} has one title and fits the viewport`, async ({ page }, info) => {
    await page.goto(path);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(title);
    await expect(page.getByRole("navigation", { name: "Documentation" })).toBeVisible();
    await fitsViewport(page);
    if (name === "python-guide") {
      await openNavigation(page, info);
      await expect(sections(page, info).locator('[aria-current="true"]')).toHaveText("Python");
      await expect(page.getByRole("navigation", { name: "Documentation" }).locator('[aria-current="page"]')).toHaveText(title);
      await expect(page.getByRole("navigation", { name: "Documentation" }).getByRole("link", { name: "Choosing a metric" })).toHaveCount(0);
    }
    if (name === "guide") {
      const image = page.getByRole("img");
      await expect(image).toBeVisible();
      await expect.poll(() => image.evaluate((element: HTMLImageElement) => element.naturalWidth)).toBeGreaterThan(0);
    }
    await capture(page, info, name);
  });
}

test("navbar logo loads on nested pages and links home", async ({ page }, info) => {
  for (const path of ["/", "/guides/comparing-predictions.html", "/packages/python/"]) {
    await page.goto(path);
    const brand = page.locator("header .brand");
    const logo = brand.locator(".brand-logo");
    await expect(brand).toHaveAccessibleName("Tiny Stats");
    await expect(logo).toBeVisible();
    await expect(logo).toHaveAttribute("alt", "");
    await expect.poll(() => logo.evaluate((element: HTMLImageElement) => element.naturalWidth)).toBeGreaterThan(0);
    const image = await logo.boundingBox();
    const title = await brand.locator("span").boundingBox();
    expect(image!.x + image!.width).toBeLessThanOrEqual(title!.x);
    expect(image!.height).toBeLessThanOrEqual(40);
    await fitsViewport(page);
    await brand.click();
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Tiny Stats");
  }
  await capture(page, info, "navbar-logo");
});

test("footer identifies the docs and exposes project, license, and generator links", async ({ page }, info) => {
  for (const path of ["/", "/guides/comparing-predictions.html", "/packages/r/"]) {
    await page.goto(path);
    const footer = page.getByRole("contentinfo");
    await expect(footer).toHaveCount(1);
    await expect(footer).toBeVisible();
    if (path === "/guides/comparing-predictions.html") {
      await expect(footer).not.toBeInViewport();
    }
    await expect(footer).toContainText("Docs: dev");
    await expect(footer).toContainText("© 2026 Johan Larsson");
    await expect(footer).toContainText("MIT or Apache-2.0");
    for (const [label, url] of [
      ["GitHub", "https://github.com/jolars/diplodocus"],
      ["Report an issue", "https://github.com/jolars/diplodocus/issues"],
      ["MIT", "https://github.com/jolars/diplodocus/blob/main/LICENSE-MIT"],
      ["Apache-2.0", "https://github.com/jolars/diplodocus/blob/main/LICENSE-APACHE"],
      ["Diplodocus", "https://diplodocus.cc/"],
    ]) {
      await expect(footer.getByRole("link", { name: label, exact: true })).toHaveAttribute("href", url);
    }
    await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
    // Integer scroll positions can leave a fraction of a pixel below the viewport.
    await expect(footer).toBeInViewport({ ratio: 0.99 });
    const layout = await page.locator(".layout").boundingBox();
    const box = await footer.boundingBox();
    expect(box!.y).toBeGreaterThanOrEqual(layout!.y + layout!.height - 1);
    expect(box!.x).toBe(0);
    expect(box!.width).toBe(page.viewportSize()!.width);
    expect(Math.abs(box!.y + box!.height - page.viewportSize()!.height)).toBeLessThan(1);
    await fitsViewport(page);
  }
  await page.evaluate(() => window.scrollTo(0, 0));
  await capture(page, info, "footer");
});

test("search finds all three APIs and follows a result", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("searchbox").fill("mean_squared_error");
  const results = page.locator("#search-results");
  const python = results.getByRole("link", { name: "tinystats.mean_squared_error · Tiny Stats for Python", exact: true });
  await expect(python).toBeVisible();
  await expect(python.locator("code")).toHaveText("tinystats.mean_squared_error");
  await expect(results.getByRole("link", { name: "mean_squared_error · Tiny Stats for R", exact: true })).toBeVisible();
  await expect(results.getByRole("link", { name: "TinyStats.mean_squared_error · Tiny Stats for Julia", exact: true })).toBeVisible();
  await python.click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("tinystats.mean_squared_error");
});

test("R package overview exposes metadata and supporting documentation", async ({ page }, info) => {
  await page.goto("/packages/r/");
  const main = page.getByRole("main");
  const metadata = page.getByRole("complementary", { name: "Package information", exact: true });
  await expect(main.getByRole("heading", { level: 1 })).toHaveText("Tiny Stats for R");
  await expect(main.getByRole("heading", { name: "Compare predictions", exact: true })).toBeVisible();
  await expect(metadata).toContainText("tinystats");
  await expect(metadata).toContainText("0.1.0");
  await expect(metadata).toContainText("MIT + file LICENSE");
  await expect(metadata.getByRole("link", { name: "Report an issue" })).toHaveAttribute("href", "https://github.com/jolars/diplodocus/issues");
  await expect(main.getByRole("img")).toBeVisible();
  await expect.poll(() => main.getByRole("img").evaluate((element: HTMLImageElement) => element.naturalWidth)).toBeGreaterThan(0);
  const body = await page.locator(".package-overview-body").boundingBox();
  const panel = await metadata.boundingBox();
  if (info.project.name === "desktop") expect(panel!.x).toBeGreaterThanOrEqual(body!.x + body!.width);
  else expect(panel!.y).toBeGreaterThanOrEqual(body!.y + body!.height);
  await fitsViewport(page);
  await capture(page, info, "r-package-overview");

  for (const [title, expected, filename] of [
    ["News", "tinystats 0.1.0", "news"],
    ["Authors and citation", "Tiny Stats: Prediction Metrics for R", "authors"],
    ["License", "Permission is hereby granted", "license"],
    ["Community", "Contributing to Tiny Stats", "community"],
  ]) {
    await metadata.getByRole("link", { name: title, exact: true }).click();
    await expect(main.getByRole("heading", { level: 1 })).toHaveText(title);
    await expect(main).toContainText(expected);
    await fitsViewport(page);
    await capture(page, info, `r-package-${filename}`);
    await page.goto("/packages/r/");
  }
  await metadata.getByRole("link", { name: "Authors and citation" }).click();
  await expect(main.locator("pre code")).toContainText("@Manual");
  const toc = page.getByRole("navigation", { name: "On this page" });
  if (info.project.name === "mobile") await toc.getByText("On this page", { exact: true }).click();
  await toc.getByRole("link", { name: "Citation", exact: true }).click();
  await expect(page).toHaveURL(/#citation$/);
});

test("R package overview is searchable by DESCRIPTION content", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("searchbox").fill("Small Statistics Example");
  await page.locator("#search-results").getByRole("link", { name: "Tiny Stats for R · Tiny Stats for R", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Tiny Stats for R");
});

test("internal R helpers and grid hooks remain addressable but undiscoverable", async ({ page }, info) => {
  await page.goto("/packages/r/");
  await openNavigation(page, info);
  const navigation = page.getByRole("navigation", { name: "Documentation" });
  const search = await (await page.request.get("/assets/search.json")).json();
  for (const name of ["setup_gpar", "makeContent.EulerGlyphs"]) {
    await expect(navigation.getByRole("link", { name, exact: true })).toHaveCount(0);
    await expect(page.getByRole("main").getByRole("link", { name, exact: true })).toHaveCount(0);
    expect(search.some((entry: { title: string }) => entry.title === name)).toBe(false);
    await page.getByRole("searchbox").fill(name);
    await expect(page.locator("#search-results").getByRole("link")).toHaveCount(0);
  }
  const item = "sid1:r:function:setup_gpar";
  const route = createHash("sha256").update(item).digest("hex");
  await page.goto(`/packages/r/reference/${route}.html`);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("setup_gpar");
  await expect(page.getByRole("main")).toContainText("Prepare parameters.");
  await fitsViewport(page);
  await capture(page, info, "r-internal-helper");
});

for (const [name, format, source] of [
  ["fruits", "100 observations of 5 variables", undefined],
  ["organisms", "7 observations", "VennMaster/blob/master/data_examples/deploy/example1.list"],
  ["pain", "pain distribution in rows", "Westergren H, Larsson J, Freeman M"],
  ["plants", "33,721 plants", "National Plant Data Center"],
]) {
  test(`R dataset ${name} appears in search and retains its documentation`, async ({ page }, info) => {
    await page.goto("/packages/r/");
    await page.getByRole("searchbox").fill(name!);
    await page.locator("#search-results").getByRole("link", { name: `${name} · Tiny Stats for R`, exact: true }).click();
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(name!);
    const main = page.getByRole("main");
    await expect(main.getByRole("heading", { name: "Usage", exact: true })).toBeVisible();
    await expect(main.getByRole("heading", { name: "Format", exact: true })).toBeVisible();
    await expect(main).toContainText(format!);
    await expect(main).not.toContainText(`${name}()`);
    await expect(main.locator("pre code")).toHaveText([name!, name!]);
    if (source) {
      await expect(main.getByRole("heading", { name: "Source", exact: true })).toBeVisible();
      await expect(main).toContainText(source);
    }
    if (name === "fruits") {
      for (const field of ["banana", "apple", "orange", "sex", "age"]) {
        await expect(main).toContainText(field);
      }
    }
    await fitsViewport(page);
    await capture(page, info, `r-dataset-${name}`);
  });
}

async function openNavigation(page: Page, info: TestInfo) {
  if (info.project.name === "mobile") {
    const disclosure = page.locator(".nav-disclosure");
    if (!await disclosure.evaluate((element: HTMLDetailsElement) => element.open)) {
      await disclosure.locator(":scope > summary").click();
    }
  }
}

function sections(page: Page, info: TestInfo) {
  return page.locator(info.project.name === "mobile" ? ".mobile-sections" : ".desktop-sections");
}

test("global sections select a contextual sidebar", async ({ page }, info) => {
  await page.goto("/");
  const navigation = page.getByRole("navigation", { name: "Documentation" });
  const global = sections(page, info);
  if (info.project.name === "mobile") {
    await expect(navigation.getByText("Browse documentation", { exact: true })).toBeVisible();
    await expect(global).not.toBeVisible();
  }
  await openNavigation(page, info);
  await expect(global.getByRole("link")).toHaveText(["Guide", "Python", "R", "Julia"]);
  await expect(global.locator('[aria-current="true"]')).toHaveText("Guide");
  await expect(navigation.getByRole("group", { name: "Guide", exact: true }).getByRole("link", { name: "Overview" })).toBeVisible();
  await expect(navigation.getByRole("group", { name: "Guides", exact: true }).getByRole("link", { name: "Comparing predictions" })).toBeVisible();
  await expect(navigation.getByRole("group", { name: "API reference" })).toHaveCount(0);

  await global.getByRole("link", { name: "Python", exact: true }).click();
  await openNavigation(page, info);
  await expect(global.locator('[aria-current="true"]')).toHaveText("Python");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Tiny Stats for Python");
  await expect(navigation.getByRole("group", { name: "Getting started" })).toHaveCount(0);
  await expect(navigation.getByRole("link", { name: "Comparing predictions" })).toHaveCount(0);
  await expect(navigation.getByRole("link", { name: "Evaluating models in Python" })).toBeVisible();
  const api = navigation.getByRole("group", { name: "API reference" });
  await expect(api.getByRole("link", { name: "mean_squared_error", exact: true })).not.toBeVisible();
  await api.getByText("tinystats", { exact: true }).click();
  await api.getByRole("link", { name: "mean_squared_error", exact: true }).click();
  await openNavigation(page, info);
  await expect(global.locator('[aria-current="true"]')).toHaveText("Python");
  await expect(navigation.locator('[aria-current="page"]')).toHaveText("mean_squared_error");
  await expect(navigation.locator('[aria-current="page"]')).toBeVisible();
  const functionCode = navigation.locator('[aria-current="page"] code');
  const proseFont = await navigation.getByRole("link", { name: "Evaluating models in Python" }).evaluate(element => getComputedStyle(element).fontFamily);
  expect(await functionCode.evaluate(element => getComputedStyle(element).fontFamily)).not.toBe(proseFont);
  await capture(page, info, "navigation");

  await global.getByRole("link", { name: "R", exact: true }).click();
  await openNavigation(page, info);
  await expect(global.locator('[aria-current="true"]')).toHaveText("R");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Tiny Stats for R");
  await expect(api.getByRole("link", { name: "mean_squared_error", exact: true })).toBeVisible();
  await expect(navigation.getByRole("link", { name: "Evaluating models in Python" })).toHaveCount(0);
  await expect(navigation.getByRole("group", { name: "Guides", exact: true })).toHaveCount(0);
  await global.getByRole("link", { name: "Guide", exact: true }).click();
  await openNavigation(page, info);
  await expect(global.locator('[aria-current="true"]')).toHaveText("Guide");
  await expect(navigation.getByRole("link", { name: "Comparing predictions" })).toBeVisible();
});

test("equivalent APIs link in both directions", async ({ page }, info) => {
  await page.goto("/guides/comparing-predictions.html");
  await page.getByRole("main").getByRole("link", { name: "pystats::tinystats.mean_squared_error", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("tinystats.mean_squared_error");
  await expect(page.getByRole("heading", { level: 1 }).locator("code")).toHaveText("tinystats.mean_squared_error");
  await fitsViewport(page);
  await capture(page, info, "python-api");
  const sameApi = page.locator("section").filter({ has: page.getByRole("heading", { name: "Same API in", exact: true }) });
  await expect(sameApi.getByRole("link")).toHaveCount(2);
  await expect(sameApi.locator("code")).toHaveText(["TinyStats.mean_squared_error", "mean_squared_error"]);
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
  // Section links are also reachable before search on desktop.
  for (let step = 0; step < 6; step++) {
    await page.keyboard.press("Tab");
    if (await page.getByRole("searchbox").evaluate(element => element === document.activeElement)) break;
  }
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
  await openNavigation(page, info);
  await expect(navigation.getByRole("group", { name: "Getting started" })).toBeVisible();
  const guideLinks = await navigation.getByRole("group", { name: "Guides", exact: true }).getByRole("link").allTextContents();
  await page.getByRole("main").getByRole("link", { name: "pystats::tinystats.mean_absolute_error", exact: true }).click();
  await openNavigation(page, info);
  await expect(sections(page, info).locator('[aria-current="true"]')).toHaveText("Python");
  await expect(navigation.getByRole("link", { name: "Choosing a metric" })).toHaveCount(0);
  const active = navigation.locator('[aria-current="page"]');
  await expect(active).toHaveText("mean_absolute_error");
  await expect(active).toBeVisible();
  const toc = page.getByRole("navigation", { name: "On this page" });
  if (info.project.name === "mobile") await toc.getByText("On this page", { exact: true }).click();
  await toc.getByRole("link", { name: "Parameters", exact: true }).click();
  await expect(page).toHaveURL(/#parameters$/);
  await expect(page.getByRole("main").getByRole("heading", { name: "Parameters", exact: true })).toBeInViewport();
  await sections(page, info).getByRole("link", { name: "Guide", exact: true }).click();
  await openNavigation(page, info);
  await expect(navigation.getByRole("group", { name: "Guides", exact: true }).getByRole("link")).toHaveText(guideLinks);
  await navigation.getByRole("group", { name: "Guides", exact: true }).getByRole("link", { name: "Choosing a metric" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Choosing a metric");
});

test("page TOC stays close to content when the window narrows", async ({ page }, info) => {
  test.skip(info.project.name !== "desktop", "This checks resizing through the desktop breakpoints.");
  await page.setViewportSize({ width: 1280, height: 1400 });
  await page.goto("/getting-started/installation.html");
  for (const width of [1099, 1000, 761, 760]) {
    await page.setViewportSize({ width, height: 1400 });
    await expect(page.locator(".toc-disclosure")).toHaveJSProperty("open", false);
    // Measure both positions together while responsive disclosure changes settle.
    await expect.poll(() => page.evaluate(() => {
      const summary = document.querySelector(".page-toc summary")!.getBoundingClientRect();
      const title = document.querySelector("main h1")!.getBoundingClientRect();
      const gap = title.top - summary.bottom;
      return gap >= 0 && gap <= 40;
    }), { message: `TOC-to-title spacing at ${width}px` }).toBe(true);
    await fitsViewport(page);
  }
  await page.setViewportSize({ width: 761, height: 1000 });
  await capture(page, info, "narrow-desktop-navigation");
  await page.setViewportSize({ width: 1000, height: 1000 });
  await capture(page, info, "medium-page-toc");

  await page.goto("/getting-started/");
  const header = await page.getByRole("banner").boundingBox();
  const title = await page.getByRole("heading", { level: 1 }).boundingBox();
  expect(title!.y - (header!.y + header!.height), "A page without a TOC should not reserve an empty row").toBeLessThanOrEqual(40);
  await capture(page, info, "medium-without-toc");
});

test("Julia families link to documented methods", async ({ page }, info) => {
  await page.goto("/guides/comparing-predictions.html");
  await page.getByRole("main").getByRole("link", { name: "juliastats::TinyStats.mean_squared_error", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("TinyStats.mean_squared_error");
  await expect(page.getByRole("heading", { name: "Methods", exact: true })).toBeVisible();
  await expect(page.getByRole("main").getByText("Calculate the mean squared difference", { exact: false })).toBeVisible();
  await fitsViewport(page);
  await capture(page, info, "julia-family");
  await page.getByRole("main").getByRole("link", { name: "vector method", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("TinyStats.mean_squared_error");
  await expect(page.getByRole("main").getByText("Method", { exact: true })).toBeVisible();
  await expect(page.getByRole("main").locator("pre code")).toHaveText("mean_squared_error(observed::AbstractVector, predicted::AbstractVector)");
  await expect(page.getByRole("main").getByText("Calculate mean squared error for numeric vectors.", { exact: true })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Documentation" }).locator('[aria-current="page"]')).toHaveText("mean_squared_error(observed::AbstractVector, predicted::AbstractVector)");
  await fitsViewport(page);
  await capture(page, info, "julia-method");
});


test.describe("navigation without JavaScript", () => {
  test.use({ javaScriptEnabled: false });

  test("section links and API trees remain usable", async ({ page }, info) => {
    await page.goto("/");
    await sections(page, info).getByRole("link", { name: "Python", exact: true }).click();
    const navigation = page.getByRole("navigation", { name: "Documentation" });
    await navigation.getByText("tinystats", { exact: true }).click();
    await navigation.getByRole("link", { name: "mean_squared_error", exact: true }).click();
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("tinystats.mean_squared_error");
    await expect(navigation.locator('[aria-current="page"]')).toBeVisible();
    await expect(sections(page, info).locator('[aria-current="true"]')).toHaveText("Python");
    await fitsViewport(page);
  });
});

test("structured Rd renders math, tables, sections, and help labels", async ({ page }, info) => {
  await page.goto("/packages/r/");
  const link = page.getByRole("navigation", { name: "Documentation" }).getByRole("link", { name: "rd_content", exact: true });
  await openNavigation(page, info);
  await link.click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("rd_content");
  await expect(page.locator(".math-display .katex")).toHaveCount(4);
  await expect(page.locator(".math:not(.math-display) .katex")).toHaveCount(2);
  await expect(page.locator(".katex-mathml math")).toHaveCount(6);
  await expect(page.getByRole("table")).toHaveCount(1);
  await expect(page.getByRole("table").locator("tr").first().locator("td")).toHaveCount(9);
  await expect(page.getByRole("heading", { level: 2, name: "Graphical parameters", exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { level: 3, name: "Links", exact: true })).toBeVisible();
  await expect(page.getByRole("main").getByRole("link", { name: "graphical parameters", exact: true })).toHaveAttribute("href", "https://rdrr.io/r/grid/gpar.html");
  await expect(page.getByRole("main").getByRole("link", { name: "rd_content()", exact: true }).locator("code")).toHaveText("rd_content()");
  await expect(page.getByRole("main")).toContainText("HTML-only content.");
  await expect(page.getByRole("main")).not.toContainText("Text-only content.");
  await expect(page.locator(".unsupported")).toHaveCount(0);
  await expect(page.locator('script[src*="katex"], script[src*="math.js"]')).toHaveCount(0);
  await fitsViewport(page);
  await capture(page, info, "r-rd-content");
});

test("Rd equations remain typeset with JavaScript disabled", async ({ browser, baseURL }, info) => {
  const context = await browser.newContext({ javaScriptEnabled: false, baseURL, viewport: info.project.use.viewport });
  try {
    const page = await context.newPage();
    await page.goto("/packages/r/");
    const link = page.getByRole("navigation", { name: "Documentation" }).getByRole("link", { name: "rd_content", exact: true });
    await page.goto(new URL((await link.getAttribute("href"))!, page.url()).href);
    await expect(page.locator(".math-display .katex")).toHaveCount(4);
    await expect(page.locator(".katex-mathml math")).toHaveCount(6);
    await fitsViewport(page);
  } finally {
    await context.close();
  }
});
