import MarkdownIt from './ask-ai-markdown.js';

(() => {
  const trigger = document.querySelector('.site-ask-ai');
  const dialog = document.querySelector('dialog.ask-ai-dialog');
  if (!trigger || !dialog) return;

  // Model output may contain arbitrary HTML or URLs. Render only Markdown and
  // leave images inert so an answer cannot trigger a third-party request.
  const markdown = new MarkdownIt({ html: false }).disable('image');
  markdown.validateLink = target => {
    if (!URL.canParse(target, location.href)) return false;
    const url = new URL(target, location.href);
    return ['https:', 'http:'].includes(url.protocol) && !url.username && !url.password;
  };

  const endpoint = new URL(dialog.dataset.endpoint, location.origin);
  const conversation = dialog.querySelector('.ask-ai-conversation');
  const status = dialog.querySelector('.ask-ai-status');
  const form = dialog.querySelector('form');
  const input = dialog.querySelector('textarea');
  const reset = dialog.querySelector('.ask-ai-reset');
  const send = dialog.querySelector('.ask-ai-send');
  const stop = dialog.querySelector('.ask-ai-stop');

  const id = () => crypto.getRandomValues(new Uint32Array(4)).join('-');
  const message = (role, text) => ({ id: id(), role, parts: [{ type: 'text', text }] });
  let chatId = id();
  let history = [];
  let active;

  async function* events(body) {
    const reader = body.pipeThrough(new TextDecoderStream()).getReader();
    let buffer = '';
    let data = [];
    try {
      while (true) {
        const { value, done } = await reader.read();
        buffer += value ?? '';
        // Wait for the next chunk when it may start with the LF of a CRLF pair.
        const lineEnding = done ? /\r\n|[\r\n]/ : /\r\n|\n|\r(?!$)/;
        for (let end; (end = lineEnding.exec(buffer));) {
          const line = buffer.slice(0, end.index);
          buffer = buffer.slice(end.index + end[0].length);
          if (!line && data.length) {
            yield data.join('\n');
            data = [];
          } else if (line === 'data' || line.startsWith('data:')) {
            data.push(line.slice(5).replace(/^ /, ''));
          }
        }
        if (done) return;
      }
    } finally {
      await reader.cancel().catch(() => {});
      reader.releaseLock();
    }
  }

  function appendSource(part, list, seen) {
    if (typeof part.url !== 'string' || !URL.canParse(part.url)) return;
    const url = new URL(part.url);
    if (!['https:', 'http:'].includes(url.protocol) || url.username || url.password || seen.has(url.href)) return;
    seen.add(url.href);
    const item = document.createElement('li');
    const link = document.createElement('a');
    const icon = document.createElement('span');
    const label = document.createElement('span');
    link.href = url.href;
    icon.className = 'ask-ai-source-icon';
    icon.setAttribute('aria-hidden', 'true');
    icon.textContent = '#';
    label.className = 'ask-ai-source-title';
    label.textContent = (typeof part.title === 'string' && part.title.trim()) || url.href;
    link.title = label.textContent;
    link.append(icon, label);
    item.append(link);
    list.append(item);
  }

  function resizeInput() {
    if (!dialog.open) return;
    input.style.height = '';
    input.style.height = `${input.scrollHeight}px`;
  }

  input.addEventListener('input', resizeInput);
  input.addEventListener('keydown', event => {
    if (event.key !== 'Enter' || !event.ctrlKey || event.altKey || event.metaKey || event.shiftKey
        || event.isComposing) return;
    event.preventDefault();
    if (!active && input.value.trim()) form.requestSubmit();
  });
  window.addEventListener('resize', resizeInput);
  trigger.addEventListener('click', () => {
    dialog.showModal();
    resizeInput();
  });
  dialog.querySelector('.ask-ai-close').addEventListener('click', () => dialog.close());
  dialog.addEventListener('close', () => {
    if (!dialog.open) active?.abort();
  });
  dialog.addEventListener('click', event => {
    if (!event.metaKey && !event.ctrlKey && !event.shiftKey && !event.altKey && event.target.closest('a')) {
      dialog.close();
    }
  });
  document.addEventListener('diplodocus:before-page-change', () => dialog.close());
  stop.addEventListener('click', () => {
    active?.abort();
    input.focus();
  });
  reset.addEventListener('click', () => {
    chatId = id();
    history = [];
    conversation.replaceChildren();
    conversation.hidden = true;
    status.textContent = '';
    input.value = '';
    resizeInput();
    input.focus();
    reset.hidden = true;
  });

  form.addEventListener('submit', async event => {
    event.preventDefault();
    const question = input.value.trim();
    if (!question || active) return;

    const request = new AbortController();
    active = request;
    const deadline = AbortSignal.timeout(90_000);
    const messages = [...history.slice(-10), message('user', question)];
    const turn = document.createElement('section');
    turn.className = 'ask-ai-turn';
    turn.innerHTML = '<h3></h3><div class="ask-ai-answer"></div><div class="ask-ai-sources" hidden><h4>Related sources</h4><ul role="list"></ul></div>';
    const heading = turn.querySelector('h3');
    heading.textContent = question;
    const answer = turn.querySelector('.ask-ai-answer');
    const sources = turn.querySelector('ul');
    conversation.hidden = false;
    conversation.append(turn);
    conversation.scrollTop = conversation.scrollHeight;
    input.value = '';
    resizeInput();
    input.focus();
    send.hidden = true;
    stop.hidden = false;
    reset.hidden = false;
    reset.disabled = true;
    status.textContent = 'Waiting for an answer…';

    const blocks = new Map();
    const seen = new Set();
    const answerText = () => Array.from(blocks.values()).join('\n\n');
    const followsAnswer = () => conversation.scrollHeight - conversation.clientHeight - conversation.scrollTop < 48;
    const scrollToAnswer = () => { conversation.scrollTop = conversation.scrollHeight; };
    let renderFrame;
    function renderAnswer() {
      renderFrame = undefined;
      const follow = followsAnswer();
      answer.innerHTML = markdown.render(answerText());
      if (follow) scrollToAnswer();
    }
    function flushAnswer() {
      if (renderFrame === undefined) return;
      cancelAnimationFrame(renderFrame);
      renderAnswer();
    }
    try {
      const response = await fetch(endpoint, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Accept: 'text/event-stream' },
        body: JSON.stringify({ id: chatId, messages, trigger: 'submit-message' }),
        signal: AbortSignal.any([request.signal, deadline]),
      });
      if (response.status === 429) {
        status.textContent = 'Too many questions. Try again in a moment.';
        return;
      }
      if (!response.ok || !response.body || !/^text\/event-stream(?:\s*;|$)/i.test(response.headers.get('Content-Type') ?? '')) {
        throw new Error('Expected a chat stream');
      }
      let complete = false;
      for await (const data of events(response.body)) {
        if (data === '[DONE]') {
          complete = true;
          break;
        }
        const part = JSON.parse(data);
        if (part.type === 'text-delta' && typeof part.delta === 'string' && part.delta) {
          if (!blocks.size) status.textContent = 'Writing an answer…';
          blocks.set(part.id, (blocks.get(part.id) ?? '') + part.delta);
          renderFrame ??= requestAnimationFrame(renderAnswer);
        } else if (part.type === 'source-url') {
          appendSource(part, sources, seen);
        } else if (part.type === 'error' || part.type === 'abort') {
          throw new Error('Chat stream failed');
        }
      }
      const text = answerText();
      if (!complete || !text.trim()) throw new Error('Incomplete answer');
      history = [...messages, message('assistant', text)];
      flushAnswer();
      if (seen.size) {
        const follow = followsAnswer();
        sources.parentElement.hidden = false;
        if (follow) scrollToAnswer();
      }
      status.textContent = 'Answer ready.';
    } catch {
      flushAnswer();
      status.textContent = request.signal.aborted ? 'Answer stopped.'
        : deadline.aborted ? 'The answer timed out. Try again.'
        : answer.textContent ? 'The answer was interrupted. Try again.'
        : 'Ask AI is temporarily unavailable. Try again.';
    } finally {
      active = undefined;
      if (document.activeElement === stop) input.focus();
      send.hidden = false;
      stop.hidden = true;
      reset.disabled = false;
      if (!answer.textContent && !request.signal.aborted && !input.value) {
        input.value = question;
        resizeInput();
      }
    }
  });

  trigger.hidden = false;
})();
