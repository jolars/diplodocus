// Deploy this entry point with an ASSETS binding and run_worker_first = true.
function acceptsMarkdown(accept) {
  let markdown = 0;
  let html = 0;
  for (const range of accept.split(',')) {
    const [media, ...parameters] = range.trim().split(';');
    const q = parameters.map(p => p.trim().split('='))
      .find(([key]) => key.toLowerCase() === 'q');
    const quality = q ? Number(q[1]) : 1;
    if (!Number.isFinite(quality) || quality < 0 || quality > 1) continue;
    if (media.trim().toLowerCase() === 'text/markdown') markdown = Math.max(markdown, quality);
    if (media.trim().toLowerCase() === 'text/html') html = Math.max(html, quality);
  }
  return markdown > 0 && markdown >= html;
}

export default {
  async fetch(request, env) {
    const wantsMarkdown = acceptsMarkdown(request.headers.get('Accept') || '');
    const lookup = new Request(request);
    if (wantsMarkdown) {
      // Validators belong to one representation and cannot be reused for its sibling.
      for (const name of ['If-None-Match', 'If-Modified-Since', 'If-Range', 'Range']) lookup.headers.delete(name);
    }
    const response = await env.ASSETS.fetch(lookup);
    if (!['GET', 'HEAD'].includes(request.method) || ![200, 304].includes(response.status) ||
        (response.status !== 304 && !response.headers.get('Content-Type')?.startsWith('text/html'))) return response;
    const headers = new Headers(response.headers);
    const vary = headers.get('Vary');
    if (!vary?.split(',').some(value => value.trim().toLowerCase() === 'accept')) {
      headers.set('Vary', vary ? `${vary}, Accept` : 'Accept');
    }
    if (wantsMarkdown) {
      const url = new URL(request.url);
      if (url.pathname.endsWith('/')) url.pathname += 'index.md';
      else if (url.pathname.endsWith('.html')) url.pathname = url.pathname.slice(0, -5) + '.md';
      else url.pathname += '.md';
      const markdown = await env.ASSETS.fetch(new Request(url, lookup));
      if (markdown.status === 200 && !markdown.headers.get('Content-Type')?.startsWith('text/html')) {
        const markdownHeaders = new Headers(markdown.headers);
        markdownHeaders.set('Content-Type', 'text/markdown; charset=utf-8');
        markdownHeaders.set('Vary', headers.get('Vary'));
        await response.body?.cancel();
        return new Response(markdown.body, { status: 200, headers: markdownHeaders });
      }
    }
    return new Response(response.body, { status: response.status, headers });
  },
};
