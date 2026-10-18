// SPDX-License-Identifier: AGPL-3.0-or-later
// Runs in the dedicated chat page. No private provider API/cookie access.
export function installDomBridge(settings) {
  if (location.origin !== settings.origin || window.__concatChatInstalled) return;
  window.__concatChatInstalled = true;
  const adapters = {
    chatgpt: {
      composer: '#prompt-textarea, #mobile-composer-prompt, textarea[data-id="root"], textarea[aria-label="Chat with ChatGPT"]',
      send: 'button[data-testid="send-button"], button[data-testid="composer-submit-button"], button[aria-label="Send prompt"], button[aria-label="Send message"], button[aria-label="Gửi tin nhắn"]',
      reply: '[data-message-author-role="assistant"], [data-message-role="assistant"]',
      streaming: 'button[data-testid="stop-button"], button[aria-label="Stop streaming"], button[aria-label="Stop response"], button[aria-label="Stop generating"]',
    },
    gemini: {
      composer: 'rich-textarea [contenteditable="true"], .ql-editor[contenteditable="true"]',
      send: 'button.send-button, button[aria-label="Send message"], button[aria-label="Gửi tin nhắn"]',
      reply: 'model-response',
      streaming: 'button[aria-label="Stop response"], button[aria-label="Dừng phản hồi"]',
    },
    claude: {
      composer: '[contenteditable="true"].ProseMirror, [contenteditable="true"][role="textbox"]',
      send: 'button[aria-label="Send message"], button[aria-label="Send Message"]',
      reply: '.font-claude-response, [data-testid="assistant-message"]',
      streaming: 'button[aria-label="Stop response"], button[aria-label="Stop Response"]',
    },
  };
  const adapter = adapters[settings.provider];
  if (!adapter) return;
  const waiting = new Map();
  let sequence = 0;
  let bypass = false;
  let submitting = false;
  let pending = null;
  let prepared = null;
  let observed = '';
  let observedAt = 0;
  let hint = 'Đang kiểm tra ô chat và project Concat…';
  let checkingReady = false;
  window.__concatChatReady = false;
  const badge = document.createElement('div');
  badge.id = 'concat-chat-status';
  Object.assign(badge.style, {
    position: 'fixed', left: '12px', bottom: '12px', zIndex: '2147483647', maxWidth: '440px',
    padding: '10px 14px', borderRadius: '12px', background: '#112431', color: '#bceeff',
    font: '13px/1.4 system-ui', border: '1px solid #2ac9ec', pointerEvents: 'none',
  });
  function status(text) { hint = text; badge.textContent = text; }
  function mount() { if (document.body && !badge.isConnected) document.body.append(badge); badge.textContent = hint; }
  mount();
  window.__concatDeliver = (id, result) => {
    const callback = waiting.get(id);
    if (!callback) return;
    waiting.delete(id);
    callback(result);
  };
  function call(action, payload = {}) {
    const id = ++sequence;
    return new Promise(resolve => {
      const timer = setTimeout(() => { waiting.delete(id); resolve({ ok: false, error: 'Concat chưa phản hồi. Hãy kiểm tra app còn mở.' }); }, 18000);
      waiting.set(id, result => { clearTimeout(timer); resolve(result); });
      window.__concatHost(JSON.stringify({ id, action, ...payload }));
    });
  }
  function text(input) { return ('value' in input ? input.value : input.innerText).trim(); }
  const visible = selector => [...document.querySelectorAll(selector)].find(element => element.getClientRects().length);
  function sendButton(input) {
    const known = visible(adapter.send);
    if (known && !known.matches(adapter.streaming)) return known;
    const fallback = input?.closest('form')?.querySelector('button[type="submit"]');
    return fallback?.getClientRects().length && !fallback.matches(adapter.streaming) ? fallback : null;
  }
  function write(input, value) {
    input.focus();
    if (input instanceof HTMLTextAreaElement || input instanceof HTMLInputElement) {
      const prototype = input instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(prototype, 'value').set.call(input, value);
      input.dispatchEvent(new Event('input', { bubbles: true }));
    } else {
      const range = document.createRange();
      range.selectNodeContents(input);
      const selection = window.getSelection();
      selection.removeAllRanges(); selection.addRange(range);
      if (!document.execCommand('insertText', false, value)) {
        input.textContent = value;
        input.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: value }));
      }
    }
  }
  async function submit(input) {
    const instruction = text(input);
    if (!instruction || submitting) return;
    if (pending && document.querySelector(adapter.streaming)) { status('AI còn đang trả lời yêu cầu trước.'); return; }
    submitting = true;
    try {
      status('Đang đính kèm project đang mở…');
      if (pending) { await call('cancel', { requestId: pending.requestId }); pending = null; }
      const result = await call('context', { instruction });
      if (!result.ok) { status(result.error || 'Chưa đọc được project Concat.'); return; }
      pending = { requestId: result.requestId, scope: result.scope };
      prepared = { requestId: result.requestId, message: result.message };
      write(input, result.message);
      // Controlled editors may replace/enable the button after a delayed render.
      let button;
      for (let attempt = 0; attempt < 20; attempt++) {
        await new Promise(resolve => setTimeout(resolve, 100));
        button = sendButton(visible(adapter.composer) || input);
        if (button && !button.disabled && button.getAttribute('aria-disabled') !== 'true') break;
      }
      if (!button || button.disabled || button.getAttribute('aria-disabled') === 'true') {
        await call('cancel', { requestId: pending.requestId });
        pending = null;
        prepared = null;
        write(input, instruction);
        status('Không tìm thấy nút gửi của website. Yêu cầu vẫn được giữ; thử lại sau khi đăng nhập.');
        return;
      }
      bypass = true;
      try { button.click(); } finally { bypass = false; }
      // A click is not proof the provider accepted a send. Do not auto-retry.
      await new Promise(resolve => setTimeout(resolve, 500));
      const current = visible(adapter.composer);
      status(current && text(current) === prepared?.message
        ? 'Project đã ghép, website chưa xác nhận gửi. Bấm Gửi một lần nữa; không ghép lại dữ liệu.'
        : 'Đã chuyển yêu cầu cho website · chờ AI lập thao tác chỉnh sửa…');
    } finally { submitting = false; }
  }
  function intercept(event) {
    if (bypass || event.defaultPrevented) return;
    const input = visible(adapter.composer);
    if (!input || !text(input)) return;
    const target = event.target instanceof Element ? event.target : null;
    const button = sendButton(input);
    const send = event.type === 'click' && button && (target === button || button.contains(target));
    const enter = event.type === 'keydown' && event.key === 'Enter' && !event.shiftKey && !event.isComposing && (target === input || input.contains(target));
    const form = event.type === 'submit' && (event.target.contains(input));
    if (!send && !enter && !form) return;
    // React may dispatch the form submit asynchronously after button.click().
    // Let that native path (or the user's retry click) send the SAME prepared
    // payload. Never treat our expanded project as a fresh user instruction.
    if (prepared && pending?.requestId === prepared.requestId && text(input) === prepared.message) return;
    event.preventDefault(); event.stopImmediatePropagation();
    if (text(input).startsWith('CONCAT_EDIT_REQUEST_V1\n')) {
      status('Ô chat còn dữ liệu ghép của phiên trước. Xóa nội dung ô nhập và nhập lại yêu cầu ngắn; chưa gửi thêm dữ liệu.');
      return;
    }
    prepared = null;
    void submit(input);
  }
  for (const name of ['click', 'keydown', 'submit']) document.addEventListener(name, intercept, true);
  setInterval(async () => {
    mount();
    if (!window.__concatChatReady && !checkingReady) {
      const input = visible(adapter.composer);
      if (!input || !sendButton(input)) {
        status('Chưa nhận diện ô chat/nút gửi. Đăng nhập AI hoặc chờ trang tải xong.');
        return;
      }
      checkingReady = true;
      try {
        const result = await call('ready');
        if (result.ok) {
          window.__concatChatReady = true;
          if (result.pending) pending = result.pending;
          if (!pending && !submitting) status(`Đã đọc project Concat · ${result.clipCount} clip · nhập yêu cầu chỉnh sửa tại đây`);
        } else status(result.error || 'Trang chat chưa đọc được project Concat.');
      } finally { checkingReady = false; }
    }
    if (!pending || submitting) return;
    const replies = document.querySelectorAll(adapter.reply);
    const latest = replies[replies.length - 1];
    if (!latest) return;
    if (latest.matches('[data-message-role="assistant"]') && !latest.hasAttribute('data-message-complete')) return;
    const source = latest.innerText;
    if (source !== observed) { observed = source; observedAt = Date.now(); return; }
    if (Date.now() - observedAt < 1800 || document.querySelector(adapter.streaming)) return;
    const blocks = [...latest.querySelectorAll('code, pre, .code-block-code, [data-test-id="code-content"]')].map(block => block.textContent);
    const candidates = blocks.length ? blocks : [source];
    for (const candidate of candidates) {
      let envelope;
      try { envelope = JSON.parse(candidate.trim()); } catch { continue; }
      if (envelope.requestId !== pending.requestId) continue;
      pending = null;
      if (typeof envelope.message === 'string' && !('plan' in envelope)) {
        const result = await call('answer', { envelope });
        status(result.ok ? 'AI đã đọc thông tin project · timeline giữ nguyên' : result.error || 'Chưa xác nhận được câu trả lời cho project.');
        return;
      }
      status('AI đã trả thao tác · Concat đang kiểm tra và áp dụng…');
      const result = await call('propose', { envelope });
      if (result.applied === true) status(`Đã chỉnh project Concat · ${result.count} thao tác · Ctrl+Z để hoàn tác`);
      else if (result.ok) status('Đã tạo bản nháp trong Concat · cần xem trước và duyệt');
      else status(result.error || 'Concat từ chối thao tác. Timeline chưa được xác nhận thay đổi.');
      // request ID is never replayed, even on timeout/unknown outcome.
      return;
    }
    if (source && Date.now() - observedAt > 12000) {
      status('AI chưa trả đúng thao tác JSON. Yêu cầu AI trả JSON theo CONCAT_EDIT_REQUEST; Concat chưa áp dụng.');
    }
  }, 600);
}
