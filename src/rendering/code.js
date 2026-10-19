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
      button.textContent = 'Copy';
      button.setAttribute('aria-label', 'Copy code');
      toolbar.append(status, button);
      pre.replaceWith(block);
      block.append(toolbar, pre);
      button.addEventListener('click', async () => {
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
          status.textContent = copied ? 'Copied' : 'Unable to copy. Select the code to copy it.';
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
