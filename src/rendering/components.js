(() => {
  let observer;
  const instances = new Map();
  const modules = new Map();
  const styles = new Map();
  const stylesheet = url => {
    if (!styles.has(url)) styles.set(url, new Promise((resolve, reject) => {
      const link = document.createElement('link');
      link.rel = 'stylesheet';
      link.href = url;
      link.onload = resolve;
      link.onerror = () => { link.remove(); styles.delete(url); reject(new Error('Component stylesheet failed.')); };
      document.head.append(link);
    }));
    return styles.get(url);
  };
  const mount = async root => {
    if (!root.isConnected) return;
    const controller = new AbortController();
    const { signal } = controller;
    const moduleUrl = new URL(root.dataset.componentModule, location.href).href;
    const assetBaseUrl = new URL('./', moduleUrl).href;
    const fallback = root.querySelector(':scope > [data-component-fallback]');
    const content = document.createElement('div');
    content.dataset.componentContent = '';
    content.hidden = true;
    instances.set(root, { controller, content, fallback });
    root.append(content);
    try {
      const props = JSON.parse(root.dataset.componentProps ?? '{}');
      await Promise.all(JSON.parse(root.dataset.componentStyles).map(url => stylesheet(new URL(url, location.href).href)));
      if (signal.aborted) return;
      if (!modules.has(moduleUrl)) modules.set(moduleUrl, import(moduleUrl).catch(error => { modules.delete(moduleUrl); throw error; }));
      const component = await modules.get(moduleUrl);
      if (signal.aborted) return;
      const cleanup = await component.mount(content, { signal, assetBaseUrl, props });
      if (signal.aborted) { cleanup?.(); return; }
      if (cleanup) signal.addEventListener('abort', cleanup, { once: true });
      fallback.hidden = true;
      content.hidden = false;
      root.dataset.componentReady = 'true';
    } catch (error) {
      if (signal.aborted) return;
      controller.abort();
      content.remove();
      if (root.isConnected) {
        const status = document.createElement('p');
        root.dataset.componentFailed = 'true';
        status.dataset.componentError = '';
        status.setAttribute('role', 'status');
        status.textContent = 'The interactive component could not load. The static explanation is available below.';
        root.prepend(status);
      }
    }
  };
  const dispose = () => {
    observer?.disconnect();
    observer = undefined;
    for (const [root, { controller, content, fallback }] of instances) {
      controller.abort();
      content.remove();
      fallback.hidden = false;
      root.querySelector(':scope > [data-component-error]')?.remove();
      delete root.dataset.componentReady;
      delete root.dataset.componentFailed;
    }
    instances.clear();
  };
  const initialize = () => {
    const currentObserver = new IntersectionObserver(entries => {
      // Delivery can outlive the page that queued these visibility changes.
      if (observer !== currentObserver) return;
      for (const entry of entries) if (entry.isIntersecting) { currentObserver.unobserve(entry.target); mount(entry.target); }
    }, { rootMargin: '200px' });
    observer = currentObserver;
    document.querySelectorAll('[data-component-module]').forEach(root => currentObserver.observe(root));
  };
  if (!('IntersectionObserver' in window)) return;
  document.addEventListener('diplodocus:before-page-change', dispose);
  document.addEventListener('diplodocus:page-change', initialize);
  window.addEventListener('pagehide', dispose);
  window.addEventListener('pageshow', event => {
    if (event.persisted) initialize();
  });
  initialize();
  document.documentElement.dataset.diplodocusComponents = 'ready';
})();
