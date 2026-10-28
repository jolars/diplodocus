(() => {
  let observer;
  let controllers = [];
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
    controllers.push(controller);
    const { signal } = controller;
    const moduleUrl = new URL(root.dataset.componentModule, location.href).href;
    const assetBaseUrl = new URL('./', moduleUrl).href;
    const fallback = root.querySelector('[data-component-fallback]');
    const content = document.createElement('div');
    content.hidden = true;
    root.append(content);
    try {
      await Promise.all(JSON.parse(root.dataset.componentStyles).map(url => stylesheet(new URL(url, location.href).href)));
      if (signal.aborted) return;
      if (!modules.has(moduleUrl)) modules.set(moduleUrl, import(moduleUrl).catch(error => { modules.delete(moduleUrl); throw error; }));
      const component = await modules.get(moduleUrl);
      if (signal.aborted) return;
      const cleanup = await component.mount(content, { signal, assetBaseUrl });
      if (signal.aborted) { cleanup?.(); return; }
      if (cleanup) signal.addEventListener('abort', cleanup, { once: true });
      fallback.hidden = true;
      content.hidden = false;
      root.dataset.componentReady = 'true';
    } catch (error) {
      controller.abort();
      content.remove();
      if (root.isConnected) {
        const status = document.createElement('p');
        status.setAttribute('role', 'status');
        status.textContent = 'The interactive component could not load. The static explanation is available below.';
        root.prepend(status);
      }
    }
  };
  const dispose = () => {
    observer?.disconnect();
    observer = undefined;
    controllers.forEach(controller => controller.abort());
    controllers = [];
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
  document.addEventListener('diplodocus:before-page-change', dispose);
  document.addEventListener('diplodocus:page-change', initialize);
  window.addEventListener('pagehide', dispose);
  window.addEventListener('pageshow', event => { if (event.persisted) { dispose(); document.querySelectorAll('[data-component-ready]').forEach(root => { root.querySelector('[data-component-fallback]').hidden = false; root.querySelector('[data-component-fallback]').nextElementSibling?.remove(); delete root.dataset.componentReady; }); initialize(); } });
  initialize();
})();
