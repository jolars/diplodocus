import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

const root = new URL("../", import.meta.url);

function temporary(t) {
  const directory = mkdtempSync(join(tmpdir(), "diplodocus-installer-test-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  return directory;
}

test("hosting assets preserve both installers and declare plain text", (t) => {
  const output = temporary(t);
  execFileSync("sh", ["scripts/docs-assets.sh", output], { cwd: root });
  for (const [route, source] of [["install", "sh"], ["install.ps1", "ps1"]]) {
    assert.deepEqual(readFileSync(join(output, route)), readFileSync(new URL(`scripts/diplodocus-installer.${source}`, root)));
    assert.ok(readFileSync(join(output, "_headers"), "utf8").includes(`/${route}\n  Content-Type: text/plain; charset=utf-8\n`));
  }
});

test("shell installer forwards arguments, propagates errors, and cleans up", (t) => {
  const directory = temporary(t);
  writeFileSync(join(directory, "curl"), `#!/bin/sh
for argument do
  case "$argument" in
    https:*) test "$argument" = 'https://github.com/jolars/diplodocus/releases/latest/download/diplodocus-installer.sh' || exit 99 ;;
  esac
  output=$argument
done
printf '%s\\n' 'printf "%s\\n" "$@"' 'exit 23' > "$output"
exit "$DOWNLOAD_STATUS"
`, { mode: 0o755 });
  for (const status of [0, 22]) {
    const result = spawnSync("sh", ["scripts/diplodocus-installer.sh", "--help", "two words"], {
      cwd: root,
      env: { ...process.env, PATH: `${directory}:${process.env.PATH}`, TMPDIR: directory, DOWNLOAD_STATUS: String(status) },
      encoding: "utf8",
    });
    assert.equal(result.status, status || 23);
    assert.equal(result.stdout, status ? "" : "--help\ntwo words\n");
    assert.deepEqual(readdirSync(directory), ["curl"]);
  }
});

const powershell = spawnSync("pwsh", ["-NoProfile", "-Command", "exit 0"]);
test("PowerShell installer forwards arguments and stops on failed downloads", { skip: powershell.error?.code === "ENOENT" }, () => {
  const result = spawnSync("pwsh", ["-NoProfile", "-Command", `
    function Invoke-RestMethod {
      param($Uri)
      if ($Uri -ne 'https://github.com/jolars/diplodocus/releases/latest/download/diplodocus-installer.ps1') { throw 'Wrong URL' }
      return 'Write-Output ($args -join "|")'
    }
    & ./scripts/diplodocus-installer.ps1 --help 'two words'
    function Invoke-RestMethod { throw 'Download failed' }
    try { & ./scripts/diplodocus-installer.ps1; exit 99 } catch { Write-Output 'Stopped' }
  `], { cwd: root, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim().replaceAll("\r", ""), "--help|two words\nStopped");
});
