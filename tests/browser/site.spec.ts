import { expect, test as base, type Page, type TestInfo } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

const homeHeading = "Small metrics. Clearer predictions.";

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

test("API signatures have stable line breaks and no copy buttons", async ({ page }, info) => {
  for (const language of ["Python", "R", "Julia", "Rust"]) {
    await page.goto("/guides/comparing-predictions.html");
    const heading = page.getByRole("heading", { name: language, exact: true });
    const href = await heading.locator("xpath=following-sibling::p[1]").getByRole("link").getAttribute("href");
    await page.goto(new URL(href!, page.url()).href);
    const signatures = page.locator("main pre[data-signature]");
    await expect(signatures.first()).toBeVisible();
    await expect(page.locator(".code-block pre[data-signature]")).toHaveCount(0);
    const text = await signatures.allTextContents();
    const viewport = page.viewportSize()!;
    await page.setViewportSize({ ...viewport, width: viewport.width > 600 ? 600 : 320 });
    expect(await signatures.allTextContents()).toEqual(text);
    await expect(signatures.first()).toHaveCSS("white-space", "pre");
    await fitsViewport(page);
    await page.setViewportSize(viewport);
    await capture(page, info, `signature-${language.toLowerCase()}`);
  }
});

for (const [path, title, name] of [
  ["/", homeHeading, "home"],
  ["/overview.html", "Tiny Stats", "overview"],
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
    const navigation = page.getByRole("navigation", { name: "Documentation" });
    if (name === "home") await expect(navigation.locator(".nav-disclosure")).not.toBeVisible();
    else await expect(navigation).toBeVisible();
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

test("navbar links stay in place when switching sections", async ({ page }, info) => {
  test.skip(info.project.name !== "desktop", "The top navbar is shown on desktop.");
  await page.goto("/overview.html");
  const links = page.locator("header .global-nav:visible a");
  const positions = await links.evaluateAll(elements => elements.map(element => {
    const { x, width } = element.getBoundingClientRect();
    return { x, width };
  }));
  for (let selected = 1; selected <= positions.length; selected++) {
    const index = selected % positions.length;
    await links.nth(index).click();
    await expect(links.nth(index)).toHaveAttribute("aria-current", "true");
    for (let link = 0; link < positions.length; link++) {
      const bounds = (await links.nth(link).boundingBox())!;
      expect(bounds.x).toBeCloseTo(positions[link].x, 1);
      expect(bounds.width).toBeCloseTo(positions[link].width, 1);
    }
  }
  await capture(page, info, "navbar-stable-sections");
});

test("landing navbar stays opaque and its border spans the viewport after scrolling", async ({ page }, info) => {
  test.skip(info.project.name !== "desktop", "The upper navbar scrolls away on phones.");
  await page.setViewportSize({ width: 1280, height: 600 });
  await page.goto("/");
  const border = () => page.locator("header").evaluate(element => {
    const style = getComputedStyle(element, "::after");
    return { opacity: style.opacity, left: style.left, right: style.right, width: parseFloat(style.width) };
  });
  await expect.poll(async () => (await border()).opacity).toBe("0");
  const brand = page.locator("header > .brand");
  const before = await brand.boundingBox();
  const branding = await brand.screenshot({ animations: "disabled" });
  await page.evaluate(() => scrollTo(0, 200));
  await expect.poll(() => page.evaluate(() => scrollY)).toBe(200);
  await expect.poll(async () => (await border()).opacity).toBe("1");
  const visible = await border();
  expect(visible.left).toBe("0px");
  expect(visible.right).toBe("0px");
  expect(visible.width).toBe(await page.evaluate(() => document.body.clientWidth));
  expect(await brand.boundingBox()).toEqual(before);
  expect((await brand.screenshot({ animations: "disabled" })).equals(branding),
    "Page content stays hidden behind the navbar branding").toBe(true);
  await capture(page, info, "landing-navbar-scrolled", true, false);
  await page.evaluate(() => scrollTo(0, 0));
  await expect.poll(async () => (await border()).opacity).toBe("0");
});

test("landing page shares the navbar with guides and all four language sections", async ({ page }, info) => {
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(homeHeading);
  await expect(page.locator(".home-feature")).toHaveCount(4);
  await expect(page.locator(".home-artwork img")).toBeVisible();
  await openSections(page, info);
  await expect(sections(page, info).getByRole("link")).toHaveText(["Guide", "Python", "R", "Julia", "Rust"]);
  await expect(sections(page, info).locator("[aria-current]")).toHaveCount(0);
  const shell = await page.evaluateHandle(() => ({
    document,
    elements: [...document.querySelectorAll("header, header a, .brand-logo, #search")],
  }));
  const geometry = () => page.locator("header > .brand, header .desktop-sections:visible a, #search, .header-actions > .repository-link:visible")
    .evaluateAll(elements => elements.map(element => element.getBoundingClientRect().toJSON()));
  const positions = await geometry();
  const stableNavbar = async () => {
    expect(await shell.evaluate(value => value.document === document && value.elements.every(element => element.isConnected))).toBe(true);
    expect(await geometry()).toEqual(positions);
    await fitsViewport(page);
  };
  await expect(page.locator(".brand")).toBeVisible();
  await stableNavbar();
  await page.getByRole("link", { name: "Get started", exact: true }).click();
  await expect(page.locator(".brand")).toBeVisible();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Quick start");
  await stableNavbar();
  await openSections(page, info);
  await sections(page, info).getByRole("link", { name: "Guide", exact: true }).click();
  await expect(page).toHaveURL(/\/overview\.html$/);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Tiny Stats");
  await stableNavbar();
  await capture(page, info, "landing-guide");
  for (const language of ["Python", "R", "Julia", "Rust"]) {
    await openSections(page, info);
    await sections(page, info).getByRole("link", { name: language, exact: true }).click();
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(`Tiny Stats for ${language}`);
    await expect(sections(page, info).locator('[aria-current="true"]')).toHaveText(language);
    await stableNavbar();
    await page.locator(".brand").click();
    await expect(page.locator("body")).toHaveAttribute("data-layout", "home");
    await expect(page.locator(".nav-disclosure")).not.toBeVisible();
    await expect(sections(page, info).locator("[aria-current]")).toHaveCount(0);
    await stableNavbar();
  }
  await page.getByRole("link", { name: "Explore the guide", exact: true }).click();
  await expect(page).toHaveURL(/\/overview\.html$/);
  await stableNavbar();
  await shell.dispose();
});

test("client navigation keeps the shell and logo mounted while updating the page", async ({ page }, info) => {
  await page.goto("/overview.html");
  const brand = page.locator("header > .brand");
  const logo = brand.locator(".brand-logo");
  await expect.poll(() => logo.evaluate((element: HTMLImageElement) => element.naturalWidth)).toBeGreaterThan(0);
  const shell = await page.evaluateHandle(() => ({
    document,
    elements: ["header", ".desktop-sections", ".mobile-sections", ".layout > nav", ".brand", ".brand-logo", "#search"]
      .map(selector => document.querySelector(selector)!),
  }));
  const before = await brand.boundingBox();
  let loads = 0;
  page.on("load", () => loads++);
  let release!: () => void;
  let requested!: () => void;
  const ready = new Promise<void>(resolve => { release = resolve; });
  const started = new Promise<void>(resolve => { requested = resolve; });
  await page.route("**/guides/comparing-predictions.html", async route => {
    const response = await route.fetch();
    requested();
    await ready;
    await route.fulfill({ response });
  });
  try {
    const link = page.getByRole("main").getByRole("link", { name: "Comparing predictions", exact: true });
    await link.focus();
    await page.keyboard.press("Enter");
    await started;
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Tiny Stats");
    expect(await shell.evaluate(value => value.document === document && value.elements.every(element => element.isConnected))).toBe(true);
    const waiting = await brand.boundingBox();
    expect(waiting!.x).toBeCloseTo(before!.x, 1);
    expect(waiting!.width).toBeCloseTo(before!.width, 1);
    expect(await logo.evaluate((element: HTMLImageElement) => element.naturalWidth)).toBeGreaterThan(0);
    release();
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Comparing predictions");
    await expect(page).toHaveTitle("Comparing predictions · Tiny Stats");
    await expect(page.getByRole("main")).toBeFocused();
    await openNavigation(page, info);
    await expect(page.locator('.contextual-nav [aria-current="page"]')).toHaveText("Comparing predictions");
    await openSections(page, info);
    await sections(page, info).getByRole("link", { name: "Python", exact: true }).click();
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Tiny Stats for Python");
    await expect(sections(page, info).locator('[aria-current="true"]')).toHaveText("Python");
    if (info.project.name === "mobile") {
      await expect(page.locator(".nav-disclosure")).not.toHaveAttribute("open");
    }
    await page.getByRole("searchbox").fill("mean_squared_error");
    await page.locator("#search-results").getByRole("link", {
      name: "mean_squared_error · Tiny Stats for R", exact: true,
    }).click();
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Mean squared error");
    await expect(page).toHaveTitle("Mean squared error · Tiny Stats");
    expect(await shell.evaluate(value => value.document === document && value.elements.every(element => element.isConnected))).toBe(true);
    expect(loads).toBe(0);
    await page.getByRole("searchbox").fill("Choosing a metric");
    await expect(page.locator("#search-results").getByRole("link", { name: "Choosing a metric", exact: true })).toBeVisible();
    await page.getByRole("searchbox").fill("");
    await expect(page.locator("#search-results > li")).toHaveCount(0);
    await capture(page, info, "client-navigation-shell");
  } finally {
    release();
    await page.unrouteAll({ behavior: "ignoreErrors" });
    await shell.dispose();
  }
});

async function followFixtureLink(page: Page, href: string) {
  await page.evaluate(destination => {
    document.getElementById("navigation-fixture")?.remove();
    const link = document.createElement("a");
    link.id = "navigation-fixture";
    link.href = destination;
    link.textContent = "Navigation fixture";
    document.querySelector("main")!.prepend(link);
  }, href);
  // The fixture link must not change the scroll position being tested.
  await page.getByRole("main").getByRole("link", { name: "Navigation fixture", exact: true }).dispatchEvent("click", { button: 0 });
}

test("client navigation restores history, scroll, and same-page and cross-page fragments", async ({ page }) => {
  await page.goto("/guides/comparing-predictions.html");
  await page.evaluate(() => window.scrollTo(0, 500));
  const savedScroll = await page.evaluate(() => window.scrollY);
  expect(savedScroll).toBeGreaterThan(0);
  // Dispatching the link avoids Playwright scrolling it into view before the click.
  await page.getByRole("main").getByRole("link", { name: "Choosing a metric", exact: true }).dispatchEvent("click", { button: 0 });
  await expect(page).toHaveTitle("Choosing a metric · Tiny Stats");
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);
  await page.goBack();
  await expect(page).toHaveURL(/\/guides\/comparing-predictions\.html$/);
  await expect(page).toHaveTitle("Comparing predictions · Tiny Stats");
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBeCloseTo(savedScroll, 0);
  await page.goForward();
  await expect(page).toHaveURL(/\/guides\/choosing-a-metric\.html$/);
  await expect(page).toHaveTitle("Choosing a metric · Tiny Stats");

  await followFixtureLink(page, "/guides/comparing-predictions.html#rust");
  await expect(page).toHaveURL(/comparing-predictions\.html#rust$/);
  await expect(page.locator("#rust")).toBeInViewport();
  await expect(page.locator("#rust")).toBeFocused();
  await followFixtureLink(page, "#python");
  await expect(page.locator("#python")).toBeInViewport();
  await expect(page.locator("#python")).toBeFocused();
  await page.goBack();
  await expect(page).toHaveURL(/#rust$/);
  await expect(page.locator("#rust")).toBeInViewport();
  await page.goForward();
  await expect(page).toHaveURL(/#python$/);
  await expect(page.locator("#python")).toBeInViewport();
  await page.keyboard.press("Control+Home");
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);
  await followFixtureLink(page, "/tabsets.html#nested-output");
  await expect(page.getByText("Nested output.", { exact: true })).toBeVisible();
  await expect(page.getByText("Nested output.", { exact: true })).toBeInViewport();
  await page.goBack();
  await expect(page).toHaveURL(/comparing-predictions\.html#python$/);
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);
});

test("client navigation initializes code and tabsets on every visit without duplicate controls", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto("/");
  for (let visit = 0; visit < 2; visit++) {
    await followFixtureLink(page, "/tabsets.html");
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Tabsets");
    await expect(page.locator(".tabset-controls")).toHaveCount(3);
    const first = page.locator(".panel-tabset").first();
    await first.getByRole("tab", { name: "Python", exact: true }).click();
    await expect(first.getByText("Python content.", { exact: true })).toBeVisible();
    await first.getByRole("tab", { name: "Python", exact: true }).press("ArrowRight");
    await expect(first.getByRole("tab", { name: "R", exact: true })).toBeFocused();
    await followFixtureLink(page, "/guides/comparing-predictions.html");
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Comparing predictions");
    await expect(page.locator("main .code-block")).toHaveCount(4);
    await expect(page.locator(".code-block .code-block")).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Copy code", exact: true })).toHaveCount(4);
    const block = page.locator("main .code-block").first();
    await block.getByRole("button", { name: "Copy code", exact: true }).click();
    await expect(block.getByRole("status")).toHaveText("Copied!");
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(await block.locator("pre code").textContent());
  }
});

test("client navigation cancels stale responses when another link or Back wins", async ({ page }) => {
  await page.goto("/overview.html");
  const pending: { release: () => void; finished: Promise<void> }[] = [];
  await page.route("**/guides/comparing-predictions.html", async route => {
    const response = await route.fetch();
    let release!: () => void;
    let finish!: () => void;
    const ready = new Promise<void>(resolve => { release = resolve; });
    const finished = new Promise<void>(resolve => { finish = resolve; });
    pending.push({ release, finished });
    await ready;
    try { await route.fulfill({ response }); } catch {}
    finish();
  });
  try {
    await page.getByRole("main").getByRole("link", { name: "Comparing predictions", exact: true }).click();
    await expect.poll(() => pending.length).toBe(1);
    await page.getByRole("main").getByRole("link", { name: "Choosing a metric", exact: true }).click();
    await expect(page).toHaveTitle("Choosing a metric · Tiny Stats");
    pending[0].release();
    await pending[0].finished;
    await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
    await expect(page).toHaveTitle("Choosing a metric · Tiny Stats");

    await followFixtureLink(page, "/guides/comparing-predictions.html");
    await expect.poll(() => pending.length).toBe(2);
    await page.goBack();
    await expect(page).toHaveURL(/\/overview\.html$/);
    await expect(page).toHaveTitle("Tiny Stats · Tiny Stats");
    pending[1].release();
    await pending[1].finished;
    await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Tiny Stats");
    await expect(page).toHaveURL(/\/overview\.html$/);
  } finally {
    for (const request of pending) request.release();
    await page.unrouteAll({ behavior: "ignoreErrors" });
  }
});

test("client navigation leaves modified, external, download, and new-window links native", async ({ page }) => {
  await page.goto("/");
  const prevented = await page.evaluate(() => {
    const cases = [
      { href: "/packages/python/", control: { ctrlKey: true } },
      { href: "/packages/python/", control: { metaKey: true } },
      { href: "/packages/python/", control: { shiftKey: true } },
      { href: "/packages/python/", control: { altKey: true } },
      { href: "/packages/python/", control: { button: 1 } },
      { href: "/packages/python/", target: "_blank" },
      { href: "/packages/python/", download: "package.html" },
      { href: "https://example.com/" },
      { href: "mailto:docs@example.com" },
      { href: "/assets/site.css" },
    ];
    return cases.map(({ href, target, download, control }) => {
      const link = document.createElement("a");
      link.href = href;
      if (target) link.target = target;
      if (download) link.download = download;
      document.querySelector("main")!.append(link);
      let canceled = false;
      const stopNativeAction = (event: MouseEvent) => {
        canceled = event.defaultPrevented;
        event.preventDefault();
      };
      window.addEventListener("click", stopNativeAction, { once: true });
      link.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, button: 0, ...control }));
      link.remove();
      return canceled;
    });
  });
  expect(prevented).toEqual(Array(10).fill(false));
  await expect(page).toHaveTitle("Tiny Stats · Tiny Stats");
});

test("client navigation falls back to document navigation for incompatible and non-HTML responses", async ({ page }) => {
  for (const [path, contentType, body] of [
    ["/other.html", "text/html", "<!doctype html><html><head><title>Other application</title></head><body>Other application</body></html>"],
    ["/plain.html", "text/plain", "Plain response"],
  ]) {
    await page.goto("/");
    let documentRequests = 0;
    await page.route(`**${path}`, async route => {
      if (route.request().isNavigationRequest()) documentRequests++;
      await route.fulfill({ status: 200, contentType, body });
    });
    await followFixtureLink(page, path);
    await expect(page).toHaveURL(new RegExp(`${path}$`));
    await expect(page.locator("body")).toContainText(path === "/other.html" ? "Other application" : "Plain response");
    expect(documentRequests).toBe(1);
    await page.unroute(`**${path}`);
  }
});

test("navbar and sidebars share outer margins", async ({ page }, info) => {
  test.skip(info.project.name !== "desktop", "The sidebars stack on mobile.");
  await page.goto("/guides/comparing-predictions.html");
  for (const width of [1024, 1280, 1920]) {
    await page.setViewportSize({ width, height: 900 });
    const brand = (await page.locator("header > .brand").boundingBox())!;
    const search = (await page.locator(".header-actions").boundingBox())!;
    const sidebar = (await page.locator(".layout > nav .nav-group h2").first().boundingBox())!;
    const contentWidth = await page.evaluate(() => document.body.clientWidth);
    expect(brand.x).toBeCloseTo(contentWidth - search.x - search.width, 0);
    expect(sidebar.x).toBeCloseTo(brand.x, 0);
    if (width >= 1100) {
      const toc = (await page.locator(".page-toc").boundingBox())!;
      expect(toc.x + toc.width).toBeCloseTo(search.x + search.width, 0);
      const article = (await page.getByRole("main").boundingBox())!;
      expect(article.x + article.width / 2).toBeCloseTo(contentWidth / 2, 0);
      expect(toc.x).toBeGreaterThanOrEqual(article.x + article.width - 1);
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

test("navbar branding stays visible while reaching lower navigation links", async ({ page }, info) => {
  test.skip(info.project.name !== "desktop", "Mobile navigation stays in the page flow.");
  await page.setViewportSize({ width: 1280, height: 400 });
  await page.goto("/overview.html");
  const sidebar = page.getByRole("navigation", { name: "Documentation", exact: true });
  const brand = page.locator("header > .brand");
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

test("brand title stays in place while the logo loads", async ({ page }, info) => {
  let releaseImages!: () => void;
  const imagesReady = new Promise<void>(resolve => { releaseImages = resolve; });
  await page.route("**/assets/*", async route => {
    if (route.request().resourceType() === "image") await imagesReady;
    await route.continue();
  });
  await page.goto("/packages/python/", { waitUntil: "domcontentloaded" });
  const brand = page.locator("header > .brand");
  const logo = brand.locator(".brand-logo");
  const title = brand.locator("span");
  await expect(logo).toBeAttached();
  expect(await logo.evaluate((element: HTMLImageElement) => element.naturalWidth)).toBe(0);
  const before = (await title.boundingBox())!;
  releaseImages();
  await expect.poll(() => logo.evaluate((element: HTMLImageElement) => element.naturalWidth)).toBeGreaterThan(0);
  const after = (await title.boundingBox())!;
  expect(after.x).toBeCloseTo(before.x, 1);
  expect(after.y).toBeCloseTo(before.y, 1);
  await capture(page, info, "brand-logo-loaded");
});

test("navbar logo loads on nested pages and links home", async ({ page }, info) => {
  for (const path of ["/overview.html", "/guides/comparing-predictions.html", "/packages/python/"]) {
    await page.goto(path);
    const brand = page.locator("header > .brand");
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
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(homeHeading);
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
  expect(await main.getByRole("link", { name: "documentation", exact: true }).evaluate((link: HTMLAnchorElement) => link.pathname))
    .toBe("/overview.html");
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
    await openSections(page, info);
    const repository = page.locator("header").getByRole("link", { name: "GitHub", exact: true });
    await expect(repository).toHaveAttribute("href", "https://github.com/jolars/diplodocus");
    await expect(repository).toHaveText("");
    await expect(repository.locator("svg")).toBeVisible();
    if (info.project.name === "mobile") await page.locator(".mobile-menu > summary").click();
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
    if (info.project.name === "desktop" && path !== "/") {
      const sidebar = (await page.locator(".layout > nav").boundingBox())!;
      expect(box!.x).toBeCloseTo(sidebar.x + sidebar.width, 0);
    } else {
      expect(box!.x).toBe(0);
    }
    expect(box!.x + box!.width).toBe(await page.evaluate(() => document.body.clientWidth));
    expect(Math.abs(box!.y + box!.height - page.viewportSize()!.height)).toBeLessThan(1);
    await fitsViewport(page);
  }
  await page.evaluate(() => window.scrollTo(0, 0));
  await capture(page, info, "footer");
});

test("phone documentation controls stay pinned as the upper navbar scrolls away", async ({ page }, info) => {
  test.skip(info.project.name !== "mobile", "The documentation controls are pinned on phones.");
  await page.goto("/guides/comparing-predictions.html");
  const sidebar = page.locator(".layout > nav");
  const toc = page.locator(".page-toc");
  await expect(page.locator("body")).not.toHaveAttribute("data-controls-stuck");
  await page.evaluate(() => scrollTo(0, 250));
  await expect(page.locator("header > .brand")).not.toBeInViewport();
  await expect.poll(async () => (await sidebar.boundingBox())!.y).toBe(0);
  expect((await toc.boundingBox())!.y).toBe(0);
  await expect(page.locator("body")).toHaveAttribute("data-controls-stuck");
  await expect(sidebar).not.toHaveCSS("box-shadow", "none");
  await expect(toc).not.toHaveCSS("box-shadow", "none");
  await capture(page, info, "phone-documentation-controls-scrolled", true, false);
  await toc.locator("summary").tap();
  await toc.getByRole("link", { name: "Python", exact: true }).tap();
  await expect(page.getByRole("heading", { name: "Python", exact: true })).toBeInViewport();
  await sidebar.locator(".nav-disclosure > summary").tap();
  await expect(page.getByRole("dialog", { name: "Documentation menu" })).toBeVisible();
  await page.keyboard.press("Escape");
  await page.evaluate(() => scrollTo(0, 0));
  await expect(page.locator("body")).not.toHaveAttribute("data-controls-stuck");
  await expect(page.locator("header > .brand")).toBeInViewport();
});

test("phone documentation menu overlays the page beside the page TOC", async ({ page }, info) => {
  test.skip(info.project.name !== "mobile", "The documentation drawer is shown on phones.");
  await page.goto("/guides/comparing-predictions.html");
  const toggle = page.locator(".nav-disclosure > summary");
  const toc = page.locator(".toc-disclosure > summary");
  await expect(toggle).toHaveText("Menu");
  const menuBox = (await toggle.boundingBox())!;
  const tocBox = (await toc.boundingBox())!;
  expect(menuBox.y + menuBox.height / 2).toBeCloseTo(tocBox.y + tocBox.height / 2, 0);
  expect(tocBox.x).toBeGreaterThan(menuBox.x + menuBox.width);
  expect(tocBox.x + tocBox.width).toBeCloseTo(page.viewportSize()!.width - 16, 0);
  const main = page.getByRole("main");
  const before = await main.boundingBox();
  await capture(page, info, "phone-documentation-controls");
  await toggle.tap();
  const drawer = page.getByRole("dialog", { name: "Documentation menu" });
  await expect(drawer).toBeVisible();
  await expect(drawer).toHaveCSS("background-color", "rgb(255, 255, 255)");
  await expect.poll(async () => (await drawer.boundingBox())!.x).toBe(0);
  expect(await main.boundingBox()).toEqual(before);
  await expect(page.locator("body")).toHaveCSS("overflow", "hidden");
  await capture(page, info, "phone-documentation-drawer", true, false);
  await page.keyboard.press("Escape");
  await expect(drawer).toBeHidden();
  await expect(toggle).toBeFocused();
  await toggle.tap();
  await page.mouse.click(page.viewportSize()!.width - 8, 200);
  await expect(drawer).toBeHidden();
  await toggle.tap();
  await drawer.getByRole("group", { name: "Guide", exact: true }).getByRole("link", { name: "Overview", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Tiny Stats");
  await expect(drawer).toBeHidden();
});

test("phone menu contains sections and repository links and dismisses", async ({ page }, info) => {
  test.skip(info.project.name !== "mobile", "The hamburger menu is shown on phones.");
  await page.goto("/");
  const menu = page.locator(".mobile-menu");
  const summary = menu.locator("summary");
  await expect(summary).toHaveAccessibleName("Navigation menu");
  await expect(page.locator(".mobile-sections")).toBeHidden();
  await expect(page.locator(".header-actions > .repository-link")).toBeHidden();
  await summary.tap();
  await expect(menu.getByRole("link")).toHaveCount(6);
  await expect(menu.locator(".repository-link")).toBeVisible();
  await capture(page, info, "phone-menu-open");
  await page.keyboard.press("Escape");
  await expect(page.locator(".mobile-sections")).toBeHidden();
  await expect(summary).toBeFocused();
  await summary.tap();
  await page.getByRole("searchbox").tap();
  await expect(page.locator(".mobile-sections")).toBeHidden();
  await page.locator(".brand").focus();
  await summary.tap();
  await menu.getByRole("link", { name: "Python", exact: true }).tap();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Tiny Stats for Python");
  await expect(page.locator(".mobile-sections")).toBeHidden();
});

test("phone search expands from an icon and collapses after leaving", async ({ page }, info) => {
  test.skip(info.project.name !== "mobile", "The compact search control is shown on phones.");
  await page.goto("/");
  const search = page.getByRole("searchbox");
  const brand = page.locator("header > .brand");
  await expect(brand).toHaveAccessibleName("Tiny Stats");
  await expect(brand.locator(".brand-logo")).toBeVisible();
  const branding = (await brand.boundingBox())!;
  const control = (await search.boundingBox())!;
  expect(branding.x + branding.width).toBeLessThan(control.x);
  expect(branding.y + branding.height / 2).toBeCloseTo(control.y + control.height / 2, 0);
  await expect(search).toHaveCSS("width", "42px");
  await capture(page, info, "phone-search-collapsed");
  await search.tap();
  await expect(search).toBeFocused();
  expect((await search.boundingBox())!.width).toBeGreaterThan(200);
  await search.fill("mean_squared_error");
  await expect(page.locator("#search-results a").first()).toBeVisible();
  const results = (await page.locator("#search-results").boundingBox())!;
  expect(results.x).toBeGreaterThanOrEqual(0);
  expect(results.x + results.width).toBeLessThanOrEqual(page.viewportSize()!.width);
  await capture(page, info, "phone-search-expanded");
  await page.locator(".brand").focus();
  await expect(search).toHaveCSS("width", "42px");
  await expect(page.locator("#search-results")).toBeHidden();
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
  await closeNavigation(page, info);
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
  await expect(main.getByRole("heading", { level: 1 })).toHaveText("Authors and citation");
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
  await closeNavigation(page, info);
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
    const menu = page.locator(".mobile-menu");
    if (await menu.evaluate((element: HTMLDetailsElement) => element.open)) {
      await menu.locator("summary").click();
    }
    const disclosure = page.locator(".nav-disclosure");
    if (!await disclosure.evaluate((element: HTMLDetailsElement) => element.open)) {
      await disclosure.locator(":scope > summary").click();
    }
  }
}

async function closeNavigation(page: Page, info: TestInfo) {
  if (info.project.name !== "mobile") return;
  const drawer = page.getByRole("dialog", { name: "Documentation menu" });
  if (await drawer.isVisible()) await drawer.getByRole("button", { name: "Close menu" }).click();
}

async function openSections(page: Page, info: TestInfo) {
  if (info.project.name !== "mobile") return;
  await closeNavigation(page, info);
  const menu = page.locator(".mobile-menu");
  if (!await menu.evaluate((element: HTMLDetailsElement) => element.open)) {
    await menu.locator("summary").click();
  }
}

function sections(page: Page, info: TestInfo) {
  return page.locator(info.project.name === "mobile" ? ".mobile-sections" : ".desktop-sections");
}

test("global sections select a contextual sidebar", async ({ page }, info) => {
  await page.goto("/overview.html");
  const navigation = page.getByRole("navigation", { name: "Documentation" });
  const global = sections(page, info);
  await openSections(page, info);
  if (info.project.name === "mobile") {
    await expect(navigation.locator(".nav-disclosure > summary")).toBeVisible();
    await expect(global).toBeVisible();
  }
  await openNavigation(page, info);
  await expect(global.locator("a")).toHaveText(["Guide", "Python", "R", "Julia", "Rust"]);
  await expect(global.locator('[aria-current="true"]')).toHaveText("Guide");
  await expect(navigation.getByRole("group", { name: "Guide", exact: true }).getByRole("link", { name: "Overview" })).toBeVisible();
  await expect(navigation.getByRole("group", { name: "Guides", exact: true }).getByRole("link", { name: "Comparing predictions" })).toBeVisible();
  await expect(navigation.getByRole("group", { name: "API reference" })).toHaveCount(0);

  await openSections(page, info);
  await global.getByRole("link", { name: "Python", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Tiny Stats for Python");
  await openNavigation(page, info);
  await expect(global.locator('[aria-current="true"]')).toHaveText("Python");
  await expect(navigation.getByRole("group", { name: "Getting started" })).toHaveCount(0);
  await expect(navigation.getByRole("link", { name: "Comparing predictions" })).toHaveCount(0);
  await expect(navigation.getByRole("link", { name: "Evaluating models in Python" })).toBeVisible();
  const api = navigation.getByRole("group", { name: "API reference" });
  await expect(api.getByRole("link", { name: "mean_squared_error", exact: true })).not.toBeVisible();
  await api.getByText("tinystats", { exact: true }).click();
  await api.getByRole("link", { name: "mean_squared_error", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("tinystats.mean_squared_error");
  await openNavigation(page, info);
  await expect(global.locator('[aria-current="true"]')).toHaveText("Python");
  await expect(navigation.locator('[aria-current="page"]')).toHaveText("mean_squared_error");
  await expect(navigation.locator('[aria-current="page"]')).toBeVisible();
  const functionCode = navigation.locator('[aria-current="page"] code');
  const proseFont = await navigation.getByRole("link", { name: "Evaluating models in Python" }).evaluate(element => getComputedStyle(element).fontFamily);
  expect(await functionCode.evaluate(element => getComputedStyle(element).fontFamily)).not.toBe(proseFont);
  await capture(page, info, "navigation");

  await openSections(page, info);
  await global.getByRole("link", { name: "R", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Tiny Stats for R");
  await openNavigation(page, info);
  await expect(global.locator('[aria-current="true"]')).toHaveText("R");
  await expect(api.getByRole("link", { name: "mean_squared_error", exact: true })).toBeVisible();
  await expect(navigation.getByRole("link", { name: "Evaluating models in Python" })).toHaveCount(0);
  await expect(navigation.getByRole("group", { name: "Guides", exact: true })).toHaveCount(0);
  await openSections(page, info);
  await global.getByRole("link", { name: "Guide", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Tiny Stats");
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
  await closeNavigation(page, info);
  await page.getByRole("main").getByRole("link", { name: "pystats::tinystats.mean_absolute_error", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("tinystats.mean_absolute_error");
  await openNavigation(page, info);
  await expect(sections(page, info).locator('[aria-current="true"]')).toHaveText("Python");
  await expect(navigation.getByRole("link", { name: "Choosing a metric" })).toHaveCount(0);
  const active = navigation.locator('[aria-current="page"]');
  await expect(active).toHaveText("mean_absolute_error");
  await expect(active).toBeVisible();
  await closeNavigation(page, info);
  const toc = page.getByRole("navigation", { name: "On this page" });
  if (info.project.name === "mobile") await toc.getByText("On this page", { exact: true }).click();
  await toc.getByRole("link", { name: "Parameters", exact: true }).click();
  await expect(page).toHaveURL(/#parameters$/);
  await expect(page.getByRole("main").getByRole("heading", { name: "Parameters", exact: true })).toBeInViewport();
  await openNavigation(page, info);
  await openSections(page, info);
  await sections(page, info).getByRole("link", { name: "Guide", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Tiny Stats");
  await openNavigation(page, info);
  await expect(navigation.getByRole("group", { name: "Guides", exact: true }).getByRole("link")).toHaveText(guideLinks);
  await navigation.getByRole("group", { name: "Guides", exact: true }).getByRole("link", { name: "Choosing a metric" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Choosing a metric");
});

test("page TOC stays close to content when the window narrows", async ({ page }, info) => {
  test.skip(info.project.name !== "desktop", "This checks resizing through the desktop breakpoints.");
  const textLeft = (selector: string) => page.locator(selector).evaluate(element => {
    const range = document.createRange();
    range.selectNodeContents(element);
    return range.getBoundingClientRect().left;
  });
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
    if (width > 760) {
      const title = (await page.getByRole("heading", { level: 1 }).boundingBox())!;
      expect(await textLeft(".page-toc summary"), `TOC-to-title alignment at ${width}px`).toBeCloseTo(title.x, 1);
    }
    await fitsViewport(page);
  }
  await page.setViewportSize({ width: 761, height: 1000 });
  await capture(page, info, "narrow-desktop-navigation");
  await page.setViewportSize({ width: 1000, height: 1000 });
  await capture(page, info, "medium-page-toc");

  const withToc = (await page.getByRole("heading", { level: 1 }).boundingBox())!;
  await page.goto("/getting-started/");
  const title = await page.getByRole("heading", { level: 1 }).boundingBox();
  expect(title!.y).toBeCloseTo(withToc.y, 1);
  await expect(page.getByRole("link", { name: "Back to top", exact: true })).toBeVisible();
  expect(await textLeft(".back-to-top")).toBeCloseTo(title!.x, 1);
  await capture(page, info, "medium-without-toc");
});

test("page titles stay aligned across guides, package overviews, and APIs", async ({ page }, info) => {
  await page.goto("/overview.html");
  const apis = await page.locator("main a").evaluateAll(elements => elements
    .filter(element => /mean_squared_error/.test(element.textContent!))
    .map(element => (element as HTMLAnchorElement).pathname));
  expect(apis).toHaveLength(4);
  const destinations = [
    ["/getting-started/", "short-guide"],
    ["/guides/comparing-predictions.html", "guide"],
    ["/packages/python/", "python-package"],
    ["/packages/r/", "r-package"],
    ["/packages/julia/", "julia-package"],
    ["/packages/rust/", "rust-package"],
    ["/packages/python/guides/evaluating-models.html", "package-guide"],
    ...apis.map((path, index) => [path, `api-${index}`]),
    ["/overview.html", "overview"],
  ];
  const widths = info.project.name === "desktop" ? [1280, 1000, 761] : [760, 390, 320];
  for (const width of widths) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/overview.html");
    await expect(page.locator(".nav-disclosure")).toHaveAttribute("data-ready");
    const baseline = (await page.getByRole("heading", { level: 1 }).boundingBox())!;
    for (const [path, name] of destinations) {
      const destination = new URL(path, page.url()).href;
      await followFixtureLink(page, destination);
      await expect(page).toHaveURL(destination);
      const heading = page.getByRole("heading", { level: 1 });
      await expect(heading).toHaveCount(1);
      await expect(heading).toBeVisible();
      const title = (await heading.boundingBox())!;
      expect(title.y, `Title position at ${width}px on ${path}`).toBeCloseTo(baseline.y, 1);
      await fitsViewport(page);
      if (width === 1000 || width === 390) {
        await capture(page, info, `title-alignment-${name}`, true, false);
      }
    }
  }
});

test("compact page TOC overlays content and dismisses after use", async ({ page }, info) => {
  const widths = info.project.name === "desktop" ? [1099, 1000, 761] : [760, 390, 320];
  for (const width of widths) {
    await page.setViewportSize({ width, height: 844 });
    await page.goto("/guides/comparing-predictions.html");
    const toc = page.locator(".toc-disclosure");
    const summary = toc.locator(":scope > summary");
    const list = toc.locator(":scope > ul");
    const firstLink = list.getByRole("link", { name: "Python", exact: true });
    await expect(toc).toHaveJSProperty("open", false);
    const before = (await page.getByRole("main").boundingBox())!;
    const scroll = await page.evaluate(() => scrollY);
    await summary.click();
    await expect(list).toBeVisible();
    await expect(list.getByRole("link").first()).toHaveText("Back to top");
    const after = (await page.getByRole("main").boundingBox())!;
    expect(after.y).toBeCloseTo(before.y, 1);
    expect(await page.evaluate(() => scrollY)).toBe(scroll);
    const panel = (await list.boundingBox())!;
    expect(panel.x).toBeGreaterThanOrEqual(0);
    expect(panel.x + panel.width).toBeLessThanOrEqual(width);
    await fitsViewport(page);
    await capture(page, info, `page-toc-popup-${width}`, true, false);

    await firstLink.focus();
    await page.keyboard.press("Escape");
    await expect(toc).toHaveJSProperty("open", false);
    await expect(summary).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(list).toBeVisible();
    await page.getByRole("searchbox").click();
    await expect(toc).toHaveJSProperty("open", false);

    await summary.click();
    await firstLink.click();
    await expect(page).toHaveURL(/#python$/);
    await expect(toc).toHaveJSProperty("open", false);
    const heading = page.getByRole("heading", { name: "Python", exact: true });
    await expect(heading).toBeInViewport();
    await expect(heading).toBeFocused();
    await summary.click();
    await list.getByRole("link", { name: "Back to top", exact: true }).click();
    await expect.poll(() => page.evaluate(() => scrollY)).toBe(0);
    await expect(page).toHaveURL(/comparing-predictions\.html$/);
    await expect(toc).toHaveJSProperty("open", false);
  }
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
    await page.getByRole("link", { name: "Get started", exact: true }).click();
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Quick start");
    await page.locator(".brand").click();
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(homeHeading);
    await openSections(page, info);
    await sections(page, info).getByRole("link", { name: "Python", exact: true }).click();
    const navigation = page.getByRole("navigation", { name: "Documentation" });
    await navigation.getByText("tinystats", { exact: true }).click();
    await navigation.getByRole("link", { name: "mean_squared_error", exact: true }).click();
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("tinystats.mean_squared_error");
    await expect(navigation.locator('[aria-current="page"]')).toBeVisible();
    await expect(sections(page, info).locator('[aria-current="true"]')).toHaveText("Python");
    await fitsViewport(page);

    await page.goto("/guides/comparing-predictions.html#rust");
    await expect(page.locator("#rust")).toBeInViewport();
    await expect(page).toHaveTitle("Comparing predictions · Tiny Stats");
    await page.goBack();
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("tinystats.mean_squared_error");
    await page.goForward();
    await expect(page.locator("#rust")).toBeInViewport();
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
  const output = main.locator(".code-output pre");
  await expect(output).toHaveCount(1);
  await expect(output).toContainText("[1] 0.5");
  await expect(output).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
  await expect(output.locator("span")).toHaveCount(0);
  await expect(main.locator(".code-output .code-block, .code-output button")).toHaveCount(0);
  await fitsViewport(page);
  await capture(page, info, "r-example-output");
});

test("console output stays separate and plain with and without JavaScript", async ({ browser, baseURL }, info) => {
  for (const javaScriptEnabled of [true, false]) {
    const context = await browser.newContext({ javaScriptEnabled, baseURL, viewport: info.project.use.viewport });
    const page = await context.newPage();
    try {
      for (const [path, language] of [
        ["/console-output/index.html", "python"],
        ["/packages/r/vignettes/code-output.html", "r"],
      ]) {
        await page.goto(path);
        const output = page.locator("main .code-output");
        expect(await output.count()).toBeGreaterThan(3);
        await expect(output.locator("pre").first()).toHaveText('  <result>& "quoted"\n    second line\n', { useInnerText: false });
        await expect(output.locator("span, .syntax-code, .code-block, button")).toHaveCount(0);
        await expect(page.locator("main")).toContainText("Output from hidden source");
        for (const pre of await output.locator("pre").all()) {
          await expect(pre).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
        }
        const source = page.locator("main pre.syntax-code").first();
        await expect(source).toBeVisible();
        expect(await source.locator("span").count()).toBeGreaterThan(0);
        const wide = output.locator("pre").filter({ hasText: "-".repeat(160) });
        expect(await wide.evaluate(element => element.scrollWidth > element.clientWidth)).toBe(true);
        await expect(wide).toHaveAttribute("tabindex", "0");
        if (javaScriptEnabled) {
          await expect(page.locator("main .code-block").first()).toHaveCSS("background-color", "rgb(246, 246, 247)");
          await expect(page.getByRole("button", { name: "Copy code", exact: true }).first()).toBeVisible();
        }
        await fitsViewport(page);
        await capture(page, info, `code-output-${language}${javaScriptEnabled ? "" : "-no-js"}`, javaScriptEnabled);
      }
    } finally {
      await context.close();
    }
  }
});

test("copy buttons preserve highlighted code and support keyboard and fallback copying", async ({ page, context }, info) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto("/packages/r/reference/mean_squared_error.html");
  const block = page.getByRole("main").locator(".code-block").first();
  const source = await block.locator("pre code").textContent();
  const copy = block.getByRole("button", { name: "Copy code", exact: true });
  const toolbar = block.locator(".code-toolbar");
  await expect(copy.locator("svg")).toHaveCount(1);
  if (await page.evaluate(() => matchMedia("(hover: hover)").matches)) {
    await page.mouse.move(0, 0);
    await expect(toolbar).toHaveCSS("opacity", "0");
    await block.hover();
    await expect(toolbar).toHaveCSS("opacity", "1");
    await page.mouse.move(0, 0);
    await expect(toolbar).toHaveCSS("opacity", "0");
  }
  await copy.focus();
  await expect(toolbar).toHaveCSS("opacity", "1");
  await page.keyboard.press("Enter");
  await expect(block.getByRole("status")).toHaveText("Copied!");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(source);
  const feedbackBounds = await block.getByRole("status").boundingBox();
  const buttonBounds = await copy.boundingBox();
  expect(feedbackBounds!.x + feedbackBounds!.width).toBeLessThan(buttonBounds!.x);
  await capture(page, info, "copy-feedback");
  await expect(block.getByRole("status")).toBeEmpty({ timeout: 5000 });

  await page.evaluate(async () => {
    await navigator.clipboard.writeText("Before fallback copying");
    navigator.clipboard.writeText = async () => { throw new Error("Clipboard permission denied"); };
  });
  await copy.click();
  await expect(block.getByRole("status")).toHaveText("Copied!");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(source);
  await expect(copy).toBeFocused();
  await expect(page.locator(".copy-buffer")).toHaveCount(0);

  await page.evaluate(() => { document.execCommand = () => false; });
  await copy.click();
  await expect(block.getByRole("status")).toHaveText("Unable to copy. Select the code to copy it.");
  await expect(block.locator("pre code")).toHaveText(source!);
});


test("tabsets support keyboard navigation, groups, nesting, and fragment links", async ({ page }) => {
  await page.goto("/tabsets.html");
  const sets = page.locator(".panel-tabset");
  const first = sets.nth(0);
  const nested = sets.nth(1);
  const second = sets.nth(2);
  const firstTabs = first.locator(":scope > .tabset-controls");
  const python = firstTabs.getByRole("tab", { name: "Python", exact: true });
  const r = firstTabs.getByRole("tab", { name: "R", exact: true });
  await expect(r).toHaveAttribute("aria-selected", "true");
  await expect(first.getByText("Python content.", { exact: true })).toBeHidden();
  await python.click();
  await expect(python).toHaveAttribute("aria-selected", "true");
  await expect(second.getByRole("tab", { name: "Python", exact: true })).toHaveAttribute("aria-selected", "true");
  await nested.getByRole("tab", { name: "Output", exact: true }).click();
  await expect(nested.getByText("Nested output.", { exact: true })).toBeVisible();
  await expect(python).toHaveAttribute("aria-selected", "true");
  await python.focus();
  await page.keyboard.press("ArrowRight");
  await expect(r).toBeFocused();
  await expect(second.getByRole("tab", { name: "R", exact: true })).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("Home");
  await expect(python).toBeFocused();
  await page.keyboard.press("ArrowLeft");
  await expect(r).toBeFocused();
  await page.keyboard.press("Home");
  await page.keyboard.press("End");
  await expect(r).toBeFocused();
  await python.click();
  await page.evaluate(() => { location.hash = "r-details"; });
  await expect(first.getByRole("heading", { name: "R details", exact: true })).toBeVisible();
  await expect(r).toHaveAttribute("aria-selected", "true");
  await page.goto("/tabsets.html#nested-output");
  await expect(nested.getByText("Nested output.", { exact: true })).toBeVisible();
  await expect(page.getByText("Content without tab headings remains visible.")).toBeVisible();
  await fitsViewport(page);
});

test("Tiny Stats residuals guide presents Python and R as tabs", async ({ page }, info) => {
  await page.goto("/guides/inspecting-residuals.html");
  const tabset = page.locator(".panel-tabset");
  await expect(tabset.getByRole("tab", { name: "Python", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(tabset.locator(".tabset-panel").first().locator("code")).toContainText("from tinystats import residuals");
  await tabset.getByRole("tab", { name: "R", exact: true }).click();
  const panel = tabset.getByRole("tabpanel");
  await expect(panel).toContainText("library(tinystats)");
  await expect(panel.getByRole("button", { name: "Copy code" })).toBeVisible();
  await expect(page.locator(".page-toc").getByRole("link", { name: "Python", exact: true })).toHaveCount(0);
  await fitsViewport(page);
  await capture(page, info, "residuals-tabset");
});

test("tabsets retain all panels without JavaScript", async ({ browser, baseURL }) => {
  const context = await browser.newContext({ javaScriptEnabled: false, baseURL });
  try {
    const page = await context.newPage();
    await page.goto("/tabsets.html");
    for (const text of ["Python content.", "R content.", "Nested details.", "Nested output.", "Second Python content.", "Second R content."]) {
      await expect(page.getByText(text, { exact: true })).toBeVisible();
    }
    await expect(page.getByRole("tab")).toHaveCount(0);
  } finally {
    await context.close();
  }
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
  await page.setViewportSize({ width: 480, height: 800 });
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
