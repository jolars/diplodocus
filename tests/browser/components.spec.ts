import { test, expect } from '@playwright/test';
import { spawnSync } from 'node:child_process';
import { createServer, type Server } from 'node:http';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { extname, join } from 'node:path';

let workspace: string;
let origin: string;
let server: Server;
test.beforeAll(async () => {
  workspace = await mkdtemp(join(tmpdir(), 'diplodocus-components-'));
  await mkdir(join(workspace, 'pages'));
  await mkdir(join(workspace, 'widget'));
  await writeFile(join(workspace, 'diplodocus.yaml'), `project:
  name: Components
presentation:
  site-url: https://example.test/manual/
repositories:
  - id: docs
    path: .
components:
  - id: counter
    repository: docs
    path: widget
    module: index.js
    stylesheets: [style.css]
content:
  - id: guide
    owner: project
    repository: docs
    path: pages
    mount: ''
    format: gfm
`);
  await writeFile(join(workspace, 'pages/index.md'), '# Home\n\n::: {.component name="counter" #counter loading="Loading the counter" min-height="240px"}\n\nStatic fallback.\n\n:::\n\n| Authored | Value |\n| --- | --- |\n| Example | 1 |\n\n[Other page](other.md)\n');
  await writeFile(join(workspace, 'pages/props.md'), String.raw`# Props

::: {.component name="counter" #first props='{"label":"<em>&quot; \u0027 \"quoted\"</em>","values":[true,null,{"count":2}]}'}

Fallback first.

:::

::: {.component name="counter" #second props='{"label":"Second"}'}

Fallback second.

:::

::: {.component name="counter" #default}

Fallback default.

:::
`);
  await writeFile(join(workspace, 'pages/other.md'), '# Other page\n\n[Home](index.md)\n');
  await writeFile(join(workspace, 'widget/index.js'), `window.evaluations = (window.evaluations || 0) + 1;
export async function mount(root, { signal, props }) {
  root.dataset.props = JSON.stringify(props);
  props.mounted = true;
  window.mounts = (window.mounts || 0) + 1;
  const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
  signal.addEventListener('abort', () => worker.terminate(), { once: true });
  const button = document.createElement('button'); button.className = 'counter'; button.textContent = 'Waiting'; root.append(button);
  const scroll = document.createElement('div'); scroll.className = 'widget-scroll'; scroll.tabIndex = 0; scroll.setAttribute('role', 'region'); scroll.setAttribute('aria-label', 'Widget results');
  const table = document.createElement('table'); table.className = 'widget-table'; table.innerHTML = '<thead><tr><th>Region</th><th>Value</th></tr></thead><tbody><tr><td>A</td><td>1</td></tr></tbody>'; scroll.append(table); root.append(scroll);
  worker.onmessage = event => { button.textContent = event.data; };
  button.addEventListener('click', () => button.textContent = 'Clicked', { signal });
  if (window.mountGate) await window.mountGate;
  return () => { window.cleanups = (window.cleanups || 0) + 1; };
}`);
  await writeFile(join(workspace, 'widget/worker.js'), "postMessage('Ready');");
  await writeFile(join(workspace, 'widget/style.css'), `.counter { border: 3px solid rgb(12, 34, 56); }
@layer widget {
  .widget-scroll { width: 180px; max-width: 100%; overflow-x: auto; }
  .widget-table { width: 600px; border-collapse: collapse; table-layout: fixed; }
  .widget-table th:first-child { width: 120px; }
  .widget-table th, .widget-table td { padding: 2px; text-align: right; }
}`);
  const binary = process.env.DIPLODOCUS_BROWSER_BINARY!;
  const snapshot = join(workspace, 'documentation.sqlite');
  const extracted = spawnSync(binary, ['extract', '--config', join(workspace, 'diplodocus.yaml'), '--output', snapshot], { encoding: 'utf8' });
  expect(extracted.status, extracted.stderr).toBe(0);
  await rm(join(workspace, 'pages'), { recursive: true });
  await rm(join(workspace, 'widget'), { recursive: true });
  await rm(join(workspace, 'diplodocus.yaml'));
  const built = spawnSync(binary, ['generate', '--input', snapshot, '--allow-components', '--output', join(workspace, 'site')], { encoding: 'utf8' });
  expect(built.status, built.stderr).toBe(0);
  server = createServer(async (request, response) => {
    try {
      let path = new URL(request.url!, 'http://localhost').pathname.replace(/^\/manual\//, '');
      if (!path || path.endsWith('/')) path += 'index.html';
      if (!extname(path)) path += '.html';
      if (path.includes('..')) throw new Error('Invalid test path');
      const body = await readFile(join(workspace, 'site', path));
      response.setHeader('Content-Type', ({ '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json' } as Record<string,string>)[extname(path)] || 'application/octet-stream');
      response.end(body);
    } catch { response.writeHead(404).end(); }
  });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  origin = `http://127.0.0.1:${(server.address() as { port: number }).port}/manual/`;
});
test.afterAll(async () => {
  if (server) await new Promise<void>((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
  if (workspace) await rm(workspace, { recursive: true, force: true });
});

test('mounts workers and styles once per instance across navigation', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(`${origin}other.html`);
  await page.getByRole('main').getByRole('link', { name: 'Home', exact: true }).click();
  const button = page.getByRole('button', { name: 'Ready', exact: true });
  await expect(button).toBeVisible();
  await expect(button).toHaveCSS('border-top-color', 'rgb(12, 34, 56)');
  await button.click();
  await expect(page.getByRole('button', { name: 'Clicked' })).toBeVisible();
  await page.getByRole('main').getByRole('link', { name: 'Other page' }).click();
  await expect(page.getByRole('heading', { name: 'Other page', exact: true })).toBeVisible();
  await expect.poll(() => page.evaluate(() => (window as any).cleanups)).toBe(1);
  await page.goBack();
  await expect(page.getByRole('button', { name: 'Ready', exact: true })).toBeVisible();
  expect(await page.evaluate(() => [(window as any).evaluations, (window as any).mounts])).toEqual([1, 2]);
  expect(errors).toEqual([]);
});

test('retains fallback when a module fails', async ({ page }) => {
  await page.route('**/components/counter/**/index.js', route => route.fulfill({ status: 500, body: 'failed' }));
  await page.goto(origin);
  await expect(page.getByRole('status')).toContainText('could not load');
  await expect(page.getByText('Static fallback.')).toBeVisible();
  await expect(page.getByText('Loading the counter')).toBeHidden();
});

test('retains fallback without JavaScript', async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto(origin);
  await expect(page.getByText('Static fallback.')).toBeVisible();
  await expect(page.getByText('Loading the counter')).toBeHidden();
  await context.close();
});

test('component tables keep their layout and keyboard scrolling', async ({ page }, info) => {
  await page.goto(origin);
  const button = page.getByRole('button', { name: 'Ready', exact: true });
  await expect(button).toBeVisible();
  const scroll = page.getByRole('region', { name: 'Widget results' });
  const table = scroll.getByRole('table');
  await expect(table).toHaveCSS('display', 'table');
  await expect(table).toHaveCSS('border-collapse', 'collapse');
  await expect(table.getByRole('columnheader').first()).toHaveCSS('padding-left', '2px');
  await expect(table.getByRole('columnheader').first()).toHaveCSS('text-align', 'right');
  const authored = page.getByRole('table').filter({ hasText: 'Authored' });
  await expect(authored).toHaveCSS('display', 'block');
  await expect(authored).toHaveAttribute('tabindex', '0');
  await button.focus();
  await page.keyboard.press('Tab');
  await expect(scroll).toBeFocused();
  await page.keyboard.press('ArrowRight');
  await expect.poll(() => scroll.evaluate(element => element.scrollLeft)).toBeGreaterThan(0);
  await page.keyboard.press('Tab');
  await expect(authored).toBeFocused();
  await expect(authored).toHaveCSS('outline-style', 'solid');
  if (process.env.DIPLODOCUS_BROWSER_CAPTURE) {
    const directory = join(process.env.DIPLODOCUS_BROWSER_ARTIFACTS!, 'screenshots', info.project.name);
    await mkdir(directory, { recursive: true });
    await page.screenshot({ path: join(directory, 'component-tables.png'), fullPage: true });
  }
  await page.addStyleTag({ content: 'th { font-weight: 500; }' });
  await expect(authored.getByRole('columnheader').first()).toHaveCSS('font-weight', '500');
});

test('queued visibility callbacks cannot mount a departed component', async ({ page }) => {
  await page.addInitScript(() => {
    const NativeObserver = window.IntersectionObserver;
    (window as any).pendingComponents = [];
    window.IntersectionObserver = class extends NativeObserver {
      constructor(callback: IntersectionObserverCallback, options?: IntersectionObserverInit) {
        super((entries, observer) => {
          if (entries.some(entry => entry.target.hasAttribute('data-component-module'))) {
            (window as any).pendingComponents.push(() => callback(entries, observer));
          } else callback(entries, observer);
        }, options);
      }
    };
  });
  await page.goto(origin);
  await expect.poll(() => page.evaluate(() => (window as any).pendingComponents.length)).toBeGreaterThan(0);
  await page.getByRole('main').getByRole('link', { name: 'Other page' }).click();
  await expect(page.getByRole('heading', { name: 'Other page', exact: true })).toBeVisible();
  expect(await page.evaluate(() => (window as any).pendingComponents.length)).toBeGreaterThan(0);
  await page.evaluate(() => (window as any).pendingComponents.splice(0).forEach((callback: () => void) => callback()));
  await page.waitForLoadState('networkidle');
  expect(await page.evaluate(() => (window as any).mounts || 0)).toBe(0);
});

test('passes isolated props to instances from a portable snapshot', async ({ page }) => {
  await page.goto(`${origin}props.html`);
  for (const [id, props] of [
    ['first', { label: `<em>&quot; ' "quoted"</em>`, values: [true, null, { count: 2 }] }],
    ['second', { label: 'Second' }],
    ['default', {}],
  ] as const) {
    const widget = page.locator(`#${id}`);
    await widget.scrollIntoViewIfNeeded();
    await expect(widget.getByRole('button')).toHaveText('Ready');
    await expect(widget.locator('[data-props]')).toHaveAttribute('data-props', JSON.stringify(props));
  }
});

test('reserves space before the loader arrives and through async mount', async ({ page }, info) => {
  let releaseLoader!: () => void;
  const loader = new Promise<void>(resolve => { releaseLoader = resolve; });
  await page.route('**/assets/components.js', async route => {
    await loader;
    await route.continue();
  });
  await page.addInitScript(() => {
    (window as any).mountGate = new Promise<void>(resolve => { (window as any).releaseMount = resolve; });
  });
  try {
    await page.goto(origin, { waitUntil: 'commit' });
    const loading = page.getByRole('status').filter({ hasText: 'Loading the counter' });
    await expect(loading).toBeVisible();
    await expect(loading).toHaveCSS('min-block-size', '240px');
    await expect(page.getByText('Static fallback.')).toBeHidden();
    await page.addStyleTag({ content: `
      #counter { --diplodocus-component-min-height: 320px; }
      @media (max-width: 600px) { #counter { --diplodocus-component-min-height: 300px; } }
    ` });
    const height = page.viewportSize()!.width <= 600 ? 300 : 320;
    await expect(loading).toHaveCSS('min-block-size', `${height}px`);
    await expect.poll(async () => (await loading.boundingBox())?.height).toBe(height);
    releaseLoader();
    await expect(page.locator('#counter [data-component-content] button')).toHaveCount(1);
    await expect(loading).toBeVisible();
    if (process.env.DIPLODOCUS_BROWSER_CAPTURE) {
      const directory = join(process.env.DIPLODOCUS_BROWSER_ARTIFACTS!, 'screenshots', info.project.name);
      await mkdir(directory, { recursive: true });
      await page.screenshot({ path: join(directory, 'component-loading.png'), fullPage: true });
    }
    await page.evaluate(() => (window as any).releaseMount());
    await expect(loading).toBeHidden();
    await expect(page.getByRole('button', { name: 'Ready', exact: true })).toBeVisible();
    const content = page.locator('#counter [data-component-content]');
    await expect(content).toHaveCSS('min-block-size', `${height}px`);
    await expect.poll(async () => (await content.boundingBox())?.height).toBe(height);
  } finally {
    releaseLoader();
  }
});

test('restores fallback when the component loader fails', async ({ page }) => {
  await page.route('**/assets/components.js', route => route.abort());
  await page.goto(origin);
  await expect(page.getByText('Static fallback.')).toBeVisible();
  await expect(page.getByText('Loading the counter')).toBeHidden();
});

test('cleans up a mount that finishes after navigation', async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).mountGate = new Promise<void>(resolve => { (window as any).releaseMount = resolve; });
  });
  await page.goto(origin);
  await expect(page.locator('#counter [data-component-content] button')).toHaveCount(1);
  await page.getByRole('main').getByRole('link', { name: 'Other page' }).click();
  await expect(page.getByRole('heading', { name: 'Other page', exact: true })).toBeVisible();
  await page.evaluate(() => (window as any).releaseMount());
  await expect.poll(() => page.evaluate(() => (window as any).cleanups)).toBe(1);
  await expect(page.getByText('Loading the counter')).toHaveCount(0);
});
