(() => {
  const script = document.currentScript;
  const root = new URL('../', script.src);
  const trigger = document.querySelector('button.site-search');
  const dialog = document.querySelector('.search-dialog');
  const input = dialog.querySelector('#search');
  const results = dialog.querySelector('#search-results');
  const status = dialog.querySelector('[role="status"]');
  const retry = dialog.querySelector('.search-retry');
  let entries;
  const pages = new Map();
  const sections = new Map();
  let loading = false;
  let failed = false;
  let opener;
  let indexRevision = 0;

  function excerpt(text, query) {
    const fragment = document.createDocumentFragment();
    const pattern = new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'iu');
    const match = pattern.exec(text);
    let start = match ? Math.max(0, match.index - 65) : 0;
    if (start) {
      const boundary = text.indexOf(' ', start);
      if (boundary !== -1 && boundary < match.index) start = boundary + 1;
    }
    let end = Math.min(text.length, Math.max(start + 175, (match?.index ?? 0) + query.length + 35));
    if (end < text.length) {
      const boundary = text.lastIndexOf(' ', end);
      if (boundary > Math.max(start, (match?.index ?? start) + (match?.[0].length ?? 0))) end = boundary;
    }
    if (start) fragment.append('…');
    if (match) {
      fragment.append(text.slice(start, match.index));
      const mark = document.createElement('mark');
      mark.textContent = match[0];
      fragment.append(mark, text.slice(match.index + match[0].length, end));
    } else {
      fragment.append(text.slice(start, end));
    }
    if (end < text.length) fragment.append('…');
    return fragment;
  }

  const pageUrl = path => new URL(path.split('#')[0], root).href;

  function search() {
    results.replaceChildren();
    retry.hidden = !failed;
    const query = input.value.trim().toLowerCase();
    if (!entries || !query) {
      status.textContent = failed ? 'Search could not load.' : loading ? 'Loading search…' : '';
      return;
    }
    const includes = value => value.toLowerCase().includes(query);
    const rank = entry => {
      const title = entry.title.toLowerCase();
      if (title === query || title.endsWith(`.${query}`) || title.endsWith(`::${query}`)) return 0;
      return includes(title) ? 1 : 2;
    };
    const matches = new Map();
    for (const entry of entries) {
      const url = new URL(entry.path, root).href;
      const section = sections.get(url);
      const sectionMatch = entry.sections?.some(item => includes(item.headings.at(-1)) || includes(item.text));
      const ownText = section?.text || entry.intro || '';
      if (sectionMatch && rank(entry) === 2 && !includes(ownText)) continue;
      if (includes(`${entry.title} ${entry.text} ${entry.package || ''}`) || includes(ownText)) {
        matches.set(url, {
          ...entry, url,
          trail: section?.headings.slice(0, -1),
          preview: ownText || entry.sections?.find(item => item.text)?.text,
        });
      }
    }
    for (const [url, section] of sections) {
      const title = section.headings.at(-1);
      if (!matches.has(url) && (includes(title) || includes(section.text))) {
        matches.set(url, { url, path: section.path, title, trail: section.headings.slice(0, -1), preview: section.text });
      }
    }
    const groups = new Map();
    const limit = 12;
    for (const hit of [...matches.values()].sort((a, b) => rank(a) - rank(b))) {
      const url = pageUrl(hit.path);
      if (!groups.has(url)) {
        if (groups.size === limit) continue;
        groups.set(url, []);
      }
      groups.get(url).push(hit);
    }
    // Leave room for multiple sections per page, with longer groups when few pages match.
    const perPage = Math.max(3, Math.floor(limit / Math.max(1, groups.size)));
    const count = [...groups.values()].reduce((total, group) => total + Math.min(group.length, perPage), 0);
    status.textContent = count ? `${count} ${count === 1 ? 'result' : 'results'}` : 'No results found.';
    for (const [url, hits] of groups) {
      const group = hits.slice(0, perPage);
      const page = pages.get(url);
      const item = document.createElement('li');
      const context = page?.context || page?.package;
      if (context && context !== page?.title) {
        const label = document.createElement('div');
        label.className = 'search-context';
        label.textContent = context;
        item.append(label);
      }
      if (!group.some(hit => hit.url === url) && page) {
        const heading = document.createElement('div');
        heading.className = 'search-page-title';
        heading.textContent = page.title;
        item.append(heading);
      }
      for (const hit of group) {
        const row = document.createElement('div');
        row.className = 'search-hit';
        const link = document.createElement('a');
        link.href = hit.url;
        const description = [
          context,
          hit.url === url ? undefined : page?.title,
          ...(hit.trail || []),
        ].filter(label => label && label !== hit.title);
        if (description.length) link.setAttribute('aria-description', [...new Set(description)].join(' › '));
        if (hit.trail?.length) {
          const trail = document.createElement('span');
          trail.className = 'search-context';
          trail.textContent = hit.trail.join(' › ');
          row.append(trail);
        }
        const title = hit.api ? document.createElement('code') : document.createElement('span');
        title.textContent = hit.title;
        link.append(title);
        row.append(link);
        if (hit.preview) {
          const preview = document.createElement('div');
          preview.className = 'search-preview';
          preview.append(excerpt(hit.preview, query));
          row.append(preview);
        }
        item.append(row);
      }
      results.append(item);
    }
  }

  async function load() {
    if (entries || loading) return;
    const revision = indexRevision;
    loading = true;
    failed = false;
    search();
    try {
      const response = await fetch(new URL('search.json', script.src));
      if (!response.ok) throw new Error('Search index unavailable');
      const index = await response.json();
      if (revision !== indexRevision) return;
      if (!Array.isArray(index)) throw new Error('Invalid search index');
      for (const entry of index) {
        if (entry.path.includes('#')) continue;
        pages.set(pageUrl(entry.path), entry);
        for (const section of entry.sections || []) {
          sections.set(new URL(section.path, root).href, section);
        }
      }
      entries = index;
    } catch {
      if (revision === indexRevision) {
        pages.clear();
        sections.clear();
        failed = true;
      }
    } finally {
      if (revision === indexRevision) {
        loading = false;
        if (dialog.open) search();
      }
    }
  }

  function open(source) {
    if (dialog.open) return;
    opener = source;
    input.value = '';
    dialog.showModal();
    input.focus();
    search();
    void load();
  }

  trigger.addEventListener('click', () => open(trigger));
  retry.addEventListener('click', () => {
    input.focus();
    void load();
  });
  dialog.querySelector('.search-close').addEventListener('click', () => dialog.close());
  dialog.addEventListener('click', event => {
    if (event.target === dialog) {
      const bounds = dialog.getBoundingClientRect();
      if (event.clientX < bounds.left || event.clientX >= bounds.right
          || event.clientY < bounds.top || event.clientY >= bounds.bottom) dialog.close();
    } else if (event.target.closest('#search-results a') && !event.ctrlKey && !event.metaKey
        && !event.shiftKey && !event.altKey) {
      dialog.close();
    }
  });
  dialog.addEventListener('close', () => {
    if (!dialog.open && (document.activeElement === document.body || dialog.contains(document.activeElement))) {
      opener?.focus({ preventScroll: true });
    }
  });
  const close = () => { if (dialog.open) dialog.close(); };
  document.addEventListener('diplodocus:before-page-change', event => {
    if (!event.detail?.preview) close();
  });
  document.addEventListener('diplodocus:preview-search', () => {
    indexRevision++;
    entries = undefined;
    loading = false;
    failed = false;
    pages.clear();
    sections.clear();
    if (dialog.open) void load();
  });
  window.addEventListener('popstate', close);
  document.addEventListener('keydown', event => {
    if ((event.ctrlKey || event.metaKey) && !event.shiftKey && !event.altKey && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      open(document.activeElement === document.body ? trigger : document.activeElement);
    }
  });
  input.form.addEventListener('submit', event => {
    event.preventDefault();
    results.querySelector('a')?.click();
  });
  input.addEventListener('input', () => {
    search();
    results.scrollTop = 0;
  });
  dialog.addEventListener('keydown', event => {
    if (event.isComposing) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      dialog.close();
      event.stopPropagation();
      return;
    }
    if (!['ArrowDown', 'ArrowUp'].includes(event.key)) return;
    const links = [...results.querySelectorAll('a')];
    if (!links.length) return;
    if (event.target === input && event.key === 'ArrowDown') {
      event.preventDefault();
      links[0].focus();
      return;
    }
    const index = links.indexOf(event.target);
    if (index === -1) return;
    event.preventDefault();
    if (event.key === 'ArrowUp') (links[index - 1] || input).focus();
    else links[index + 1]?.focus();
  });
})();
