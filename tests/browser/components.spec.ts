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
  await writeFile(join(workspace, 'pages/index.md'), '# Home\n\n::: {.component name="counter"}\n\nStatic fallback.\n\n:::\n\n[Other page](other.md)\n');
  await writeFile(join(workspace, 'pages/other.md'), '# Other page\n\n[Home](index.md)\n');
  await writeFile(join(workspace, 'widget/index.js'), `window.evaluations = (window.evaluations || 0) + 1;
export async function mount(root, { signal }) {
  window.mounts = (window.mounts || 0) + 1;
  const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
  signal.addEventListener('abort', () => worker.terminate(), { once: true });
  const button = document.createElement('button'); button.className = 'counter'; button.textContent = 'Waiting'; root.append(button);
  worker.onmessage = event => { button.textContent = event.data; };
  button.addEventListener('click', () => button.textContent = 'Clicked', { signal });
  return () => { window.cleanups = (window.cleanups || 0) + 1; };
}`);
  await writeFile(join(workspace, 'widget/worker.js'), "postMessage('Ready');");
  await writeFile(join(workspace, 'widget/style.css'), '.counter { border: 3px solid rgb(12, 34, 56); }');
  const binary = process.env.DIPLODOCUS_BROWSER_BINARY!;
  const built = spawnSync(binary, ['build', '--config', join(workspace, 'diplodocus.yaml'), '--output', join(workspace, 'site')], { encoding: 'utf8' });
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
});

test('retains fallback without JavaScript', async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto(origin);
  await expect(page.getByText('Static fallback.')).toBeVisible();
  await context.close();
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
