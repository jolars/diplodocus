import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";

const root = fileURLToPath(new URL("../", import.meta.url));
const result = spawnSync(process.execPath, [
  `${root}node_modules/@playwright/cli/playwright-cli.js`, ...process.argv.slice(2),
], {
  cwd: root,
  stdio: "inherit",
  env: {
    ...process.env,
    PLAYWRIGHT_MCP_EXECUTABLE_PATH: chromium.executablePath(),
    ...(process.env.DIPLODOCUS_BROWSER_FONTCONFIG_FILE
      ? { FONTCONFIG_FILE: process.env.DIPLODOCUS_BROWSER_FONTCONFIG_FILE }
      : {}),
  },
});
if (result.error) throw result.error;
process.exitCode = result.status ?? 1;
