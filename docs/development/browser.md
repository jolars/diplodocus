# Browser development

The browser workflow uses the small [R/Python monorepo](../../examples/monorepo/README.md).
Its static extraction needs no running Python or R kernels. Rust tests cover
the extraction and generation contracts; Playwright tests exercise the pages
through Chromium.

## Setup

From the repository root:

```console
devenv shell
npm ci --ignore-scripts
```

Devenv supplies Node, the matching Playwright Chromium build, and a fixed font
configuration. Browser downloads are disabled. The npm lockfile contains only
development tools; the generated site has no Node runtime dependency.

## Preview, test, and capture

```console
site-dev
```

Open <http://127.0.0.1:8000/>. Editing the example's documentation triggers a
rebuild; refresh the browser afterward. Restart this command after changing
Rust so the preview uses the new renderer. Stop the server with Ctrl-C.

```console
site-test
site-test --project desktop --grep search
site-capture
```

`site-test` builds the current binary, copies the example into a temporary
workspace, and starts its own server on a free port. Playwright waits for the
server's ready message, then runs desktop and mobile projects. Tests cover
titles, images, layout width, search, equivalent APIs, and keyboard navigation.
JavaScript and console errors fail tests, except the browser's automatic request
for an undeclared `/favicon.ico`.

`site-capture` runs the same tests and saves full-page screenshots of the home,
guide, package, and API pages. Inspect those images when reviewing presentation
changes. These are review artifacts, not automatically accepted visual baselines.

Each run prints a unique directory under `artifacts/browser/`, containing:

- `report/`: the HTML test report and its attachments;
- `results/`: failure screenshots, traces, and browser error logs;
- `screenshots/desktop/` and `screenshots/mobile/`: images from `site-capture`.

Open a report or trace with the locally installed Playwright runner:

```console
npx --no-install playwright show-report artifacts/browser/<run>/report
npx --no-install playwright show-trace artifacts/browser/<run>/results/<test>/trace.zip
```

CI runs `site-capture` and uploads these artifacts even when a test fails.
Tracing retains failed runs without retrying them. Reference images can be added
later with Playwright's screenshot assertions after reviewing the intended
appearance in this environment.

## Interactive agent inspection

The official [Playwright CLI skill](../../.agents/skills/playwright-cli/SKILL.md)
is installed in the repository and is available to agents on their next turn.
With `site-dev` running in another terminal:

```console
playwright-cli -s=review open http://127.0.0.1:8000/
playwright-cli -s=review snapshot
playwright-cli -s=review resize 390 844
playwright-cli -s=review screenshot
playwright-cli -s=review console
playwright-cli -s=review close
```

Use references from the snapshot to click or fill controls. For example,
`playwright-cli -s=review fill <ref> mean_squared_error` exercises search.
The repository wrapper selects the same Chromium and fonts as the tests.
Interactive artifacts go to the ignored `.playwright-cli/` directory. Named
sessions keep independent investigations separate; close only your own session.

The equivalent npm commands are `npm run site-dev`, `npm run site-test -- <args>`,
`npm run site-capture`, and `npm run browser -- <args>`. They still need the
browser environment supplied by devenv.

## Concurrent runs and overrides

Automated runs use separate source copies, output trees, artifacts, and ports.
The source copy is removed when its server stops. To choose locations explicitly:

| Variable | Applies to | Default |
|:---------|:-----------|:--------|
| `DIPLODOCUS_SITE_PORT` | Interactive preview | `8000` |
| `DIPLODOCUS_SITE_OUTPUT` | Interactive preview | `site/monorepo` |
| `DIPLODOCUS_BROWSER_PORT` | Automated preview | `0`, assigned by the OS |
| `DIPLODOCUS_BROWSER_ARTIFACTS` | Test and capture output | A unique run directory |

For two interactive previews, choose different ports and site directories:

```console
DIPLODOCUS_SITE_PORT=8001 DIPLODOCUS_SITE_OUTPUT=site/review site-dev
```

## Updating the tools

The test runner and Chromium revision must agree. `devenv.nix` checks that
`@playwright/test` matches the version in the pinned Nix packages. Update the
Nix input, npm version, and lockfile together, then run `site-capture` and inspect
the output.

The thin `@playwright/cli` launcher is pinned separately. Its npm overrides use
the same stable `playwright` and `playwright-core` as the test runner instead of
the launcher's prerelease dependencies. When updating it, verify `open`,
`snapshot`, and `close`, and refresh the vendored skill from the matching
upstream commit. See [skill provenance](../../.agents/skills/UPSTREAM.md).
