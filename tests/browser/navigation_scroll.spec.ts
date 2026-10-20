import { expect, test as base, type Page } from "@playwright/test";

const test = base.extend({
  baseURL: async ({}, use) => {
    const port = process.env.DIPLODOCUS_PREVIEW_PORT;
    if (!port) throw new Error("The preview server did not report its port.");
    await use(`http://127.0.0.1:${port}`);
  },
});

const guide = "/guides/comparing-predictions.html";
const fragment = "after-navigation-image";

async function delayedImage(page: Page) {
  let held = false;
  let release!: () => void;
  let requested!: () => void;
  const ready = new Promise<void>(resolve => { release = resolve; });
  const pending = new Promise<void>(resolve => { requested = resolve; });
  await page.route(`**${guide}`, async route => {
    const response = await route.fetch();
    const body = (await response.text()).replace("</h1>", `</h1>
<img id="navigation-image" src="/late-navigation-image.svg" alt="Tall chart">
<h2 id="${fragment}">After delayed image</h2>`);
    await route.fulfill({ response, body });
  });
  await page.route("**/late-navigation-image.svg", async route => {
    if (held) {
      requested();
      await ready;
    }
    await route.fulfill({
      contentType: "image/svg+xml",
      headers: { "Cache-Control": "no-store" },
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="300" height="3000"><rect width="300" height="3000" fill="#165c7c"/></svg>',
    });
  });
  return { hold: () => { held = true; }, pending, release };
}

async function follow(page: Page, href: string) {
  // Activating a link must not scroll the mobile header into view first.
  await page.evaluate(destination => {
    const link = document.createElement("a");
    link.href = destination;
    document.body.append(link);
    link.click();
    link.remove();
  }, href);
}

async function loadedImage(page: Page) {
  await expect.poll(() => page.locator("#navigation-image").evaluate(
    (image: HTMLImageElement) => image.naturalHeight,
  )).toBe(3000);
}

test("client navigation restores Back scroll after a delayed image loads", async ({ page }) => {
  const image = await delayedImage(page);
  try {
    await page.goto(guide);
    await loadedImage(page);
    await page.locator(".brand").evaluate(brand => { brand.setAttribute("data-scroll-test", "mounted"); });
    await page.evaluate(() => window.scrollTo(0, 1500));
    await expect.poll(() => page.evaluate(() => history.state.diplodocus.scroll[1])).toBe(1500);
    await follow(page, "/");
    await expect(page.locator("body")).toHaveAttribute("data-layout", "home");

    image.hold();
    await page.goBack({ waitUntil: "commit" });
    await image.pending;
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Comparing predictions");
    await expect(page.locator(".brand")).toHaveAttribute("data-scroll-test", "mounted");
    image.release();
    await loadedImage(page);
    await expect.poll(() => page.evaluate(() => Math.abs(scrollY - 1500))).toBeLessThan(3);
    await expect.poll(() => page.evaluate(() => history.state.diplodocus.scroll[1])).toBe(1500);
  } finally {
    image.release();
  }
});

test("client navigation positions a fragment after a delayed image loads", async ({ page }) => {
  const image = await delayedImage(page);
  try {
    await page.goto("/");
    image.hold();
    await follow(page, `${guide}#${fragment}`);
    await image.pending;
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Comparing predictions");
    image.release();
    await loadedImage(page);
    const target = page.locator(`#${fragment}`);
    await expect(target).toBeFocused();
    await expect.poll(() => target.evaluate(element => Math.abs(
      element.getBoundingClientRect().top - parseFloat(getComputedStyle(element).scrollMarginTop),
    ))).toBeLessThan(3);
  } finally {
    image.release();
  }
});

test("client navigation respects keyboard scrolling while an image is pending", async ({ page }) => {
  const image = await delayedImage(page);
  try {
    await page.goto("/");
    image.hold();
    await follow(page, `${guide}#${fragment}`);
    await image.pending;
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Comparing predictions");
    await page.keyboard.press("Control+Home");
    await expect.poll(() => page.evaluate(() => scrollY)).toBe(0);
    image.release();
    await loadedImage(page);
    await page.evaluate(() => new Promise<void>(resolve => {
      requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    }));
    await expect.poll(() => page.evaluate(() => scrollY)).toBe(0);
  } finally {
    image.release();
  }
});
