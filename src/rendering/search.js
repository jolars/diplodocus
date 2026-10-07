(() => {
  const script = document.currentScript;
  const root = new URL('../', script.src);
  const input = document.querySelector('#search');
  const results = document.querySelector('#search-results');
  let entries = [];

  function search() {
    results.replaceChildren();
    const query = input.value.trim().toLowerCase();
    if (!query) return;
    const rank = entry => {
      const title = entry.title.toLowerCase();
      if (title === query || title.endsWith(`.${query}`)) return 0;
      if (title.includes(query)) return 1;
      return 2;
    };
    const matches = entries
      .filter(entry => `${entry.title} ${entry.text} ${entry.package || ''}`.toLowerCase().includes(query))
      .sort((left, right) => rank(left) - rank(right))
      .slice(0, 12);
    for (const entry of matches) {
      const li = document.createElement('li');
      const link = document.createElement('a');
      link.href = new URL(entry.path, root);
      if (entry.api) {
        const code = document.createElement('code');
        code.textContent = entry.title;
        link.append(code);
      } else {
        link.append(entry.title);
      }
      if (entry.package) link.append(' · ' + entry.package);
      li.append(link);
      results.append(li);
    }
  }

  fetch(new URL('search.json', script.src))
    .then(response => response.json())
    .then(value => { entries = value; search(); })
    .catch(() => {});
  input.form.addEventListener('submit', event => event.preventDefault());
  input.addEventListener('input', search);
})();
