import { defineConfig } from "@playwright/test";
import { join } from "node:path";

const artifacts = process.env.DIPLODOCUS_BROWSER_ARTIFACTS;
if (!artifacts) throw new Error("Run npm run site-test or npm run site-capture.");

export default defineConfig({
  testDir: "./tests/browser",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 0,
  workers: 1,
  outputDir: join(artifacts, "results"),
  reporter: [["list"], ["html", { outputFolder: join(artifacts, "report"), open: "never" }]],
  use: {
    browserName: "chromium",
    channel: "chromium",
    locale: "en-US",
    timezoneId: "UTC",
    colorScheme: "light",
    reducedMotion: "reduce",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [
    { name: "desktop", use: { viewport: { width: 1280, height: 900 } } },
    { name: "mobile", use: { viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true } },
  ],
  webServer: {
    command: "node scripts/site.mjs serve-test",
    wait: { stderr: /Serving http:\/\/127\.0\.0\.1:(?<DIPLODOCUS_PREVIEW_PORT>\d+)/ },
    reuseExistingServer: false,
    timeout: 30_000,
    gracefulShutdown: { signal: "SIGTERM", timeout: 10_000 },
  },
});
