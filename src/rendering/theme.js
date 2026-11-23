(() => {
  const root = document.documentElement;
  const key = 'diplodocus-theme';
  const system = matchMedia('(prefers-color-scheme: dark)');
  const normalize = value => ['light', 'dark'].includes(value) ? value : 'system';
  let preference = 'system';
  root.classList.add('js');
  try {
    preference = normalize(localStorage.getItem(key));
  } catch {
    // Storage can be unavailable in private or embedded browsing contexts.
  }
  const apply = value => {
    preference = normalize(value);
    root.dataset.theme = preference;
    const dark = preference === 'dark' || (preference === 'system' && system.matches);
    for (const toggle of document.querySelectorAll('[data-theme-toggle]')) {
      toggle.setAttribute('aria-checked', String(dark));
    }
  };
  // Apply the preference before the stylesheet loads to avoid a flash of the wrong theme.
  apply(preference);
  document.addEventListener('DOMContentLoaded', () => {
    apply(preference);
    for (const toggle of document.querySelectorAll('[data-theme-toggle]')) {
      toggle.addEventListener('click', () => {
        const next = toggle.getAttribute('aria-checked') === 'true' ? 'light' : 'dark';
        const systemTheme = system.matches ? 'dark' : 'light';
        apply(next === systemTheme ? 'system' : next);
        try {
          if (preference === 'system') {
            localStorage.removeItem(key);
          } else {
            localStorage.setItem(key, preference);
          }
        } catch {
          // The theme still works for this visit when storage is unavailable.
        }
      });
    }
  });
  system.addEventListener('change', () => apply(preference));
  addEventListener('storage', event => {
    if (event.key === key || event.key === null) apply(event.newValue);
  });
})();
