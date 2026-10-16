import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { cp, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { chromium, expect } from "@playwright/test";

const repository = fileURLToPath(new URL("../../", import.meta.url));
const script = join(repository, "examples/vitepress/run.mjs");
const temporary = await mkdtemp(join(tmpdir(), "diplodocus-vitepress-"));
const root = join(temporary, "examples/vitepress");
const artifacts = resolve(process.env.DIPLODOCUS_VITEPRESS_ARTIFACTS ??
  join(repository, "artifacts/vitepress", `${Date.now()}-${process.pid}`));
await mkdir(artifacts, { recursive: true });
if (process.env.DIPLODOCUS_BROWSER_FONTCONFIG_FILE) {
  process.env.FONTCONFIG_FILE = process.env.DIPLODOCUS_BROWSER_FONTCONFIG_FILE;
}

async function run(command, args, env = process.env) {
  const process_ = spawn(command, args, { cwd: repository, env, stdio: "inherit" });
  await new Promise((resolve_, reject) => {
    process_.on("error", reject);
    process_.on("close", (code) => code === 0 ? resolve_() : reject(new Error(`${command} exited ${code}`)));
  });
}

async function until(condition, label) {
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    if (await condition()) return;
    await delay(100);
  }
  throw new Error(`Timed out: ${label}`);
}

async function digest(path) {
  return createHash("sha256").update(await readFile(path)).digest("hex");
}

let developer;
let browser;
let context;
let page;
let log = "";
let exited;
try {
  if (!process.env.DIPLODOCUS_BIN) await run("cargo", ["build", "--locked", "--bin", "diplodocus"]);
  const env = { ...process.env, DIPLODOCUS_BIN: process.env.DIPLODOCUS_BIN ?? join(repository, "target/debug/diplodocus") };
  for (const path of ["diplodocus.toml", "docs/index.md", "docs/guide.md", "docs/.vitepress/config.mjs"]) {
    await mkdir(join(root, path, ".."), { recursive: true });
    await cp(join(repository, "examples/vitepress", path), join(root, path));
  }
  await cp(join(repository, "examples/monorepo"), join(temporary, "examples/monorepo"), {
    recursive: true,
    filter: (path) => !path.split("/").some((part) => [".diplodocus", "__pycache__"].includes(part)),
  });
  await symlink(join(repository, "node_modules"), join(temporary, "node_modules"), "dir");
  const portServer = createServer();
  await new Promise((resolve_) => portServer.listen(0, "127.0.0.1", resolve_));
  const port = portServer.address().port;
  await new Promise((resolve_) => portServer.close(resolve_));
  developer = spawn(process.execPath, [script, "dev", "--root", root, "--port", String(port)], {
    cwd: repository, env, stdio: ["ignore", "pipe", "pipe"],
  });
  for (const stream of [developer.stdout, developer.stderr]) {
    stream.setEncoding("utf8").on("data", (chunk) => { log += chunk; });
  }
  exited = new Promise((resolve_) => developer.on("close", (code) => resolve_(code)));
  const generations = () => [...log.matchAll(/Published VitePress reference \(generation (\d+)\)/g)].length;
  await until(() => {
    if (developer.exitCode !== null) throw new Error(log);
    return generations() >= 1;
  }, "initial reference");
  const snapshot = join(root, ".diplodocus/documentation.sqlite");
  const initialSnapshot = await digest(snapshot);

  browser = await chromium.launch({ executablePath: chromium.executablePath(), headless: true });
  context = await browser.newContext({ baseURL: `http://127.0.0.1:${port}` });
  await context.tracing.start({ screenshots: true, snapshots: true, sources: true });
  page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/guide.html");
  await expect(page.locator(".vp-doc h1")).toContainText("Comparing predictions");
  const guidePath = join(root, "docs/guide.md");
  await writeFile(guidePath, `${await readFile(guidePath, "utf8")}\nGuide updates arrive without package extraction.\n`);
  await expect(page.getByText("Guide updates arrive without package extraction.")).toBeVisible();
  assert.equal(await digest(snapshot), initialSnapshot);
  assert.equal(generations(), 1);

  const apiUrl = "/reference/python/tinystats.mean_squared_error.html";
  await page.goto(apiUrl);
  await expect(page.locator(".vp-doc")).toContainText("Compute the mean squared difference");
  assert.equal(await page.locator("#parameters").count(), 1);
  await page.locator(`.vp-doc a[href="/reference/r/mean_squared_error.html"]`).click();
  await expect(page).toHaveURL(/\/reference\/r\/mean_squared_error\.html$/);
  await expect(page.locator(".vp-doc")).toContainText("mean_squared_error");
  await page.goto(apiUrl);

  const python = join(temporary, "examples/monorepo/python/src/tinystats/__init__.py");
  const original = await readFile(python, "utf8");
  const updated = original.replace(
    "Compute the mean squared difference between observations and predictions.",
    "Updated package documentation arrives automatically.",
  );
  await writeFile(python, updated);
  await until(() => generations() >= 2, "docstring rebuild");
  await expect(page.locator(".vp-doc")).toContainText("Updated package documentation arrives automatically.", { timeout: 30_000 });

  const extra = `${updated.replace('    "mean_squared_error",', '    "live_metric",\n    "mean_squared_error",')}\n\ndef live_metric():\n    \"\"\"A newly added public metric.\"\"\"\n    return 1\n`;
  await writeFile(python, extra);
  await until(() => generations() >= 3, "added API route");
  await page.goto("/reference/python/tinystats.live_metric.html");
  await expect(page.locator(".vp-doc")).toContainText("A newly added public metric.");
  await writeFile(python, updated);
  await until(() => generations() >= 4, "removed API route");
  await assert.rejects(readFile(join(root, "docs/reference/python/tinystats.live_metric.md")), { code: "ENOENT" });
  await page.goto(apiUrl);
  await expect(page.locator(".VPSidebar")).not.toContainText("live_metric");

  const julia = join(temporary, "examples/monorepo/julia/src/TinyStats.jl");
  await writeFile(julia, (await readFile(julia, "utf8")).replace(
    "Prediction metrics for numeric vectors.", "Julia documentation also updates automatically.",
  ));
  await until(() => generations() >= 5, "Julia source rebuild");
  await page.goto("/reference/julia/TinyStats.html");
  await expect(page.locator(".vp-doc")).toContainText("Julia documentation also updates automatically.");

  const complete = await digest(snapshot);
  const configPath = join(root, "diplodocus.toml");
  const config = await readFile(configPath, "utf8");
  await writeFile(configPath, "[invalid TOML");
  await until(() => log.includes("error:"), "failed extraction diagnostic");
  assert.equal(await digest(snapshot), complete);
  assert.equal(generations(), 5);
  await page.goto("/guide.html");
  await expect(page.locator(".vp-doc h1")).toContainText("Comparing predictions");
  await writeFile(guidePath, `${await readFile(guidePath, "utf8")}\nGuides keep updating during an extraction error.\n`);
  await expect(page.getByText("Guides keep updating during an extraction error.")).toBeVisible();
  await writeFile(configPath, config);
  await until(() => generations() >= 6, "extraction recovery");

  await page.goto(apiUrl);
  await page.getByRole("button", { name: /Search/ }).click();
  await page.locator(".VPLocalSearchBox input").fill("mean_squared_error");
  await expect(page.locator(".VPLocalSearchBox")).toContainText("tinystats.mean_squared_error");
  await page.keyboard.press("Escape");
  for (const [name, viewport] of [
    ["desktop", { width: 1440, height: 1000 }],
    ["mobile", { width: 390, height: 844 }],
  ]) {
    await page.setViewportSize(viewport);
    await page.goto(apiUrl);
    await expect(page.locator(".vp-doc")).toContainText("Updated package documentation arrives automatically.");
    await page.screenshot({ path: join(artifacts, `${name}-reference.png`), fullPage: true });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth), `${name} horizontal overflow`);
    await page.goto("/guide.html");
    await expect(page.locator(".vp-doc h1")).toContainText("Comparing predictions");
    await page.screenshot({ path: join(artifacts, `${name}-guide.png`), fullPage: true });
  }
  assert.deepEqual(errors, []);
  await context.tracing.stop({ path: join(artifacts, "trace.zip") });
  await browser.close();
  browser = undefined;
  developer.kill("SIGTERM");
  assert.equal(await exited, 0);
  developer = undefined;

  await rm(join(temporary, "examples/monorepo"), { recursive: true });
  await run(process.execPath, [script, "build", "--root", root, "--snapshot", snapshot], env);
  const built = await readFile(join(root, "docs/.vitepress/dist/reference/python/tinystats.mean_squared_error.html"), "utf8");
  assert.ok(built.includes("Updated package documentation arrives automatically."));
  await assert.rejects(readFile(join(root, "docs/.vitepress/dist/reference/python/tinystats.live_metric.html")), { code: "ENOENT" });
  console.log(`VitePress guide/API updates, added/removed routes, failure recovery, search, and detached build passed. Artifacts: ${artifacts}`);
} catch (error) {
  console.error(log);
  if (page && !page.isClosed()) await page.screenshot({ path: join(artifacts, "failure.png"), fullPage: true }).catch(() => {});
  if (context) await context.tracing.stop({ path: join(artifacts, "trace.zip") }).catch(() => {});
  throw error;
} finally {
  await writeFile(join(artifacts, "development.log"), log);
  if (browser) await browser.close();
  if (developer) { developer.kill("SIGTERM"); await exited; }
  await rm(temporary, { recursive: true, force: true });
}
