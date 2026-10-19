(() => {
  let tabsets = [];
  // Repeated initialization preserves selections without retaining removed pages.
  const initialized = new WeakMap();
  const uniqueId = base => {
    let id = base;
    for (let suffix = 2; document.getElementById(id); suffix++) id = `${base}-${suffix}`;
    return id;
  };
  const select = (tabset, index) => {
    tabset.tabs.forEach((tab, ordinal) => {
      const selected = ordinal === index;
      tab.button.setAttribute('aria-selected', String(selected));
      tab.button.tabIndex = selected ? 0 : -1;
      tab.panel.hidden = !selected;
    });
  };
  const activate = (tabset, index) => {
    select(tabset, index);
    if (!tabset.group) return;
    const label = tabset.tabs[index].label;
    for (const other of tabsets) {
      if (other === tabset || other.group !== tabset.group) continue;
      const matching = other.tabs.findIndex(tab => tab.label === label);
      if (matching !== -1) select(other, matching);
    }
  };
  const initialize = () => {
    tabsets = [];
    for (const element of document.querySelectorAll('.panel-tabset')) {
      const existing = initialized.get(element);
      if (existing) {
        tabsets.push(existing);
        continue;
      }
      const panels = Array.from(element.children).filter(child => child.classList.contains('tabset-panel'));
      if (!panels.length) continue;
      const list = document.createElement('div');
      list.className = 'tabset-controls';
      list.setAttribute('role', 'tablist');
      list.setAttribute('aria-label', 'Content options');
      const tabset = { element, group: element.dataset.tabsetGroup, tabs: [] };
      for (const panel of panels) {
        const heading = panel.firstElementChild;
        const button = document.createElement('button');
        button.type = 'button';
        button.textContent = heading.textContent;
        button.id = uniqueId(`${heading.id}-tab`);
        panel.id = uniqueId(`${heading.id}-panel`);
        button.setAttribute('role', 'tab');
        button.setAttribute('aria-controls', panel.id);
        panel.setAttribute('role', 'tabpanel');
        panel.setAttribute('aria-labelledby', button.id);
        panel.tabIndex = 0;
        const index = tabset.tabs.length;
        button.addEventListener('click', () => activate(tabset, index));
        button.addEventListener('keydown', event => {
          let next;
          if (event.key === 'ArrowRight') next = (index + 1) % panels.length;
          if (event.key === 'ArrowLeft') next = (index + panels.length - 1) % panels.length;
          if (event.key === 'Home') next = 0;
          if (event.key === 'End') next = panels.length - 1;
          if (next === undefined) return;
          event.preventDefault();
          activate(tabset, next);
          tabset.tabs[next].button.focus();
        });
        list.append(button);
        tabset.tabs.push({ button, panel, label: heading.textContent, active: heading.dataset.tabActive === 'true' });
      }
      element.insertBefore(list, panels[0]);
      element.classList.add('tabset-ready');
      tabsets.push(tabset);
      initialized.set(element, tabset);
      select(tabset, Math.max(0, tabset.tabs.findIndex(tab => tab.active)));
    }
  };
  const reveal = target => {
    for (const tabset of tabsets) {
      const index = tabset.tabs.findIndex(tab => tab.panel.contains(target));
      if (index !== -1) activate(tabset, index);
    }
  };
  const revealHash = () => {
    if (document.documentElement.dataset.diplodocusRouter === 'true') return;
    let target;
    try { target = document.getElementById(decodeURIComponent(location.hash.slice(1))); } catch { return; }
    if (!target) return;
    reveal(target);
    const headingPanel = target.classList.contains('tabset-heading') ? target.parentElement : target;
    headingPanel.scrollIntoView();
  };
  document.addEventListener('diplodocus:before-page-change', () => { tabsets = []; });
  document.addEventListener('diplodocus:page-change', initialize);
  document.addEventListener('diplodocus:reveal-fragment', event => {
    if (event.detail?.target instanceof Element) reveal(event.detail.target);
  });
  window.addEventListener('hashchange', revealHash);
  initialize();
  if (location.hash) revealHash();
})();
