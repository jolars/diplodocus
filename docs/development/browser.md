# Browser development

The browser workflow uses the small [four-language monorepo](../../examples/monorepo/README.md).
Its R extraction uses the R installation supplied by devenv for library
discovery. Rust extraction uses the separate pinned nightly supplied by devenv
and verifies native doctests. Python and Julia API doctests use their native
runtimes. Rust tests cover
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

Open <http://127.0.0.1:8000/>. `site-dev` runs `task preview`, which watches the
generator's Rust, embedded CSS and JavaScript, and build inputs. Changes trigger
recompilation, restart the server, and refresh open pages. Editing the example's
documentation rebuilds the site within the running server. Stop the preview with
Ctrl-C.

The preview task enters devenv so the watcher is available when launched from an
editor with an older environment.

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

The header contains the Tiny Stats logo and title, section links, search, and
repository links. On desktop, the logo and title sit above the gray sidebar.
On phones, they sit at the left of the navbar, and search expands from a
magnifying glass when tapped. Branding stays pinned while the sidebar links
scroll. A hamburger menu in the upper-right corner holds the section and
repository links on phones. Browser tests check identical header positions and
retained elements when moving between layouts, including clean URLs under a hosting
prefix.

On phones, the staggered-lines **Menu** control and the right-aligned
**On this page** control share a row below the navbar. **Menu** opens a white
drawer from the left over a dimmed page without moving the content. The drawer
keeps keyboard focus inside it and closes through its close button, Escape,
a tap on the backdrop, or navigation.

The homepage's **Get started** action opens the quick start, and **Guide** opens
`overview.html`. The remaining navbar links open the four language sections.
`site-capture` includes both the landing page and its guide overview.

Standalone sites use client-side navigation for ordinary internal page links,
including search results. The header, branding, logo, and navigation shell stay
mounted while the page content and contextual links update. Each URL also serves
complete static HTML for direct visits and navigation without JavaScript.
Modified clicks, downloads, and external links retain their browser behavior.

Navigation updates the page title and metadata, closes the mobile documentation
menu, and focuses the new content. Fragment links reveal containing tabs before
scrolling, and browser back and forward restore the saved scroll position. Copy
controls, tabsets, and the page TOC initialize after each page replacement. The
preview keeps its existing live-reload connection. Failed requests and pages
with an incompatible navigation shell fall back to a full page visit.

Browser tests retain references to the document, header, branding, and logo to
verify that internal navigation preserves their identity. They also exercise
history, anchors, repeated visits, and delayed responses. Review desktop and
mobile screenshots for logo decoding, navigation geometry, and layout stability
when changing this behavior.

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

## External frontend example

The [VitePress example](../../examples/vitepress/README.md) combines watched
package extraction with VitePress's own guide updates. Run `npm run vitepress-dev`
for interactive development or `npm run vitepress-test` for isolated browser
checks. Its temporary workspace includes both the VitePress site and the package
sources; screenshots, traces, and logs go to `artifacts/vitepress/`.

## Concurrent runs and overrides

Automated runs use separate source copies, output trees, artifacts, and ports.
The source copy is removed when its server stops. To choose locations explicitly:

| Variable | Applies to | Default |
|:---------|:-----------|:--------|
| `DIPLODOCUS_EXAMPLE_CONFIG` | Interactive preview | `examples/monorepo/diplodocus.toml` |
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
