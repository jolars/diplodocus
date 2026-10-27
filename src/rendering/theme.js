(() => {
  const root = document.documentElement;
  const key = 'diplodocus-theme';
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
    for (const select of document.querySelectorAll('[data-theme-select]')) {
      select.value = preference;
    }
  };
  // Apply the preference before the stylesheet loads to avoid a flash of the wrong theme.
  apply(preference);
  document.addEventListener('DOMContentLoaded', () => {
    apply(preference);
    for (const select of document.querySelectorAll('[data-theme-select]')) {
      select.addEventListener('change', () => {
        apply(select.value);
        try {
          if (preference === 'system') localStorage.removeItem(key);
          else localStorage.setItem(key, preference);
        } catch {
          // The theme still works for this visit when storage is unavailable.
        }
      });
    }
  });
  addEventListener('storage', event => {
    if (event.key === key || event.key === null) apply(event.newValue);
  });
})();
