(() => {
  const disclosure = document.querySelector('.nav-disclosure');
  const sidebar = document.querySelector('.layout > nav');
  const narrow = matchMedia('(max-width:760px)');
  const wide = matchMedia('(min-width:1100px)');
  const initializeToc = () => {
    const toc = document.querySelector('.toc-disclosure');
    if (!toc) return;
    toc.open = wide.matches;
    toc.dataset.ready = '';
  };
  disclosure.open = !narrow.matches;
  disclosure.dataset.ready = '';
  narrow.addEventListener('change', event => { disclosure.open = !event.matches; });
  wide.addEventListener('change', initializeToc);
  document.addEventListener('diplodocus:page-change', initializeToc);
  initializeToc();
  document.addEventListener('click', event => {
    const toc = document.querySelector('.toc-disclosure');
    if (wide.matches || !toc?.open) return;
    if (!toc.contains(event.target) || event.target.closest('a[href]')) toc.open = false;
  });
  document.addEventListener('keydown', event => {
    const toc = document.querySelector('.toc-disclosure');
    if (event.key !== 'Escape' || wide.matches || !toc?.open) return;
    event.preventDefault();
    toc.open = false;
    toc.querySelector('summary').focus({ preventScroll: true });
  });
  // Responsive navigation can change the sticky header's height.
  const header = document.querySelector('header');
  const updateHeaderBorder = () => header.toggleAttribute('data-scrolled', scrollY > 0);
  addEventListener('scroll', updateHeaderBorder, { passive: true });
  document.addEventListener('diplodocus:page-change', updateHeaderBorder);
  updateHeaderBorder();
  new ResizeObserver(() => {
    document.documentElement.style.setProperty('--header-height', `${header.getBoundingClientRect().height}px`);
  }).observe(header);

  if (!['http:', 'https:'].includes(location.protocol) || !window.fetch || !window.AbortController) return;
  const protocol = document.body.dataset.diplodocusNavigation;
  const shell = document.body.dataset.diplodocusShell;
  if (protocol !== '1' || !shell) return;
  document.documentElement.dataset.diplodocusRouter = 'true';

  const normalizeUrls = (root, base) => {
    for (const element of root.querySelectorAll('[href], [src]')) {
      for (const attribute of ['href', 'src']) {
        if (element.hasAttribute(attribute)) {
          element.setAttribute(attribute, new URL(element.getAttribute(attribute), base).href);
        }
      }
    }
  };
  // Retained elements must keep their destinations when pushState changes the base URL.
  normalizeUrls(document, location.href);
  document.querySelector('.skip-link').setAttribute('href', '#main');
  const home = document.querySelector('.brand').href;
  const documentKey = url => `${url.origin}${url.pathname}${url.search}`;
  const assetUrls = new Set();
  const rememberAssets = root => {
    for (const element of root.querySelectorAll('[src], link[rel="stylesheet"], link[rel="alternate"][type="text/markdown"]')) {
      assetUrls.add(documentKey(new URL(element.getAttribute('src') || element.getAttribute('href'))));
    }
  };
  rememberAssets(document);
  const emit = (name, detail) => document.dispatchEvent(new CustomEvent(`diplodocus:${name}`, { detail }));
  const sidebarStates = new Map();
  let serial = 0;
  let sequence = 0;
  let controller;
  let pendingPosition;
  let mountedUrl = new URL(location.href);
  let mountedEntry;
  const newEntry = () => ({ id: `${Date.now()}-${++serial}`, scroll: [0, 0], sidebar: [0, 0] });
  const entry = state => state?.diplodocus;
  const writeEntry = (value, url, push = false) => {
    const state = { ...history.state, diplodocus: value };
    if (push) history.pushState(state, '', url);
    else history.replaceState(state, '', url);
    mountedEntry = value;
    mountedUrl = new URL(url);
  };
  const initialEntry = entry(history.state);
  writeEntry(initialEntry || newEntry(), location.href);
  history.scrollRestoration = 'manual';

  const viewportEntry = () => ({
    ...mountedEntry,
    scroll: [scrollX, scrollY],
    sidebar: [disclosure.scrollTop, sidebar.scrollTop],
  });
  const recordPosition = () => {
    // During traversal the address bar already belongs to an unmounted page.
    if (pendingPosition || location.href !== mountedUrl.href || entry(history.state)?.id !== mountedEntry.id) return;
    writeEntry(viewportEntry(), location.href);
  };
  let scrollFrame;
  const recordScroll = () => {
    if (scrollFrame) return;
    scrollFrame = requestAnimationFrame(() => {
      scrollFrame = undefined;
      recordPosition();
    });
  };
  window.addEventListener('scroll', recordScroll, { passive: true });
  disclosure.addEventListener('scroll', recordScroll, { passive: true });
  sidebar.addEventListener('scroll', recordScroll, { passive: true });
  window.addEventListener('pagehide', recordPosition);
  window.addEventListener('pageshow', () => { history.scrollRestoration = 'manual'; });

  const rememberSidebar = () => {
    const contextual = document.querySelector('.contextual-nav');
    sidebarStates.set(contextual.dataset.section, {
      open: new Map(Array.from(contextual.querySelectorAll('.item-disclosure'), details => [details.querySelector('a').href, details.open])),
      scroll: [disclosure.scrollTop, sidebar.scrollTop],
    });
  };
  const updateSidebar = next => {
    const contextual = document.querySelector('.contextual-nav');
    const replacement = next.querySelector('.contextual-nav');
    const saved = sidebarStates.get(replacement.dataset.section);
    if (saved) {
      for (const details of replacement.querySelectorAll('.item-disclosure')) {
        details.open ||= saved.open.get(details.querySelector('a').href) || false;
      }
    }
    contextual.replaceWith(replacement);
    disclosure.scrollTop = saved?.scroll[0] || 0;
    sidebar.scrollTop = saved?.scroll[1] || 0;
    for (const selector of ['.desktop-sections', '.mobile-sections']) {
      const selected = next.querySelector(`${selector} [aria-current]`)?.href;
      for (const link of document.querySelectorAll(`${selector} a`)) {
        if (link.href === selected) link.setAttribute('aria-current', 'true');
        else link.removeAttribute('aria-current');
      }
    }
  };

  const metadata = 'meta[name="description"], meta[property^="og:"], meta[name^="twitter:"], link[rel="canonical"], link[rel="alternate"][type="text/markdown"]';
  const styles = new Map(Array.from(document.querySelectorAll('link[rel="stylesheet"]'), link => [link.href, Promise.resolve()]));
  const loadStyles = next => Promise.all(Array.from(next.querySelectorAll('link[rel="stylesheet"]'), source => {
    if (!styles.has(source.href)) {
      const link = source.cloneNode(true);
      const loaded = new Promise((resolve, reject) => {
        const timeout = setTimeout(() => failed(), 10000);
        const failed = () => {
          clearTimeout(timeout);
          link.remove();
          styles.delete(source.href);
          reject(new Error('Unable to load page styles.'));
        };
        link.addEventListener('load', () => { clearTimeout(timeout); resolve(); }, { once: true });
        link.addEventListener('error', failed, { once: true });
        document.head.append(link);
      });
      styles.set(source.href, loaded);
    }
    return styles.get(source.href);
  }));

  const fragmentTarget = url => {
    let target;
    try { target = document.getElementById(decodeURIComponent(url.hash.slice(1))); } catch { return; }
    if (!target) return;
    emit('reveal-fragment', { target });
    return target.classList.contains('tabset-heading') ? target.parentElement : target;
  };
  const focus = target => {
    if (!target.hasAttribute('tabindex') && target.tabIndex < 0) target.setAttribute('tabindex', '-1');
    target.focus({ preventScroll: true });
  };
  const positionPage = (url, saved, changedPage, moveFocus = true) => {
    const request = sequence;
    let active = true;
    const target = url.hash ? fragmentTarget(url) : undefined;
    if (narrow.matches) disclosure.open = false;
    if (moveFocus && (changedPage || target)) focus(target || document.querySelector('#main'));
    const correct = () => {
      if (!active || request !== sequence) return;
      if (saved) {
        window.scrollTo({ left: saved.scroll[0], top: saved.scroll[1], behavior: 'instant' });
        disclosure.scrollTop = saved.sidebar?.[0] || 0;
        sidebar.scrollTop = saved.sidebar?.[1] || 0;
      } else if (target) {
        target.scrollIntoView({ block: 'start', behavior: 'instant' });
      } else if (changedPage || !url.hash || url.hash.toLowerCase() === '#top') {
        window.scrollTo({ left: 0, top: 0, behavior: 'instant' });
      }
      // A new fragment entry needs a position even if another link wins before layout settles.
      if (!saved) writeEntry(viewportEntry(), location.href);
    };
    correct();
    if (!saved && !target) {
      recordPosition();
      return;
    }
    // Images without intrinsic dimensions can otherwise clamp and erase a saved position.
    const cleanup = [];
    const finish = (restore = true) => {
      if (pendingPosition !== finish) return;
      for (const dispose of cleanup) dispose();
      pendingPosition = undefined;
      if (restore) correct();
      active = false;
      if (request === sequence) recordPosition();
    };
    pendingPosition = finish;
    const main = document.querySelector('#main');
    const observer = new ResizeObserver(correct);
    observer.observe(main);
    cleanup.push(() => observer.disconnect());
    const images = Array.from(main.querySelectorAll('img')).filter(image => !image.complete);
    const loaded = images.map(image => new Promise(resolve => {
      const done = () => { correct(); resolve(); };
      image.addEventListener('load', done, { once: true });
      image.addEventListener('error', done, { once: true });
      cleanup.push(() => {
        image.removeEventListener('load', done);
        image.removeEventListener('error', done);
      });
    }));
    loaded.push(document.fonts.ready);
    Promise.all(loaded).then(() => requestAnimationFrame(() => requestAnimationFrame(() => finish())));
    const timeout = setTimeout(finish, 5000);
    cleanup.push(() => clearTimeout(timeout));
  };
  const cancelPosition = () => {
    const pending = pendingPosition;
    // Superseding navigation keeps the intended position until its entry is left.
    pending?.(false);
  };
  for (const event of ['wheel', 'touchstart', 'pointerdown', 'keydown']) {
    window.addEventListener(event, () => pendingPosition?.(false), { passive: true, capture: true });
  }

  const navigate = async (destination, traversal = false, state) => {
    const request = ++sequence;
    const positioning = !!pendingPosition;
    cancelPosition();
    controller?.abort();
    controller = new AbortController();
    const signal = controller.signal;
    if (!traversal && !positioning) recordPosition();
    const current = () => request === sequence;
    const saved = traversal ? entry(state) : undefined;
    try {
      if (documentKey(destination) === documentKey(mountedUrl)) {
        if (traversal) writeEntry(saved || newEntry(), destination.href);
        else if (destination.href !== location.href) writeEntry(newEntry(), destination.href, true);
        positionPage(destination, saved, false, !traversal);
        return;
      }
      const response = await fetch(destination, { signal, headers: { Accept: 'text/html' } });
      if (!current()) return;
      if (!response.ok || response.headers.get('content-type')?.split(';')[0].trim().toLowerCase() !== 'text/html'
          || response.headers.get('content-disposition')?.toLowerCase().startsWith('attachment')) {
        throw new Error('Destination is not a documentation page.');
      }
      const finalUrl = new URL(response.url);
      if (finalUrl.origin !== location.origin) throw new Error('Destination is outside this site.');
      finalUrl.hash = finalUrl.hash || destination.hash;
      const html = await response.text();
      if (!current()) return;
      const next = new DOMParser().parseFromString(html, 'text/html');
      normalizeUrls(next, finalUrl);
      if (next.body.dataset.diplodocusNavigation !== protocol || next.body.dataset.diplodocusShell !== shell
          || next.querySelector('.brand')?.href !== home || !next.querySelector('.contextual-nav')
          || !next.querySelector('.page-content main#main') || !next.querySelector('.desktop-sections')
          || !next.querySelector('.mobile-sections') || !next.querySelector('title')) {
        throw new Error('Destination has an incompatible site shell.');
      }
      await loadStyles(next);
      if (!current()) return;
      rememberAssets(next);
      if (!positioning) recordPosition();
      rememberSidebar();
      emit('before-page-change');
      writeEntry(saved || newEntry(), finalUrl.href, !traversal);
      document.title = next.title;
      for (const element of document.head.querySelectorAll(metadata)) element.remove();
      for (const element of next.head.querySelectorAll(metadata)) document.head.append(element);
      if (next.body.dataset.layout) document.body.dataset.layout = next.body.dataset.layout;
      else delete document.body.dataset.layout;
      updateSidebar(next);
      document.querySelector('.page-content').replaceWith(next.querySelector('.page-content'));
      document.querySelector('#search').value = '';
      document.querySelector('#search-results').replaceChildren();
      emit('page-change');
      positionPage(finalUrl, saved, true);
    } catch {
      if (!current() || signal.aborted) return;
      if (traversal) location.reload();
      else location.assign(destination.href);
    }
  };

  document.addEventListener('click', event => {
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    const link = event.target.closest?.('a[href]');
    if (!link || link.hasAttribute('download') || (link.target && link.target.toLowerCase() !== '_self')
        || link.relList.contains('external') || (link.type && link.type !== 'text/html')) return;
    const destination = new URL(link.getAttribute('href'), mountedUrl);
    if (!['http:', 'https:'].includes(destination.protocol) || destination.origin !== location.origin
        || assetUrls.has(documentKey(destination))) return;
    event.preventDefault();
    void navigate(destination);
  });
  const nativeFragment = () => {
    const destination = new URL(location.href);
    ++sequence;
    cancelPosition();
    controller?.abort();
    writeEntry(newEntry(), destination.href);
    positionPage(destination, undefined, false);
  };
  window.addEventListener('popstate', event => {
    const destination = new URL(location.href);
    // A native location.hash assignment clones the old entry instead of creating router state.
    if (documentKey(destination) === documentKey(mountedUrl) && destination.href !== mountedUrl.href
        && (!entry(event.state) || entry(event.state).id === mountedEntry.id)) nativeFragment();
    else void navigate(destination, true, event.state);
  });
  window.addEventListener('hashchange', () => {
    // Traversal also emits hashchange; only native fragment changes need another update.
    if (location.href === mountedUrl.href || documentKey(new URL(location.href)) !== documentKey(mountedUrl)) return;
    nativeFragment();
  });
  document.addEventListener('DOMContentLoaded', () => {
    if (initialEntry) positionPage(new URL(location.href), initialEntry, false, false);
    else {
      if (location.hash) positionPage(new URL(location.href), undefined, false, false);
      else recordPosition();
    }
  });
})();
