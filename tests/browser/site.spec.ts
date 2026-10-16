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

async function capture(page: Page, info: TestInfo, name: string, waitForFrame = true, fullPage = true) {
  if (!process.env.DIPLODOCUS_BROWSER_CAPTURE) return;
  const directory = join(process.env.DIPLODOCUS_BROWSER_ARTIFACTS!, "screenshots", info.project.name);
  await mkdir(directory, { recursive: true });
  const path = join(directory, `${name}.png`);
  // Chromium can finish navigation before its first composited frame is ready.
  if (waitForFrame) {
    await page.evaluate(() => new Promise<void>(resolve => {
      requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    }));
  }
  await page.screenshot({ path, fullPage, animations: "disabled" });
  await info.attach(name, { path, contentType: "image/png" });
}

async function fitsViewport(page: Page) {
  await expect.poll(() => page.evaluate(() =>
    document.documentElement.scrollWidth <= document.documentElement.clientWidth,
  )).toBe(true);
}

test("code chunks and API signatures stay highlighted without JavaScript", async ({ browser, baseURL }, info) => {
  const context = await browser.newContext({ javaScriptEnabled: false, baseURL, viewport: info.project.use.viewport });
  const page = await context.newPage();
  try {
    await page.goto("/guides/comparing-predictions.html");
    const blocks = page.locator("main pre.syntax-code");
    await expect(blocks).toHaveCount(4);
    for (const block of await blocks.all()) {
      const colors = await block.locator("span").evaluateAll(elements =>
        [...new Set(elements.map(element => getComputedStyle(element).color))],
      );
      expect(colors.length).toBeGreaterThan(1);
    }
    await expect(blocks.nth(0).locator("code")).toHaveText("from tinystats import mean_squared_error\n\nmean_squared_error([1.0, 2.0], [1.0, 3.0])\n# 0.5\n", { useInnerText: false });
    await fitsViewport(page);
    // Disabled JavaScript prevents animation-frame callbacks from running.
    await capture(page, info, "syntax-highlighting", false);

    for (const language of ["Python", "R", "Julia", "Rust"]) {
      await page.goto("/guides/comparing-predictions.html");
      const heading = page.getByRole("heading", { name: language, exact: true });
      const href = await heading.locator("xpath=following-sibling::p[1]").getByRole("link").getAttribute("href");
      await page.goto(new URL(href!, page.url()).href);
      const signature = page.locator("main pre.syntax-code").first();
      await expect(signature).toBeVisible();
      expect(await signature.locator("span").count()).toBeGreaterThan(0);
      await fitsViewport(page);
    }
  } finally {
    await context.close();
  }
});

for (const [path, title, name] of [
  ["/", "Tiny Stats", "home"],
  ["/guides/comparing-predictions.html", "Comparing predictions", "guide"],
  ["/packages/python/", "Tiny Stats for Python", "python-package"],
  ["/packages/r/", "Tiny Stats for R", "r-package"],
  ["/packages/julia/", "Tiny Stats for Julia", "julia-package"],
  ["/packages/rust/", "Tiny Stats for Rust", "rust-package"],
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

test("navbar and sidebars share outer margins", async ({ page }, info) => {
  test.skip(info.project.name !== "desktop", "The sidebars stack on mobile.");
  await page.goto("/guides/comparing-predictions.html");
  for (const width of [1024, 1280, 1920]) {
    await page.setViewportSize({ width, height: 900 });
    const brand = (await page.locator(".layout > nav > .brand").boundingBox())!;
    const search = (await page.locator(".header-actions").boundingBox())!;
    const sidebar = (await page.locator(".layout > nav .nav-group h2").first().boundingBox())!;
    expect(brand.x).toBeCloseTo(width - search.x - search.width, 0);
    expect(sidebar.x).toBeCloseTo(brand.x, 0);
    if (width >= 1100) {
      const toc = (await page.locator(".page-toc").boundingBox())!;
      expect(toc.x + toc.width).toBeCloseTo(search.x + search.width, 0);
      const article = (await page.getByRole("main").boundingBox())!;
      expect(toc.x).toBeCloseTo(article.x + article.width, 0);
    }
    await fitsViewport(page);
    await capture(page, info, `navbar-alignment-${width}`);
  }
});

test("desktop sidebar stays pinned at the page bottom and scrolls independently", async ({ page }, info) => {
  test.skip(info.project.name !== "desktop", "Mobile navigation stays in the page flow.");
  await page.setViewportSize({ width: 1280, height: 400 });
  await page.goto("/guides/comparing-predictions.html");
  const sidebar = page.getByRole("navigation", { name: "Documentation", exact: true });
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  await expect(page.locator(".site-footer")).toBeInViewport();
  const box = (await sidebar.boundingBox())!;
  expect(box.y).toBeCloseTo(0, 0);
  expect(box.height).toBeCloseTo(400, 0);
  const pageScroll = await page.evaluate(() => window.scrollY);
  await sidebar.hover();
  await page.mouse.wheel(0, 300);
  await expect.poll(() => sidebar.locator(".nav-disclosure").evaluate(element => element.scrollTop)).toBeGreaterThan(0);
  expect(await page.evaluate(() => window.scrollY)).toBe(pageScroll);
  await capture(page, info, "sidebar-at-page-bottom", true, false);
});

test("sidebar branding stays visible while reaching lower navigation links", async ({ page }, info) => {
  test.skip(info.project.name !== "desktop", "Mobile navigation stays in the page flow.");
  await page.setViewportSize({ width: 1280, height: 400 });
  await page.goto("/");
  const sidebar = page.getByRole("navigation", { name: "Documentation", exact: true });
  const brand = sidebar.locator(":scope > .brand");
  const before = (await brand.boundingBox())!;
  const link = sidebar.getByRole("link", { name: "Inspecting residuals", exact: true });
  await link.scrollIntoViewIfNeeded();
  expect(await sidebar.locator(".nav-disclosure").evaluate(element => element.scrollTop)).toBeGreaterThan(0);
  const after = (await brand.boundingBox())!;
  expect(after.y).toBeCloseTo(before.y, 0);
  await expect(brand).toBeInViewport({ ratio: 1 });
  await link.click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Inspecting residuals");
  await expect(brand).toBeInViewport({ ratio: 1 });
  await capture(page, info, "sidebar-branding", true, false);
});

test("navbar logo loads on nested pages and links home", async ({ page }, info) => {
  for (const path of ["/", "/guides/comparing-predictions.html", "/packages/python/"]) {
    await page.goto(path);
    const brand = page.locator(".layout > nav > .brand");
    const logo = brand.locator(".brand-logo");
    await expect(brand).toHaveAccessibleName("Tiny Stats");
    await expect(logo).toBeVisible();
    await expect(logo).toHaveAttribute("alt", "");
    await expect.poll(() => logo.evaluate((element: HTMLImageElement) => element.naturalWidth)).toBeGreaterThan(0);
    const image = await logo.boundingBox();
    const title = await brand.locator("span").boundingBox();
    expect(image!.x + image!.width).toBeLessThanOrEqual(title!.x);
    expect(image!.height).toBeLessThanOrEqual(28);
    await fitsViewport(page);
    await brand.click();
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Tiny Stats");
  }
  await capture(page, info, "navbar-logo");
});

test("discovered community links preserve repository and site destinations", async ({ page }, info) => {
  await page.goto("/packages/r/community.html");
  const main = page.getByRole("main");
  await expect(main.getByRole("heading", { level: 1 })).toHaveText("Community");
  for (const [label, path] of [
    ["specification", "spec/README.md#contract"],
    ["trust policy", "TRUST.md"],
    ["operations guide", "OPERATIONS.md"],
  ]) {
    await expect(main.getByRole("link", { name: label, exact: true }))
      .toHaveAttribute("href", `https://github.com/jolars/diplodocus/blob/main/${path}`);
  }
  await expect(main.getByRole("link", { name: "documentation", exact: true }))
    .toHaveAttribute("href", "../../index.html");
  await fitsViewport(page);
  await capture(page, info, "r-community");
  await main.getByRole("link", { name: "documentation", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Tiny Stats");
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
    await expect(footer.getByRole("link", { name: "GitHub", exact: true })).toHaveCount(0);
    await expect(page.locator("header").getByRole("link", { name: "GitHub", exact: true })).toHaveAttribute("href", "https://github.com/jolars/diplodocus");
    await expect(footer).toContainText("Docs: dev");
    await expect(footer).toContainText("© 2026 Johan Larsson");
    await expect(footer).toContainText("MIT or Apache-2.0");
    for (const [label, url] of [
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
    if (info.project.name === "desktop") {
      const sidebar = (await page.locator(".layout > nav").boundingBox())!;
      expect(box!.x).toBeCloseTo(sidebar.x + sidebar.width, 0);
    } else {
      expect(box!.x).toBe(0);
    }
    expect(box!.x + box!.width).toBe(page.viewportSize()!.width);
    expect(Math.abs(box!.y + box!.height - page.viewportSize()!.height)).toBeLessThan(1);
    await fitsViewport(page);
  }
  await page.evaluate(() => window.scrollTo(0, 0));
  await capture(page, info, "footer");
});

test("search finds all four APIs and follows a result", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("searchbox").fill("mean_squared_error");
  const results = page.locator("#search-results");
  const python = results.getByRole("link", { name: "tinystats.mean_squared_error · Tiny Stats for Python", exact: true });
  await expect(python).toBeVisible();
  await expect(python.locator("code")).toHaveText("tinystats.mean_squared_error");
  await expect(results.getByRole("link", { name: "mean_squared_error · Tiny Stats for R", exact: true })).toBeVisible();
  await expect(results.getByRole("link", { name: "TinyStats.mean_squared_error · Tiny Stats for Julia", exact: true })).toBeVisible();
  await expect(results.getByRole("link", { name: "tinystats::mean_squared_error · Tiny Stats for Rust", exact: true })).toBeVisible();
  await python.click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("tinystats.mean_squared_error");
});

test("Python overloads have one searchable page with every signature", async ({ page }, info) => {
  await page.goto("/");
  await page.getByRole("searchbox").fill("fetch_data");
  const result = page.locator("#search-results").getByRole("link", {
    name: "tinystats.fetch_data · Tiny Stats for Python", exact: true,
  });
  await expect(result).toHaveCount(1);
  await result.click();
  const main = page.getByRole("main");
  await expect(main.getByRole("heading", { level: 1 })).toHaveText("tinystats.fetch_data");
  await expect(main).toContainText("Retrieve one dataset, optionally returning its metadata.");
  const signatures = main.locator('[id^="overload-"]');
  await expect(signatures).toHaveCount(3);
  await expect(signatures.nth(0)).toContainText("Literal[False]");
  await expect(signatures.nth(1)).toContainText("Literal[True]");
  await expect(signatures.nth(2)).toContainText("return_info: bool");
  await openNavigation(page, info);
  const reference = page.getByRole("group", { name: "API reference", exact: true });
  await expect(reference.getByRole("link", { name: "fetch_data", exact: true })).toHaveCount(1);
  await fitsViewport(page);
  await capture(page, info, "python-overloads");
  const destination = page.url();
  await page.getByRole("searchbox").fill("tinystats._api.fetch_data");
  const canonicalResult = page.locator("#search-results").getByRole("link", {
    name: "tinystats.fetch_data · Tiny Stats for Python", exact: true,
  });
  await expect(canonicalResult).toHaveCount(1);
  await canonicalResult.click();
  await expect(page).toHaveURL(destination);
  await capture(page, info, "python-public-reexport-search");
});

test("Python classes retain overloaded methods on their class page", async ({ page }, info) => {
  await page.goto("/");
  await page.getByRole("searchbox").fill("Reader.read");
  const result = page.locator("#search-results").getByRole("link", {
    name: "tinystats.Reader · Tiny Stats for Python", exact: true,
  });
  await expect(result).toHaveCount(1);
  await expect(page.locator("#search-results").getByRole("link", {
    name: "tinystats.Reader.read · Tiny Stats for Python", exact: true,
  })).toHaveCount(0);
  await result.click();
  const main = page.getByRole("main");
  await expect(main.getByRole("heading", { level: 1 })).toHaveText("tinystats.Reader");
  await expect(main.getByRole("heading", { level: 2, name: "read", exact: true })).toHaveAttribute("id", "read");
  await expect(main.locator('[id^="overload-"]')).toHaveCount(2);
  await expect(main).toContainText("read(value: str) -> str");
  await expect(main).toContainText("read(value: bytes) -> bytes");
  await expect(main).toContainText("Return the artifact in its original representation.");
  await openNavigation(page, info);
  const reference = page.getByRole("group", { name: "API reference", exact: true });
  await expect(reference.getByRole("link", { name: "Reader", exact: true })).toHaveCount(1);
  await expect(reference.getByRole("link", { name: "read", exact: true })).toHaveCount(0);
  await fitsViewport(page);
  await capture(page, info, "python-class");
});

test("Rust type pages group fields and methods with searchable anchors", async ({ page }, info) => {
  await page.goto("/");
  await page.getByRole("searchbox").fill("tinystats::Score");
  await page.locator("#search-results").getByRole("link", {
    name: "tinystats::Score · Tiny Stats for Rust", exact: true,
  }).click();
  const main = page.getByRole("main");
  await expect(main.getByRole("heading", { level: 1 })).toHaveText("tinystats::Score");
  await expect(main.locator("#field-value")).toContainText("Computed metric value.");
  await expect(main.locator("#field-observations")).toContainText("Number of observations.");
  await expect(main.getByRole("heading", { name: "mse", exact: true })).toHaveCount(1);
  await expect(main).toContainText("pub fn mse(observed: &[f64], predicted: &[f64]) -> Self");
  await expect(main).toContainText("impl<T> Any for T");
  await expect(main).not.toContainText("$crate::");
  const pageUrl = new URL(page.url());
  await fitsViewport(page);
  await capture(page, info, "rust-type");
  await page.getByRole("searchbox").fill("tinystats::Score::mse");
  await page.locator("#search-results").getByRole("link", {
    name: "tinystats::Score::mse · Tiny Stats for Rust", exact: true,
  }).click();
  const methodUrl = new URL(page.url());
  expect(methodUrl.pathname).toBe(pageUrl.pathname);
  expect(methodUrl.hash).toMatch(/^#method-mse-/);
  await expect(main.getByRole("heading", { level: 1 })).toHaveText("tinystats::Score");
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
  await page.goto("/packages/r/reference/setup_gpar.html");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Internal helper");
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
    await expect(page.getByRole("heading", { level: 1 })).toHaveText({ fruits: "Fruits", organisms: "Organisms", pain: "Pain distribution data", plants: "Plants" }[name!]);
    const main = page.getByRole("main");
    await expect(main.getByRole("heading", { name: "Usage", exact: true })).toBeVisible();
    await expect(main.getByRole("heading", { name: "Format", exact: true })).toBeVisible();
    await expect(main).toContainText(format!);
    await expect(main).not.toContainText(`${name}()`);
    await expect(main.locator("pre code")).toHaveText([name!]);
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
  await expect(global.getByRole("link")).toHaveText(["Guide", "Python", "R", "Julia", "Rust"]);
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
  const parameters = page.locator("#parameters + dl");
  await expect(parameters.locator(":scope > dt")).toHaveText(["actual : list of float", "predicted : list of float"]);
  await expect(parameters.locator(":scope > dd")).toHaveCount(2);
  await expect(page.locator("#returns + dl > dt")).toHaveText("float");
  await expect(page.locator("#raises + dl > dt")).toHaveText("ValueError");
  await capture(page, info, "python-api");
  const sameApi = page.locator("section").filter({ has: page.getByRole("heading", { name: "Same API in", exact: true }) });
  await expect(sameApi.getByRole("link")).toHaveCount(3);
  await expect(sameApi.locator("code")).toHaveText(["TinyStats.mean_squared_error", "mean_squared_error", "tinystats::mean_squared_error"]);
  await sameApi.getByRole("link", { name: "Tiny Stats for Rust: tinystats::mean_squared_error", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("tinystats::mean_squared_error");
  await expect(page.locator("main pre.syntax-code").first()).toContainText("pub fn mean_squared_error");
  await fitsViewport(page);
  await capture(page, info, "rust-api");
  await sameApi.getByRole("link", { name: "Tiny Stats for Python: tinystats.mean_squared_error", exact: true }).click();
  await sameApi.getByRole("link", { name: "Tiny Stats for R: mean_squared_error", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Mean squared error", exact: true })).toBeVisible();
  await fitsViewport(page);
  await expect(page.getByRole("main")).toContainText("[1] 0.5");
  const figure = page.getByRole("main").getByRole("img", { name: "Example figure", exact: true });
  await expect(figure).toBeVisible();
  await expect(figure).toHaveJSProperty("complete", true);
  expect(await figure.evaluate((image: HTMLImageElement) => image.naturalWidth)).toBeGreaterThan(0);
  await expect(figure).toHaveAttribute("src", /assets\/[a-f0-9]{64}\.png$/);
  await expect(page.locator("#arguments + dl > dt")).toHaveText(["actual", "predicted"]);
  await expect(page.locator("#arguments + dl > dd")).toHaveCount(2);
  await capture(page, info, "r-api");
  await page.getByRole("link", { name: "Tiny Stats for Python: tinystats.mean_squared_error", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("tinystats.mean_squared_error");
});

test("search and result navigation work with the keyboard", async ({ page }) => {
  await page.goto("/");
  await page.keyboard.press("Tab");
  await expect(page.getByRole("link", { name: "Skip to content" })).toBeFocused();
  // Section links are also reachable before search on desktop.
  const headerStops = await page.locator("header a:visible").count() + 1;
  for (let step = 0; step < headerStops; step++) {
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

test("Julia functions include methods on one page", async ({ page }, info) => {
  await page.goto("/guides/comparing-predictions.html");
  await page.getByRole("main").getByRole("link", { name: "juliastats::TinyStats.mean_squared_error", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("TinyStats.mean_squared_error");
  await expect(page.getByRole("heading", { name: "Methods", exact: true })).toBeVisible();
  await expect(page.getByRole("main").getByText("Calculate the mean squared difference", { exact: false })).toBeVisible();
  await fitsViewport(page);
  await capture(page, info, "julia-family");
  await page.getByRole("main").getByRole("link", { name: "vector method", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("TinyStats.mean_squared_error");
  await expect(page).toHaveURL(/#method-/);
  const method = page.getByRole("main").locator('section[id^="method-"]').first();
  await expect(method).toBeInViewport();
  await expect(method.getByRole("heading", { level: 3 })).toHaveText("mean_squared_error(observed::AbstractVector, predicted::AbstractVector)");
  await expect(method).toContainText("Calculate mean squared error for numeric vectors.");
  await expect(page.getByRole("navigation", { name: "Documentation" }).locator('[aria-current="page"]')).toHaveText("mean_squared_error");
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
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Structured R documentation");
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

test("shared R topics retain all usages and navigate methods by class", async ({ page }, info) => {
  await page.goto("/packages/r/");
  await openNavigation(page, info);
  await page.getByRole("navigation", { name: "Documentation" }).getByRole("link", { name: "fit", exact: true }).click();
  const main = page.getByRole("main");
  await expect(main.locator("pre code")).toHaveText([
    "fit(x, ...)", "fit.default(x, y, ...)", "fit.model(x, weights = NULL, ...)",
  ]);
  await expect(main).toContainText("Response values for the default method.");
  await expect(main).toContainText("Observation weights for the model method.");
  const methods = main.getByRole("navigation", { name: "S3 methods by class" });
  const toc = page.getByRole("navigation", { name: "On this page" });
  if (info.project.name === "mobile") await toc.getByText("On this page", { exact: true }).click();
  await toc.getByRole("link", { name: "Methods by class", exact: true }).click();
  await expect(page).toHaveURL(/#methods-by-class$/);
  await expect(methods.getByRole("heading", { name: "Methods by class", exact: true })).toBeInViewport();
  await expect(methods.getByRole("heading", { name: "Class default", exact: true })).toBeVisible();
  await expect(methods.getByRole("heading", { name: "Class model", exact: true })).toBeVisible();
  await fitsViewport(page);
  await page.evaluate(() => window.scrollTo(0, 0));
  await capture(page, info, "r-shared-generic");
  await methods.getByRole("link", { name: "fit.model", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Fit a Model");
  await expect(main.locator("pre code")).toHaveCount(3);
  await expect(main).toContainText("Response values for the default method.");
  await fitsViewport(page);
  await capture(page, info, "r-shared-method");
  await main.getByRole("link", { name: "model-fitting", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Fit a Model");
  await main.getByRole("link", { name: "eulergram-compose", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Compose Euler Diagrams");
  await expect(main.locator("pre code").nth(0)).toHaveText("`|.eulergram`(e1, e2)");
  await expect(main.locator("pre code").nth(1)).toHaveText("`/.eulergram`(e1, e2)");
  await expect(methods.getByRole("heading", { name: "Class eulergram", exact: true })).toBeVisible();
  await fitsViewport(page);
  await capture(page, info, "r-shared-operators");
  await methods.getByRole("link", { name: "/.eulergram", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Compose Euler Diagrams");
  await expect(main.locator("pre code").nth(0)).toHaveText("`|.eulergram`(e1, e2)");
});

test("R references show descriptions, summaries, readable URLs, and source links", async ({ page }, info) => {
  await page.goto("/packages/r/reference/index.html");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Reference");
  const topics = page.locator(".reference-topics");
  await expect(topics.getByRole("link", { name: "fit", exact: true })).toHaveCount(1);
  await expect(topics.getByRole("link", { name: "fit.model", exact: true })).toHaveCount(0);
  await expect(topics).toContainText("Fit a model with class-specific arguments.");
  await expect(topics).not.toContainText("setup_gpar");
  await fitsViewport(page);
  await capture(page, info, "r-reference-index");
  await topics.getByRole("link", { name: "mean_squared_error", exact: true }).click();
  await expect(page).toHaveURL(/\/reference\/mean_squared_error\.html$/);
  await expect(page).toHaveTitle("Mean squared error · Tiny Stats");
  const main = page.getByRole("main");
  await expect(main.getByRole("heading", { level: 1 })).toHaveText("Mean squared error");
  await expect(main.locator(".api-name code")).toHaveText("mean_squared_error");
  await expect(main.locator("pre code").first()).toHaveText("mean_squared_error(actual, predicted)");
  const description = main.getByText("Compute the mean squared difference between observations and predictions.", { exact: true });
  const usage = main.getByRole("heading", { name: "Usage", exact: true });
  expect((await description.boundingBox())!.y).toBeLessThan((await usage.boundingBox())!.y);
  await expect(main.locator(".source-links a")).toHaveCount(2);
  await expect(main.locator(".source-links").getByRole("link", { name: "r/man/mean_squared_error.Rd", exact: true })).toHaveAttribute("href", "https://github.com/jolars/diplodocus/blob/main/r/man/mean_squared_error.Rd");
  await expect(main.locator("pre.syntax-code").first().locator('span[class^="syntax-"]').first()).toBeVisible();
  await fitsViewport(page);
});

test("copy buttons preserve highlighted code and support keyboard and fallback copying", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto("/packages/r/reference/mean_squared_error.html");
  const block = page.getByRole("main").locator(".code-block").first();
  const source = await block.locator("pre code").textContent();
  const copy = block.getByRole("button", { name: "Copy code", exact: true });
  await copy.focus();
  await page.keyboard.press("Enter");
  await expect(block.getByRole("status")).toHaveText("Copied");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(source);

  await page.evaluate(async () => {
    await navigator.clipboard.writeText("Before fallback copying");
    navigator.clipboard.writeText = async () => { throw new Error("Clipboard permission denied"); };
  });
  await copy.click();
  await expect(block.getByRole("status")).toHaveText("Copied");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(source);
  await expect(copy).toBeFocused();
  await expect(page.locator(".copy-buffer")).toHaveCount(0);

  await page.evaluate(() => { document.execCommand = () => false; });
  await copy.click();
  await expect(block.getByRole("status")).toHaveText("Unable to copy. Select the code to copy it.");
  await expect(block.locator("pre code")).toHaveText(source!);
});


test("Tiny Stats vignette evaluates predictions", async ({ page }, info) => {
  await page.goto("/packages/r/");
  await page.getByRole("main").getByRole("link", { name: "R vignette", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Evaluating predictions in R");
  const main = page.getByRole("main");
  await expect(main).toContainText("The mean squared error is 1, and the mean absolute error is 0.5");
  await expect(main.locator("table tr")).toHaveCount(6);
  await expect(main.locator(".katex")).toHaveCount(1);
  await expect(main.locator("figure figcaption")).toHaveText("The last prediction accounts for all of the error.");
  const image = main.getByRole("img", { name: "Three residuals are zero; the residual for observed value four is minus two." });
  await expect(image).toBeVisible();
  expect(await image.evaluate((el: HTMLImageElement) => el.naturalWidth)).toBeGreaterThan(0);
  await fitsViewport(page);
  await capture(page, info, "r-tinystats-vignette");
  await main.getByRole("link", { name: "Choosing a metric", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Choosing a metric");
});

test("R Markdown articles render collected results", async ({ page }, info) => {
  await page.goto("/packages/r/vignettes/article.html");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("R Markdown workflow");
  await expect(page.locator(".article-metadata")).toContainText("Example Author");
  await expect(page.locator("main")).toContainText("The mean is 2.5");
  await expect(page.locator("main table tr")).toHaveCount(5);
  await expect(page.locator("main .katex")).toHaveCount(1);
  await expect(page.locator("main figure figcaption")).toContainText("Squares of the input values.");
  const image = page.getByRole("img", { name: "Four points increasing from one to sixteen" });
  await expect(image).toBeVisible();
  expect(await image.evaluate((el: HTMLImageElement) => el.naturalWidth)).toBeGreaterThan(0);
  await expect(page.locator(".bibliography-entry")).toContainText("Smith");
  await expect(page.locator(".footnotes")).toContainText("Chunks share state");
  await expect(page.locator("main")).not.toContainText("knitr::opts_chunk");
  await fitsViewport(page);
  await capture(page, info, "r-vignette");
});

test("delayed navigation initialization preserves the initial layout", async ({ page }) => {
  let release!: () => void;
  const ready = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/assets/nav.js", async route => {
    const response = await route.fetch();
    await ready;
    await route.fulfill({ response });
  });
  try {
    await page.goto("/guides/comparing-predictions.html", { waitUntil: "commit" });
    await page.getByRole("main").waitFor();
    await page.waitForFunction(() => getComputedStyle(document.querySelector("main")!).paddingTop === "32px" || getComputedStyle(document.querySelector("main")!).paddingTop === "24px");
    const before = await page.getByRole("main").boundingBox();
    release();
    await page.waitForLoadState("networkidle");
    const after = await page.getByRole("main").boundingBox();
    expect(after!.y).toBeCloseTo(before!.y, 0);
  } finally {
    release();
    await page.unrouteAll({ behavior: "ignoreErrors" });
  }
});

test("overflowing code can be scrolled with the keyboard", async ({ page }) => {
  await page.goto("/guides/comparing-predictions.html");
  const blocks = page.locator("main pre");
  const index = await blocks.evaluateAll(elements => elements.findIndex(el => el.scrollWidth > el.clientWidth));
  expect(index).toBeGreaterThanOrEqual(0);
  const block = blocks.nth(index);
  await expect(block).toHaveAttribute("tabindex", "0");
  await block.locator("..").getByRole("button", { name: "Copy code", exact: true }).focus();
  await page.keyboard.press("Tab");
  await expect(block).toBeFocused();
  await page.keyboard.press("ArrowRight");
  await expect.poll(() => block.evaluate(el => el.scrollLeft)).toBeGreaterThan(0);
});
