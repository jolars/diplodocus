import { spawn } from "node:child_process";
import {
  access, cp, mkdir, mkdtemp, readFile, readdir, rename, rm, writeFile,
} from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createInterface } from "node:readline";
import { setTimeout as delay } from "node:timers/promises";

const example = fileURLToPath(new URL("./", import.meta.url));
const repository = resolve(example, "../..");
const vitepress = fileURLToPath(new URL("bin/vitepress.js", import.meta.resolve("vitepress/package.json")));
const marker = ".diplodocus-owned";
const owner = "Diplodocus VitePress example\n";
const packageMounts = { pystats: "python", rstats: "r", juliastats: "julia", ruststats: "rust" };
const children = new Set();
const completions = new WeakMap();
const intentionalStops = new WeakSet();
let stopping = false;

function child(command, args, stdout = "inherit") {
  const process_ = spawn(command, args, {
    cwd: repository,
    stdio: ["ignore", stdout, "inherit"],
  });
  children.add(process_);
  process_.once("close", () => children.delete(process_));
  const completed = new Promise((resolve_, reject) => {
    process_.once("error", reject);
    process_.once("close", (code, signal) => {
      if (code === 0 || stopping || intentionalStops.has(process_)) resolve_();
      else reject(new Error(`Command failed (${signal ?? code}): ${process_.spawnargs.join(" ")}`));
    });
  });
  completed.catch(() => {});
  completions.set(process_, completed);
  return process_;
}

function completion(process_) {
  return completions.get(process_);
}

async function stop(process_) {
  if (!process_) return;
  intentionalStops.add(process_);
  if (process_.exitCode === null && process_.signalCode === null) process_.kill("SIGTERM");
  await completion(process_);
}

async function run(command, args) {
  await completion(child(command, args));
}

async function binary() {
  if (process.env.DIPLODOCUS_BIN) return resolve(process.env.DIPLODOCUS_BIN);
  await run("cargo", ["build", "--locked", "--bin", "diplodocus"]);
  const process_ = child("cargo", ["metadata", "--locked", "--no-deps", "--format-version", "1"], "pipe");
  let metadata = "";
  process_.stdout.setEncoding("utf8").on("data", (data) => { metadata += data; });
  await completion(process_);
  return join(JSON.parse(metadata).target_directory, "debug", "diplodocus");
}

async function exists(path) {
  try { await access(path); return true; }
  catch (error) { if (error.code === "ENOENT") return false; throw error; }
}

async function files(root) {
  const result = [];
  for (const entry of await readdir(root, { withFileTypes: true })) {
    const path = join(root, entry.name);
    if (entry.isDirectory()) result.push(...await files(path));
    else result.push(path);
  }
  return result.sort();
}

function pagePath(document) {
  const name = document.public_name ?? document.qualified_name;
  if (name && document.package) {
    const component = encodeURIComponent(name.replaceAll("::", "."));
    return `${packageMounts[document.package] ?? document.package}/${component}.md`;
  }
  return document.path.replace(/\.html$/, ".md");
}

function publicUrl(path) {
  return `/reference/${path.replace(/\.md$/, ".html").replace(/index\.html$/, "")}`;
}

function markdown(source) {
  // Native heading anchors let VitePress use the retained IDs in its outline.
  const lines = source.split("\n");
  const identifiers = new Set([...source.matchAll(/^<a id="([^"]+)"><\/a>$/gm)].map((match) => match[1]));
  let fence;
  for (let i = 0; i < lines.length; i++) {
    const delimiter = lines[i].match(/^\s*(`{3,}|~{3,})/);
    if (delimiter) {
      if (!fence) fence = delimiter[1];
      else if (delimiter[1][0] === fence[0] && delimiter[1].length >= fence.length) fence = undefined;
      continue;
    }
    if (fence) continue;
    const anchor = lines[i].match(/^<a id="([^"]+)"><\/a>$/);
    if (anchor) {
      let next = i + 1;
      while (lines[next] === "") next++;
      if (/^#{1,6} /.test(lines[next] ?? "")) {
        lines[i] = "";
        lines[next] += ` {#${anchor[1]}}`;
      }
    } else if (/^#{1,6} /.test(lines[i]) && !/\{#[^}]+\}$/.test(lines[i])) {
      // Host-generated titles must not take an ID retained elsewhere in the body.
      const base = "vitepress-" + lines[i].replace(/^#+ /, "").toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "-");
      let id = base;
      for (let suffix = 2; identifiers.has(id); suffix++) id = `${base}-${suffix}`;
      identifiers.add(id);
      lines[i] += ` {#${id}}`;
    }
  }
  return lines.join("\n");
}

async function prepare(root, executable, snapshot) {
  const storage = join(root, ".diplodocus/vitepress");
  await mkdir(storage, { recursive: true });
  const staging = await mkdtemp(join(storage, "generation-"));
  try {
    // The watcher may replace its snapshot while URL discovery and export run.
    const input = join(staging, "documentation.sqlite");
    await cp(snapshot, input);
    const fragments = join(staging, "fragments");
    const args = ["generate", "--format", "fragments", "--input", input, "--output", fragments];
    await run(executable, args);
    const manifest = JSON.parse(await readFile(join(fragments, "documents.json"), "utf8"));
    if (manifest.format_version !== 2) throw new Error("Expected fragment format version 2.");
    const paths = new Map();
    const urls = new Set();
    const map = { format_version: 1, documents: {}, assets: {} };
    for (const document of manifest.documents) {
      const path = pagePath(document);
      const url = publicUrl(path);
      if (urls.has(url)) throw new Error(`Duplicate VitePress route: ${url}`);
      urls.add(url);
      paths.set(document.path, path);
      map.documents[document.path] = url;
    }
    for (const asset of await files(join(fragments, "assets"))) {
      const suffix = asset.slice(join(fragments, "assets").length + 1).replaceAll("\\", "/");
      map.assets[`assets/${suffix}`] = `/_diplodocus/${suffix}`;
    }
    const mapPath = join(staging, "urls.json");
    await writeFile(mapPath, JSON.stringify(map));
    await run(executable, [...args, "--url-map", mapPath]);
    const reference = join(staging, "reference");
    await mkdir(reference);
    const groups = new Map();
    for (const document of manifest.documents) {
      const path = paths.get(document.path);
      const content = await readFile(join(fragments, document.markdown_path), "utf8");
      const destination = join(reference, path);
      await mkdir(dirname(destination), { recursive: true });
      const frontmatter = `---\ntitle: ${JSON.stringify(document.title)}\nsearch: ${document.visible}\n---\n\n`;
      await writeFile(destination, frontmatter + markdown(content));
      if (document.visible && document.qualified_name) {
        const group = packageMounts[document.package] ?? document.package ?? "Project";
        if (!groups.has(group)) groups.set(group, []);
        groups.get(group).push({ text: document.public_name ?? document.title, link: map.documents[document.path] });
      }
    }
    const sidebar = [...groups].map(([text, items]) => ({ text, items }));
    await writeFile(join(reference, "sidebar.json"), JSON.stringify(sidebar));
    const assets = join(staging, "assets");
    await cp(join(fragments, "assets"), assets, { recursive: true });
    await Promise.all([reference, assets].map((path) => writeFile(join(path, marker), owner)));
    return { staging, directories: [
      [reference, join(root, "docs/reference")],
      [assets, join(root, "docs/public/_diplodocus")],
    ] };
  } catch (error) {
    await rm(staging, { recursive: true, force: true });
    throw error;
  }
}

async function publish(generation) {
  for (const [, destination] of generation.directories) {
    if (await exists(destination) &&
        (!(await exists(join(destination, marker))) || await readFile(join(destination, marker), "utf8") !== owner)) {
      throw new Error(`Refusing to replace author-written directory: ${destination}`);
    }
  }
  const moved = [];
  try {
    for (const [source, destination] of generation.directories) {
      await mkdir(dirname(destination), { recursive: true });
      const backup = `${source}.previous`;
      const previous = await exists(destination);
      if (previous) await rename(destination, backup);
      moved.push({ destination, backup, previous });
      await rename(source, destination);
    }
  } catch (error) {
    for (const { destination, backup, previous } of moved.reverse()) {
      await rm(destination, { recursive: true, force: true });
      if (previous) await rename(backup, destination);
    }
    throw error;
  }
}

async function develop(root, executable, port) {
  let server;
  let serverFailure;
  let published = 0;
  async function start() {
    // Separate processes let the supervisor await kernel cleanup before exiting.
    server = child(process.execPath, [vitepress, "dev", join(root, "docs"), "--host", "127.0.0.1", "--port", String(port), "--strictPort"]);
    const current = server;
    const deadline = Date.now() + 30_000;
    await Promise.race([
      (async () => {
        while (!stopping && current.exitCode === null && current.signalCode === null && Date.now() < deadline) {
          try {
            const response = await fetch(`http://127.0.0.1:${port}/`, { signal: AbortSignal.timeout(500) });
            await response.arrayBuffer();
            if (response.ok) return;
          } catch { /* The server has not started listening yet. */ }
          await delay(50);
        }
        throw new Error("VitePress did not become ready.");
      })(),
      completion(current).then(() => { throw new Error("VitePress exited before becoming ready."); }),
    ]);
    const failed = (error) => {
      if (!stopping && !intentionalStops.has(current)) {
        serverFailure = error;
        extractor.kill("SIGTERM");
      }
    };
    completion(current).then(() => failed(new Error("VitePress exited unexpectedly.")), failed);
  }
  const extractor = child(executable, ["extract", "--watch", "--config", join(root, "diplodocus.yaml")], "pipe");
  const exited = completion(extractor);
  // Attach immediately so an initial extraction failure cannot reject unhandled.
  exited.catch(() => {});
  try {
    for await (const line of createInterface({ input: extractor.stdout })) {
      if (stopping) break;
      const event = JSON.parse(line);
      if (event.format_version !== 1) throw new Error("Expected extraction event format version 1.");
      if (event.event !== "snapshot") continue;
      let generation;
      try {
        generation = await prepare(root, executable, event.path);
        if (stopping) break;
        // Restart after publication so route discovery and search see one generation.
        await stop(server);
        await publish(generation);
        await start();
        console.log(`Published VitePress reference (generation ${++published}).`);
      } catch (error) {
        if (!published || stopping) throw error;
        console.error(error);
        if (!server || server.exitCode !== null || server.signalCode !== null) await start();
      } finally {
        if (generation) await rm(generation.staging, { recursive: true, force: true });
      }
    }
    await exited;
    if (serverFailure) throw serverFailure;
    if (!stopping) throw new Error("Extraction watcher exited unexpectedly.");
  } finally {
    await Promise.allSettled([stop(extractor), stop(server)]);
  }
}

async function main() {
  const [mode, ...args] = process.argv.slice(2);
  const options = {};
  for (let i = 0; i < args.length; i += 2) {
    if (!["--root", "--port", "--snapshot"].includes(args[i]) || !args[i + 1]) {
      throw new Error(`Unknown or incomplete option: ${args[i]}`);
    }
    options[args[i].slice(2)] = args[i + 1];
  }
  if (!["dev", "build"].includes(mode) || (mode === "dev" && options.snapshot)) {
    throw new Error("Usage: run.mjs dev|build [--root PATH] [--port PORT] [--snapshot PATH (build only)]");
  }
  const root = resolve(options.root ?? example);
  const executable = await binary();
  if (mode === "dev") {
    const port = Number(options.port ?? "5173");
    if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error("Invalid port.");
    await develop(root, executable, port);
  } else {
    const snapshot = options.snapshot ? resolve(options.snapshot) : join(root, ".diplodocus/documentation.sqlite");
    if (!options.snapshot) await run(executable, ["extract", "--config", join(root, "diplodocus.yaml"), "--output", snapshot]);
    const generation = await prepare(root, executable, snapshot);
    try {
      await publish(generation);
      await run(process.execPath, [vitepress, "build", join(root, "docs")]);
    } finally {
      await rm(generation.staging, { recursive: true, force: true });
    }
  }
}

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.once(signal, () => {
    stopping = true;
    for (const process_ of children) process_.kill(signal);
  });
}
try {
  await main();
} catch (error) {
  if (!stopping) { console.error(error); process.exitCode = 1; }
}
