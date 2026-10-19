import { spawn, spawnSync } from "node:child_process";
import { appendFileSync, cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const [mode, ...args] = process.argv.slice(2);
process.chdir(root);
if (process.env.DIPLODOCUS_BROWSER_FONTCONFIG_FILE) {
  process.env.FONTCONFIG_FILE = process.env.DIPLODOCUS_BROWSER_FONTCONFIG_FILE;
}

function build() {
  const result = spawnSync("cargo", ["build", "--locked", "--bin", "diplodocus"], {
    stdio: "inherit",
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
  const metadata = spawnSync(
    "cargo",
    ["metadata", "--locked", "--no-deps", "--format-version", "1"],
    { encoding: "utf8" },
  );
  if (metadata.error) throw metadata.error;
  if (metadata.status !== 0) throw new Error(metadata.stderr);
  return join(JSON.parse(metadata.stdout).target_directory, "debug", "diplodocus");
}

async function run(command, arguments_) {
  const child = spawn(command, arguments_, { stdio: "inherit" });
  const interrupt = () => child.kill("SIGINT");
  const terminate = () => child.kill("SIGTERM");
  process.on("SIGINT", interrupt);
  process.on("SIGTERM", terminate);
  try {
    return await new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("exit", (code, signal) => resolve(code ?? (signal ? 130 : 1)));
    });
  } finally {
    process.off("SIGINT", interrupt);
    process.off("SIGTERM", terminate);
  }
}

if (mode === "serve-test") {
  if (!process.env.DIPLODOCUS_BROWSER_BINARY) {
    throw new Error("Run site-test or site-capture to build the test binary first.");
  }
  const workspace = mkdtempSync(join(tmpdir(), "diplodocus-browser-"));
  try {
    // Separate sources also isolate snapshots and caches between concurrent runs.
    for (const path of ["diplodocus.toml", "docs", "python", "r", "julia", "rust"]) {
      cpSync(join(root, "examples/monorepo", path), join(workspace, path), {
        recursive: true,
      });
    }
    const configuration = join(workspace, "diplodocus.toml");
    cpSync(join(root, "tests/fixtures/tabsets.md"), join(workspace, "docs/tabsets.md"));
    writeFileSync(configuration, readFileSync(configuration, "utf8")
      .replace('url = "https://github.com/jolars/diplodocus"\n', 'url = "https://github.com/jolars/diplodocus"\nrevision = "main"\n'));
    mkdirSync(join(workspace, "spec"));
    writeFileSync(join(workspace, "spec/README.md"), "# Contract\n\nA repository specification.\n");
    writeFileSync(join(workspace, "TRUST.md"), "# Trust\n\nA repository trust policy.\n");
    writeFileSync(join(workspace, "OPERATIONS.md"), "# Operations\n\nRepository operations.\n");
    appendFileSync(join(workspace, "r/CONTRIBUTING.md"), "\nRead the [specification](../spec/README.md#contract), [trust policy](../TRUST.md), and [operations guide](../OPERATIONS.md) before contributing.\n\nSee the [documentation](../docs/index.md) for the shared guide.\n");
    const pythonEntry = join(workspace, "python/src/tinystats/__init__.py");
    const overloadSource = readFileSync(
      join(root, "tests/fixtures/python-overloads/python/demo/_api.py"), "utf8",
    );
    const pythonSource = readFileSync(pythonEntry, "utf8")
      .replace('__all__ = [', '__all__ = ["fetch_data", "Reader",');
    writeFileSync(join(workspace, "python/src/tinystats/_api.py"), overloadSource);
    writeFileSync(pythonEntry, `${pythonSource}\nfrom ._api import Reader, fetch_data\n`);
    cpSync(join(root, "tests/fixtures/r-vignettes"), join(workspace, "r/vignettes"), { recursive: true });
    const internalTopics = join(root, "tests/fixtures/r-internal-topics");
    for (const directory of ["R", "man"]) {
      cpSync(join(internalTopics, directory), join(workspace, "r", directory), { recursive: true });
    }
    appendFileSync(join(workspace, "r/NAMESPACE"), `\n${readFileSync(join(internalTopics, "NAMESPACE"), "utf8")}`);
    cpSync(join(root, "tests/fixtures/r-datasets/man"), join(workspace, "r/man"), { recursive: true });
    const rdContent = join(root, "tests/fixtures/r-rd-content");
    for (const directory of ["R", "man"]) {
      cpSync(join(rdContent, directory), join(workspace, "r", directory), { recursive: true });
    }
    appendFileSync(join(workspace, "r/NAMESPACE"), `\n${readFileSync(join(rdContent, "NAMESPACE"), "utf8")}`);
    const sharedTopics = join(root, "tests/fixtures/r-shared-topics");
    for (const directory of ["R", "man"]) {
      cpSync(join(sharedTopics, directory), join(workspace, "r", directory), { recursive: true });
    }
    appendFileSync(join(workspace, "r/NAMESPACE"), `\n${readFileSync(join(sharedTopics, "NAMESPACE"), "utf8")}`);
    process.exitCode = await run(process.env.DIPLODOCUS_BROWSER_BINARY, [
      "serve", "--config", join(workspace, "diplodocus.toml"),
      "--output", join(workspace, "site"),
      "--host", "127.0.0.1", "--port", process.env.DIPLODOCUS_BROWSER_PORT ?? "0",
      "--live-reload",
    ]);
  } finally {
    rmSync(workspace, { recursive: true, force: true });
  }
} else if (mode === "dev") {
  process.exitCode = await run("go-task", ["--silent", "preview", "--", ...args]);
} else if (mode === "test" || mode === "capture") {
  process.env.DIPLODOCUS_BROWSER_BINARY = build();
  const artifacts = process.env.DIPLODOCUS_BROWSER_ARTIFACTS
    ? resolve(process.env.DIPLODOCUS_BROWSER_ARTIFACTS)
    : join(root, "artifacts/browser", `${mode}-${Date.now()}-${process.pid}`);
  mkdirSync(artifacts, { recursive: true });
  process.env.DIPLODOCUS_BROWSER_ARTIFACTS = artifacts;
  if (mode === "capture") process.env.DIPLODOCUS_BROWSER_CAPTURE = "1";
  console.log(`Browser artifacts: ${artifacts}`);
  process.exitCode = await run(process.execPath, [
    "node_modules/@playwright/test/cli.js", "test", ...args,
  ]);
} else {
  throw new Error("Usage: node scripts/site.mjs dev|test|capture [arguments]");
}
