(() => {
  const disclosure = document.querySelector('.nav-disclosure');
  const narrow = matchMedia('(max-width:760px)');
  const toc = document.querySelector('.toc-disclosure');
  const wide = matchMedia('(min-width:1100px)');
  disclosure.open = !narrow.matches;
  narrow.addEventListener('change', event => { disclosure.open = !event.matches; });
  if (toc) {
    toc.open = wide.matches;
    wide.addEventListener('change', event => { toc.open = event.matches; });
  }
  // Branding and extra package tabs can change the sticky header's height.
  const header = document.querySelector('header');
  new ResizeObserver(() => {
    document.documentElement.style.setProperty('--header-height', `${header.getBoundingClientRect().height}px`);
  }).observe(header);
})();
