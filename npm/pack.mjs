import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const [tag, destination] = process.argv.slice(2);
if (!/^v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$/.test(tag) || !destination) {
  throw new Error("Usage: node npm/pack.mjs vMAJOR.MINOR.PATCH OUTPUT_DIRECTORY");
}

const output = resolve(destination);
const archives = join(output, "archives");
mkdirSync(archives, { recursive: true });

const release = JSON.parse(execFileSync("gh", [
  "release", "view", tag, "--json", "isDraft,isPrerelease",
], { encoding: "utf8" }));
if (release.isDraft || release.isPrerelease) {
  throw new Error(`Expected a published stable release for ${tag}`);
}

// The static Linux builds work on glibc and musl, including systems with
// a nonstandard glibc loader location.
const targets = [
  ["linux", "x64", "x86_64-unknown-linux-musl"],
  ["linux", "arm64", "aarch64-unknown-linux-musl"],
  ["darwin", "x64", "x86_64-apple-darwin"],
  ["darwin", "arm64", "aarch64-apple-darwin"],
  ["win32", "x64", "x86_64-pc-windows-msvc"],
];
// Releases through v0.16.0 predate Windows ARM64 archives.
const [major, minor, patch] = tag.slice(1).split(".").map(Number);
if (major > 0 || minor > 16 || (minor === 16 && patch > 0)) {
  targets.push(["win32", "arm64", "aarch64-pc-windows-msvc"]);
}
const common = {
  version: tag.slice(1),
  license: "MIT OR Apache-2.0",
  homepage: "https://diplodocus.cc",
  repository: { type: "git", url: "https://github.com/jolars/diplodocus" },
  publishConfig: { access: "public", registry: "https://registry.npmjs.org/" },
};

function pack(directory, manifest) {
  const licenses = ["LICENSE-APACHE", "LICENSE-MIT"];
  const metadata = { ...manifest, files: [...manifest.files, ...licenses] };
  writeFileSync(join(directory, "package.json"), JSON.stringify(metadata, null, 2) + "\n");
  for (const license of licenses) {
    copyFileSync(license, join(directory, license));
  }
  execFileSync("npm", ["pack", directory, "--pack-destination", output], { stdio: "inherit" });
}

const optionalDependencies = {};
for (const [os, cpu, target] of targets) {
  const platform = `${os}-${cpu}`;
  const name = `@jolars/diplodocus-${platform}`;
  const binary = os === "win32" ? "diplodocus.exe" : "diplodocus";
  const asset = `diplodocus-${target}.${os === "win32" ? "zip" : "tar.xz"}`;
  const directory = join(output, platform);
  mkdirSync(directory);
  execFileSync("gh", [
    "release", "download", tag, "--pattern", asset, "--pattern", `${asset}.sha256`, "--dir", archives,
  ], { stdio: "inherit" });
  execFileSync("sha256sum", ["--check", `${asset}.sha256`], { cwd: archives, stdio: "inherit" });
  const archive = join(archives, asset);
  // Search by basename because cargo-dist wraps Unix archives in a directory.
  const contents = os === "win32"
    ? execFileSync("unzip", ["-Z1", archive], { encoding: "utf8" })
    : execFileSync("tar", ["-tJf", archive], { encoding: "utf8" });
  const members = contents.trimEnd().split("\n").filter(path => path.split("/").at(-1) === binary);
  if (members.length !== 1) {
    throw new Error(`Expected exactly one ${binary} in ${asset}`);
  }
  const executable = os === "win32"
    ? execFileSync("unzip", ["-p", archive, members[0]], { maxBuffer: 128 * 1024 * 1024 })
    : execFileSync("tar", ["-xJOf", archive, members[0]], { maxBuffer: 128 * 1024 * 1024 });
  writeFileSync(join(directory, binary), executable, { mode: 0o755 });
  pack(directory, {
    ...common,
    name,
    description: `Diplodocus CLI for ${platform}`,
    os: [os],
    cpu: [cpu],
    files: [binary],
  });
  optionalDependencies[name] = common.version;
}

const cli = join(output, "cli");
mkdirSync(cli);
copyFileSync("npm/diplodocus.cjs", join(cli, "diplodocus.cjs"));
chmodSync(join(cli, "diplodocus.cjs"), 0o755);
copyFileSync("README.md", join(cli, "README.md"));
pack(cli, {
  ...common,
  name: "diplodocus",
  description: "Build coherent documentation websites for polyglot software projects.",
  engines: { node: ">=22" },
  bin: { diplodocus: "diplodocus.cjs" },
  files: ["diplodocus.cjs"],
  optionalDependencies,
});
