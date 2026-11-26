import { spawn, execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { access, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { availableParallelism, cpus, platform, release, arch, tmpdir, totalmem } from "node:os";
import { dirname, join, resolve } from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { parseArgs, stripVTControlCharacters } from "node:util";
import { chromium } from "@playwright/test";

const root = fileURLToPath(new URL("..", import.meta.url));
const { values } = parseArgs({ options: {
  pages: { type: "string", default: "50,500" },
  samples: { type: "string", default: "5" },
  saves: { type: "string", default: "3" },
  output: { type: "string" },
} });
function integer(value, minimum, name) {
  const number = Number(value);
  if (!/^\d+$/.test(value) || !Number.isSafeInteger(number) || number < minimum) {
    throw new Error(`${name} must be an integer of at least ${minimum}`);
  }
  return number;
}
const sizes = [...new Set(values.pages.split(",").map(value => integer(value, 3, "--pages")))];
const samples = integer(values.samples, 1, "--samples");
const saves = integer(values.saves, 1, "--saves");
const timeout = integer(process.env.SITE_BENCH_TIMEOUT_MS ?? "120000", 1, "SITE_BENCH_TIMEOUT_MS");
const output = resolve(values.output ?? join(root, "artifacts/site-benchmark", `${Date.now()}-${process.pid}.json`));
const binary = resolve(process.env.DIPLODOCUS_BIN ?? join(root, "target/release/diplodocus"));
const vitepressPackage = fileURLToPath(import.meta.resolve("vitepress/package.json"));
const vitepress = join(dirname(vitepressPackage), "bin/vitepress.js");
const requireVitePress = createRequire(vitepressPackage);
const sha256 = bytes => createHash("sha256").update(bytes).digest("hex");
const env = { ...process.env, NO_COLOR: "1", FORCE_COLOR: "0" };
const interrupted = new AbortController();
for (const signal of ["SIGINT", "SIGTERM"]) {
  process.once(signal, () => interrupted.abort(new Error(`Interrupted by ${signal}`)));
}

function deadline(promise, message, duration = timeout, cancellable = true) {
  let timer;
  let abort;
  const guard = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(message)), duration);
    if (cancellable) {
      abort = () => reject(interrupted.signal.reason);
      if (interrupted.signal.aborted) abort();
      else interrupted.signal.addEventListener("abort", abort, { once: true });
    }
  });
  return Promise.race([promise, guard]).finally(() => {
    clearTimeout(timer);
    if (abort) interrupted.signal.removeEventListener("abort", abort);
  });
}

function start(tool, args, cwd) {
  interrupted.signal.throwIfAborted();
  const started = performance.now();
  const options = { cwd, env, stdio: ["ignore", "pipe", "pipe"] };
  const child = tool === "diplodocus"
    ? spawn(binary, ["--color", "never", "--no-progress", ...args], options)
    : spawn(process.execPath, [vitepress, ...args], options);
  let log = "";
  let ready;
  const url = new Promise(resolveUrl => { ready = resolveUrl; });
  for (const stream of [child.stdout, child.stderr]) {
    stream.setEncoding("utf8").on("data", chunk => {
      log += chunk;
      const address = stripVTControlCharacters(log).match(/(?:Serving |Local:\s+)(http:\/\/127\.0\.0\.1:\d+\/)/);
      if (address) ready(address[1]);
    });
  }
  const exited = new Promise(resolveExit => {
    child.once("error", error => resolveExit({ error }));
    child.once("exit", (code, signal) => resolveExit({ code, signal, elapsed: performance.now() - started }));
  });
  const closed = new Promise(resolveClose => child.once("close", resolveClose));
  return {
    started, exited, url,
    log: () => log,
    async stop() {
      if (child.exitCode === null && child.signalCode === null) child.kill("SIGTERM");
      try {
        await deadline(closed, "Process did not stop", 5_000, false);
      } catch {
        child.kill("SIGKILL");
        await deadline(closed, "Process did not close after SIGKILL", 5_000, false);
      }
    },
  };
}

async function freePort() {
  const socket = createServer();
  await new Promise((res, rej) => socket.once("error", rej).listen(0, "127.0.0.1", res));
  const { port } = socket.address();
  await new Promise((res, rej) => socket.close(error => error ? rej(error) : res()));
  return port;
}

function corpus(count, image) {
  const paths = ["index.md", ...Array.from({ length: count - 1 }, (_, i) => `guides/guide-${i + 1}.md`)];
  const files = new Map(paths.map((path, i) => {
    const next = paths[(i + 1) % count];
    const link = i === 0 ? next : next.replace(/^guides\//, "").replace(/^index\.md$/, "../index.md");
    const marker = `Benchmark content page ${i} revision 0.`;
    return [path, `# ${i === 1 ? "Benchmark calibration guide" : `Measurement guide ${i}`}

${marker}

## Compare observations

Record the observed value and its prediction before calculating the error. Measure
each result against the same reference, so another reader can reproduce the
comparison. In this example, the observed value is ${i + 2} and the prediction is ${i + 1}.

- Record both values in the original units.
- Check the sign before comparing the magnitude.

| Quantity | Value |
| --- | --- |
| Observation | ${i + 2} |
| Prediction | ${i + 1} |

\`\`\`js
const error = ${i + 2} - ${i + 1};
console.log(error ** 2);
\`\`\`

![Squared error diagram](${i ? "../" : ""}images/errors.svg)

[Next guide](${link})
`];
  }));
  const fingerprint = sha256(JSON.stringify([...files, ["images/errors.svg", sha256(image)]]));
  return { files, image, fingerprint };
}

async function prepare(workspace, tool, { files, image }) {
  const docs = join(workspace, "docs");
  await Promise.all(["guides", "images", ".vitepress"].map(dir => mkdir(join(docs, dir), { recursive: true })));
  await Promise.all([...files].map(([path, body]) => writeFile(join(docs, path), body)));
  await writeFile(join(docs, "images/errors.svg"), image);
  if (tool === "diplodocus") {
    await writeFile(join(workspace, "diplodocus.yaml"), `project:
  name: Benchmark
content:
  - path: docs
    mount: ""
    navigation: none
presentation:
  sidebar:
    - text: Calibration guide
      link: docs/guides/guide-1.md
`);
  } else {
    await symlink(join(root, "node_modules"), join(workspace, "node_modules"), "dir");
    await writeFile(join(docs, ".vitepress/config.mjs"), `export default {
  title: "Benchmark",
  themeConfig: {
    sidebar: [{ text: "Calibration guide", link: "/guides/guide-1" }],
    search: { provider: "local" },
  },
};
`);
  }
}

async function verifyBuild(tool, workspace, files) {
  const site = join(workspace, tool === "diplodocus" ? "site" : "docs/.vitepress/dist");
  for (const [index, path] of [...files.keys()].entries()) {
    const html = await readFile(join(site, path.replace(/\.md$/, ".html")), "utf8");
    const marker = new RegExp(`<p\\b[^>]*>\\s*Benchmark content page ${index} revision 0[.]\\s*</p>`);
    if (!marker.test(html)) {
      throw new Error(`Missing rendered content for ${path}`);
    }
  }
}

async function browserProbe(page) {
  let matches;
  await page.exposeBinding("benchmarkVisible", (_, text) => matches?.(text));
  await page.addInitScript(() => {
    let pending = false;
    function inspect() {
      if (pending) return;
      pending = true;
      requestAnimationFrame(() => {
        pending = false;
        const paragraph = [...document.querySelectorAll("p")].find(element =>
          element.textContent.trim().startsWith("Benchmark content page 1 revision ") &&
          element.checkVisibility({ visibilityProperty: true, opacityProperty: true }));
        if (paragraph) void window.benchmarkVisible(paragraph.textContent.trim());
      });
    }
    new MutationObserver(inspect).observe(document, { subtree: true, childList: true, characterData: true, attributes: true });
    inspect();
  });
  return revision => new Promise(res => {
    const expected = `Benchmark content page 1 revision ${revision}.`;
    matches = text => {
      if (text === expected) {
        matches = undefined;
        res(performance.now());
      }
    };
  });
}

async function preview(tool, workspace, input, browser, setProcess, warmup) {
  const port = tool === "diplodocus" ? 0 : await freePort();
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: "en-US" });
  try {
    const page = await context.newPage();
    page.on("pageerror", error => console.error(error));
    const visible = await browserProbe(page);
    const args = tool === "diplodocus"
      ? ["serve", "--host", "127.0.0.1", "--port", String(port)]
      : ["dev", "docs", "--host", "127.0.0.1", "--port", String(port), "--strictPort"];
    const server = setProcess(start(tool, args, workspace));
    const prematureExit = server.exited.then(status => {
      throw status.error ?? new Error(`Preview exited (${status.code ?? status.signal})`);
    });
    const startup = (async () => {
      const url = await server.url;
      const ready = visible(0);
      await page.goto(new URL("guides/guide-1.html", url).href, { waitUntil: "commit", timeout });
      return (await ready) - server.started;
    })();
    const startup_ms = await deadline(Promise.race([startup, prematureExit]), "Timed out waiting for preview content");
    const path = "guides/guide-1.md";
    const original = input.files.get(path);
    const save_ms = [];
    for (let revision = 1; revision <= saves + 1; revision++) {
      // An editor's next save arrives after the previous watcher event settles.
      await new Promise(resolveDelay => setTimeout(resolveDelay, 300));
      const ready = visible(revision);
      const started = performance.now();
      await writeFile(join(workspace, "docs", path), original.replace("page 1 revision 0.", `page 1 revision ${revision}.`));
      const end = await deadline(Promise.race([ready, prematureExit]), `Timed out waiting for save ${revision}`);
      if (revision > 1) save_ms.push(end - started);
    }
    if (warmup) {
      await page.getByRole("button", { name: /^Search/ }).first().click({ timeout });
      await page.locator(tool === "diplodocus" ? "#search" : "#localsearch-input")
        .fill("Benchmark calibration guide", { timeout });
      const results = tool === "diplodocus" ? "#search-results" : ".VPLocalSearchBox .results";
      await page.locator(`${results} a[href*="guides/guide-1.html"]`).first().waitFor({ timeout });
    }
    return { startup_ms, save_ms };
  } finally {
    await context.close();
  }
}

async function trial(tool, workflow, input, browser, label, warmup) {
  const workspace = await mkdtemp(join(tmpdir(), "diplodocus-site-bench-"));
  let child;
  try {
    await prepare(workspace, tool, input);
    if (workflow === "preview") {
      return await preview(tool, workspace, input, browser, running => (child = running), warmup);
    }
    child = start(tool, tool === "diplodocus" ? ["build"] : ["build", "docs"], workspace);
    const status = await deadline(child.exited, "Build timed out");
    if (status.code !== 0) throw status.error ?? new Error(`Build exited (${status.code ?? status.signal})`);
    await verifyBuild(tool, workspace, input.files);
    return { build_ms: status.elapsed };
  } catch (error) {
    await child?.stop();
    const log = `${output}.failure.log`;
    await mkdir(dirname(log), { recursive: true });
    await writeFile(log, `${label}: ${error.stack}\n${child?.log() ?? ""}\n`);
    throw new Error(`${label}: ${error.message}. Log: ${log}`, { cause: error });
  } finally {
    try {
      await child?.stop();
    } finally {
      await rm(workspace, { recursive: true, force: true });
    }
  }
}

function summarize(numbers) {
  const sorted = [...numbers].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return {
    count: sorted.length,
    median_ms: sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2,
    min_ms: sorted[0],
    max_ms: sorted.at(-1),
  };
}

await access(chromium.executablePath());
const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
const report = {
  format_version: 1,
  revision: git("rev-parse", "HEAD"),
  dirty: Boolean(git("status", "--porcelain")),
  versions: {
    diplodocus: execFileSync(binary, ["--version"], { encoding: "utf8" }).trim(),
    vitepress: requireVitePress(vitepressPackage).version,
    vite: requireVitePress("vite/package.json").version,
    node: process.version,
    playwright: createRequire(import.meta.url)("@playwright/test/package.json").version,
  },
  diplodocus_sha256: sha256(await readFile(binary)),
  host: { platform: platform(), release: release(), architecture: arch(), cpu: cpus()[0]?.model,
    available_cpus: availableParallelism(), memory_bytes: totalmem() },
  protocol: { sizes, samples, saves, timeout_ms: timeout, warmups: 1, preview_warmup_saves: 1 },
  trials: [],
  summary: [],
};
const image = await readFile(join(root, "examples/monorepo/docs/assets/errors.svg"));
const inputs = new Map(sizes.map(size => [size, corpus(size, image)]));
report.corpora = sizes.map(pages => ({ pages, sha256: inputs.get(pages).fingerprint }));

const tools = ["diplodocus", "vitepress"];
for (const workflow of ["build", "preview"]) {
  const browser = workflow === "preview"
    ? await chromium.launch({ executablePath: chromium.executablePath(), headless: true })
    : undefined;
  try {
    if (browser) report.versions.chromium = browser.version();
    for (const [sizeIndex, pages] of sizes.entries()) {
      console.log(`${workflow}: ${pages} pages`);
      for (let index = 0; index <= samples; index++) {
        const order = (index + sizeIndex) % 2 ? [...tools].reverse() : tools;
        for (const tool of order) {
          const label = `${tool} ${workflow}, ${pages} pages, ${index ? `trial ${index}` : "warm-up"}`;
          const timing = await trial(tool, workflow, inputs.get(pages), browser, label, index === 0);
          if (index) report.trials.push({ pages, tool, trial: index, ...timing });
        }
      }
    }
  } finally {
    await browser?.close();
  }
}
for (const pages of sizes) {
  for (const tool of tools) {
    const trials = report.trials.filter(row => row.pages === pages && row.tool === tool);
    for (const metric of ["build_ms", "startup_ms", "save_ms"]) {
      const stats = summarize(trials.flatMap(row => row[metric] ?? []));
      report.summary.push({ pages, tool, metric, ...stats });
      console.log(`${pages} pages | ${tool} | ${metric} | median ${stats.median_ms.toFixed(1)} ms | range ${stats.min_ms.toFixed(1)}–${stats.max_ms.toFixed(1)} ms`);
    }
  }
}
await mkdir(dirname(output), { recursive: true });
await writeFile(output, JSON.stringify(report, null, 2) + "\n");
console.log(`Report: ${output}`);
