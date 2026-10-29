import assert from 'node:assert/strict';
import test from 'node:test';
import worker from '../src/rendering/markdown_worker.js';

function assets({ missing = false } = {}) {
  return { async fetch(request) {
    const path = new URL(request.url).pathname;
    if (path.endsWith('.md')) {
      assert.equal(request.headers.get('If-None-Match'), null);
      return new Response(request.method === 'HEAD' ? null : '# Guide\n', {
        status: missing ? 404 : 200,
        headers: { 'Content-Type': 'text/plain', ETag: 'markdown', 'Cache-Control': 'public, max-age=60' },
      });
    }
    if (path.endsWith('.css')) return new Response('body{}', { headers: { 'Content-Type': 'text/css' } });
    return new Response(request.method === 'HEAD' ? null : '<html>Guide</html>', {
      headers: { 'Content-Type': 'text/html', Vary: 'Accept-Encoding', ETag: 'html' },
    });
  } };
}

for (const path of ['/', '/guide/', '/guide/index.html', '/guide/page']) {
  test(`negotiates Markdown at ${path}`, async () => {
    const response = await worker.fetch(new Request(`https://example.test${path}`, {
      headers: { Accept: 'text/markdown', 'If-None-Match': 'html' },
    }), { ASSETS: assets() });
    assert.equal(response.headers.get('Content-Type'), 'text/markdown; charset=utf-8');
    assert.equal(response.headers.get('Vary'), 'Accept-Encoding, Accept');
    assert.equal(response.headers.get('ETag'), 'markdown');
    assert.equal(await response.text(), '# Guide\n');
  });
}
for (const accept of ['', '*/*', 'text/html', 'text/markdown;q=0', 'text/markdown;q=0.1,text/html;q=0.9']) {
  test(`HTML default for ${accept}`, async () => {
    const response = await worker.fetch(new Request('https://example.test/', { headers: { Accept: accept } }), { ASSETS: assets() });
    assert.equal(response.headers.get('Content-Type'), 'text/html');
    assert.equal(response.headers.get('Vary'), 'Accept-Encoding, Accept');
  });
}
test('HEAD returns Markdown headers without a body', async () => {
  const response = await worker.fetch(new Request('https://example.test/', { method: 'HEAD', headers: { Accept: 'text/markdown' } }), { ASSETS: assets() });
  assert.equal(response.headers.get('Content-Type'), 'text/markdown; charset=utf-8');
  assert.equal(await response.text(), '');
});
test('missing Markdown falls back to HTML', async () => {
  const response = await worker.fetch(new Request('https://example.test/', { headers: { Accept: 'text/markdown' } }), { ASSETS: assets({ missing: true }) });
  assert.equal(response.headers.get('Content-Type'), 'text/html');
});
test('assets retain their media type', async () => {
  const response = await worker.fetch(new Request('https://example.test/style.css', { headers: { Accept: 'text/markdown' } }), { ASSETS: assets() });
  assert.equal(response.headers.get('Content-Type'), 'text/css');
});

test('Workers static assets negotiate real files and canonical redirects', async (t) => {
  const { mkdtemp, mkdir, writeFile, rm } = await import('node:fs/promises');
  const { tmpdir } = await import('node:os');
  const { join } = await import('node:path');
  const { fileURLToPath } = await import('node:url');
  const { unstable_dev } = await import('wrangler');
  const directory = await mkdtemp(join(tmpdir(), 'diplodocus-markdown-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  await mkdir(join(directory, 'assets', 'guide'), { recursive: true });
  for (const path of ['index', 'guide/index', 'guide/page']) {
    await writeFile(join(directory, 'assets', `${path}.html`), '<!doctype html><h1>Guide</h1>');
    await writeFile(join(directory, 'assets', `${path}.md`), '# Guide\n');
  }
  await writeFile(join(directory, 'wrangler.json'), JSON.stringify({
    name: 'markdown-test', compatibility_date: '2026-10-06',
    assets: { directory: './assets', binding: 'ASSETS', run_worker_first: true },
  }));
  const server = await unstable_dev(fileURLToPath(new URL('../src/rendering/markdown_worker.js', import.meta.url)), {
    config: join(directory, 'wrangler.json'), local: true, logLevel: 'error',
    experimental: { disableExperimentalWarning: true, disableDevRegistry: true },
  });
  t.after(() => server.stop());
  for (const path of ['/', '/index.html', '/guide/', '/guide/index.html', '/guide/page', '/guide/page.html']) {
    const response = await fetch(`http://${server.address}:${server.port}${path}`, { headers: { Accept: 'text/markdown' } });
    assert.equal(response.status, 200, path);
    assert.equal(response.headers.get('Content-Type'), 'text/markdown; charset=utf-8', path);
    assert.match(response.headers.get('Vary'), /Accept/i);
    assert.equal(await response.text(), '# Guide\n');
  }
  const html = await server.fetch('/');
  assert.match(html.headers.get('Content-Type'), /text\/html/);
  assert.match(html.headers.get('Vary'), /Accept/i);
});

test('generated redirects preserve migrated guides and CLI pages in Workers', { timeout: 600_000 }, async (t) => {
  const { execFileSync } = await import('node:child_process');
  const { mkdtemp, mkdir, writeFile, rm } = await import('node:fs/promises');
  const { tmpdir } = await import('node:os');
  const { dirname, join } = await import('node:path');
  const { fileURLToPath } = await import('node:url');
  const { unstable_dev } = await import('wrangler');
  const directory = await mkdtemp(join(tmpdir(), 'diplodocus-docs-redirects-'));
  let server;
  t.after(async () => {
    await server?.stop();
    await rm(directory, { recursive: true, force: true });
  });
  const assetsDirectory = join(directory, 'assets');
  const migrations = [
    ...['authoring', 'configuration', 'diagnostics', 'execution', 'extraction',
      'installation', 'quick-start', 'snapshots'].map(name => [name, `guide/${name}`]),
    ['cli', 'reference/cli'],
    ['guide/cli', 'reference/cli'],
  ];
  const redirects = migrations.flatMap(([source, target]) =>
    ['', '.html'].map(suffix => `    /${source}${suffix}: ${target}.html`)).join('\n');
  await writeFile(join(directory, 'diplodocus.yaml'), `project:
  name: Redirects
presentation:
  canonical-url-style: clean
  redirects:
${redirects}
    /old-home: index.html
    /old-guide/: guide/index.html
    /café%20old: reference/cli.html
    /old%2dpath: reference/cli.html
    /old&path: reference/cli.html
repositories:
  - id: docs
    path: .
content:
  - id: guide
    owner: project
    repository: docs
    path: docs
    mount: ""
    format: gfm
`);
  for (const route of new Set(['index', 'guide/index', ...migrations.map(([, target]) => target)])) {
    await mkdir(dirname(join(directory, 'docs', route)), { recursive: true });
    await writeFile(join(directory, 'docs', `${route}.md`), '# Guide\n');
  }
  execFileSync('cargo', ['run', '--locked', '--', 'build', '--config',
    join(directory, 'diplodocus.yaml'), '--output', assetsDirectory], {
    cwd: new URL('../', import.meta.url),
  });
  execFileSync('sh', ['scripts/docs-assets.sh', assetsDirectory], {
    cwd: new URL('../', import.meta.url),
  });
  await writeFile(join(directory, 'wrangler.json'), JSON.stringify({
    name: 'docs-redirects-test', compatibility_date: '2026-10-06',
    assets: { directory: './assets', binding: 'ASSETS', run_worker_first: true },
  }));
  server = await unstable_dev(fileURLToPath(new URL('../src/rendering/markdown_worker.js', import.meta.url)), {
    config: join(directory, 'wrangler.json'), local: true, logLevel: 'error',
    experimental: { disableExperimentalWarning: true, disableDevRegistry: true },
  });
  const origin = `http://${server.address}:${server.port}`;
  for (const [source, target] of migrations) {
    for (const suffix of ['', '.html']) {
      const path = `/${source}${suffix}`;
      const response = await fetch(`${origin}${path}`, { redirect: 'manual' });
      assert.equal(response.status, 301, path);
      assert.equal(new URL(response.headers.get('Location'), origin).pathname, `/${target}`, path);
      await response.body?.cancel();
    }
    const destination = await fetch(`${origin}/${target}`, { redirect: 'manual' });
    assert.equal(destination.status, 200, target);
    await destination.body?.cancel();
  }
  for (const [source, target] of [['/old-home', '/'], ['/old-guide/', '/guide/'],
    ['/caf%C3%A9%20old', '/reference/cli'], ['/old%2dpath', '/reference/cli'],
    ['/old&path', '/reference/cli']]) {
    const response = await fetch(`${origin}${source}`, { redirect: 'manual' });
    assert.equal(response.status, 301, source);
    assert.equal(new URL(response.headers.get('Location'), origin).pathname, target, source);
    await response.body?.cancel();
  }
  const unlistedSpelling = await fetch(`${origin}/old-path`, { redirect: 'manual' });
  assert.equal(unlistedSpelling.status, 404);
  await unlistedSpelling.body?.cancel();
  const head = await fetch(`${origin}/configuration`, { method: 'HEAD', redirect: 'manual' });
  assert.equal(head.status, 301);
  assert.equal(await head.text(), '');
  const query = await fetch(`${origin}/configuration?from=old&view=full`, { redirect: 'manual' });
  assert.equal(query.status, 301);
  assert.equal(new URL(query.headers.get('Location'), origin).search, '?from=old&view=full');
  await query.body?.cancel();
  const markdown = await fetch(`${origin}/configuration.html`, { headers: { Accept: 'text/markdown' } });
  assert.equal(markdown.status, 200);
  assert.equal(markdown.url, `${origin}/guide/configuration`);
  assert.match(markdown.headers.get('Content-Type'), /^text\/markdown/);
  assert.match(await markdown.text(), /# Guide/);
  const home = await fetch(`${origin}/`, { redirect: 'manual' });
  assert.equal(home.status, 200);
  await home.body?.cancel();
  const missing = await fetch(`${origin}/missing-page`, { redirect: 'manual' });
  assert.equal(missing.status, 404);
  await missing.body?.cancel();
});
