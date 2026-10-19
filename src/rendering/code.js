(() => {
  // Short blocks do not need a tab stop, but overflow must remain keyboard accessible.
  const focusOverflow = pre => {
    if (pre.scrollWidth > pre.clientWidth) pre.tabIndex = 0;
    else pre.removeAttribute('tabindex');
  };
  const observer = new ResizeObserver(entries => {
    for (const { target } of entries) focusOverflow(target);
  });
  const initialize = () => {
    observer.disconnect();
    for (const code of document.querySelectorAll('main pre > code')) {
      const pre = code.parentElement;
      if (pre.hasAttribute('data-signature') || code.closest('.code-output') || pre.parentElement.classList.contains('code-block')) continue;
      const block = document.createElement('div');
      block.className = 'code-block';
      const toolbar = document.createElement('div');
      toolbar.className = 'code-toolbar';
      const status = document.createElement('span');
      status.setAttribute('role', 'status');
      const button = document.createElement('button');
      button.type = 'button';
      button.innerHTML = '<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><rect x="8" y="3" width="8" height="4" rx="1"/><path d="M8 5H6a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-2"/></svg>';
      button.setAttribute('aria-label', 'Copy code');
      toolbar.append(status, button);
      pre.replaceWith(block);
      block.append(toolbar, pre);
      let feedbackTimer;
      button.addEventListener('click', async () => {
        clearTimeout(feedbackTimer);
        status.textContent = '';
        let copied = false;
        try {
          await navigator.clipboard.writeText(code.textContent);
          copied = true;
        } catch {
          if (!code.isConnected) return;
          // Static sites can run where the Clipboard API is unavailable.
          const text = document.createElement('textarea');
          text.value = code.textContent;
          text.className = 'copy-buffer';
          text.setAttribute('aria-hidden', 'true');
          document.body.append(text);
          text.select();
          try { copied = document.execCommand('copy'); } catch {}
          text.remove();
          button.focus({ preventScroll: true });
        }
        if (status.isConnected) {
          status.textContent = copied ? 'Copied!' : 'Unable to copy. Select the code to copy it.';
          feedbackTimer = setTimeout(() => { status.textContent = ''; }, 2500);
        }
      });
    }
    for (const pre of document.querySelectorAll('main pre')) {
      focusOverflow(pre);
      observer.observe(pre);
    }
  };
  document.addEventListener('diplodocus:before-page-change', () => observer.disconnect());
  document.addEventListener('diplodocus:page-change', initialize);
  initialize();
})();
