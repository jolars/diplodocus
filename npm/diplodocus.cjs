#!/usr/bin/env node

const { spawnSync } = require("node:child_process");

const platform = `${process.platform}-${process.arch}`;
const executable = process.platform === "win32" ? "diplodocus.exe" : "diplodocus";
const dependency = `@jolars/diplodocus-${platform}`;
const { optionalDependencies } = require("./package.json");

if (!Object.hasOwn(optionalDependencies, dependency)) {
  console.error(`Diplodocus has no npm binary for ${platform}. See https://diplodocus.cc/installation.`);
  process.exit(1);
}

let binary;
try {
  binary = require.resolve(`${dependency}/${executable}`);
} catch {
  console.error(`Missing ${dependency}. Reinstall Diplodocus with optional dependencies enabled.`);
  process.exit(1);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`Could not run Diplodocus: ${result.error.message}`);
  process.exit(1);
}
if (result.signal) {
  process.kill(process.pid, result.signal);
}
process.exit(result.status ?? 1);
