import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import test from "node:test";
import { fileURLToPath } from "node:url";

test("the browser fixture stops when the runner closes its pipe", { skip: process.platform === "win32" }, async () => {
  const scratch = await mkdtemp(join(tmpdir(), "diplodocus-site-test-"));
  const binary = join(scratch, "server");
  await writeFile(binary, `#!${process.execPath}
process.on("SIGTERM", () => process.exit(0));
setInterval(() => {}, 1000);
console.log("started");
`, { mode: 0o700 });
  const child = spawn(process.execPath, [
    fileURLToPath(new URL("site.mjs", import.meta.url)), "serve-test", "--playwright",
  ], {
    detached: true,
    stdio: ["pipe", "pipe", "inherit"],
    env: { ...process.env, TMPDIR: scratch, DIPLODOCUS_BROWSER_BINARY: binary },
  });
  const lines = createInterface({ input: child.stdout });
  try {
    const signal = AbortSignal.timeout(5000);
    const [, status] = await Promise.all([
      once(lines, "line", { signal }).then(([line]) => {
        assert.equal(line, "started");
        child.stdin.end();
      }),
      once(child, "exit", { signal }),
    ]);
    assert.deepEqual(status, [0, null]);
    assert.deepEqual((await readdir(scratch)).filter((name) => name.startsWith("diplodocus-browser-")), []);
  } finally {
    lines.close();
    if (child.pid && child.exitCode === null && child.signalCode === null) {
      const exited = once(child, "exit");
      process.kill(-child.pid, "SIGKILL");
      await exited;
    }
    await rm(scratch, { recursive: true, force: true });
  }
});
