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
