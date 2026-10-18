// Real installed browser CDP + DOM + loopback TCP. Provider HTTP is fulfilled with a local
// fixture, so this neither signs in nor calls a paid/public AI endpoint.
import test from 'node:test';
import assert from 'node:assert/strict';
import net from 'node:net';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { launchBrowser } from './browser-cdp.mjs';
import { ChatHandoff, attachChat, PROVIDERS } from './web-chat.mjs';
import { callBridge } from './server.mjs';

export async function browserFixture(call, provider = 'chatgpt', operationsForContext, { headless = true, mobileComposer = false, enter = false, informational = false, reloadBeforeResponse = false, deferredSubmit = false, manualSend = false, delayedButton = false } = {}) {
  const testRoot = process.env.CONCAT_NATIVE_BROWSER_SMOKE ? process.env.TMP : fileURLToPath(new URL('../../.tmp/', import.meta.url));
  await mkdir(testRoot, { recursive: true });
  const scratch = await mkdtemp(path.join(testRoot, 'concat-chat-browser-'));
  let browser;
  try {
    browser = await launchBrowser(scratch, { headless });
    const { cdp, sessionId } = browser;
    const selectors = {
      chatgpt: mobileComposer
        ? ['<textarea id="mobile-composer-prompt" aria-label="Chat with ChatGPT"></textarea>', 'aria-label="Send message" type="submit"', 'data-message-author-role="assistant"']
        : ['<div id="prompt-textarea" contenteditable="true"></div>', 'data-testid="send-button"', 'data-message-author-role="assistant"'],
      gemini: ['<rich-textarea><div contenteditable="true"></div></rich-textarea>', 'class="send-button"', 'data-fixture="assistant"'],
      claude: ['<div class="ProseMirror" contenteditable="true"></div>', 'aria-label="Send message"', 'class="font-claude-response"'],
    }[provider];
    const html = `<!doctype html><html><body><form>${selectors[0]}<button ${selectors[1]}>Send</button></form><main></main><script>
      window.sent=[];
      if (${delayedButton}) document.querySelector('[contenteditable],textarea').addEventListener('input',()=>{
        document.querySelector('button').disabled=true;
        setTimeout(()=>{document.querySelector('button').disabled=false;},350);
      });
      function record(event) {
        event?.preventDefault();
        const input=document.querySelector('[contenteditable],textarea');
        window.sent.push('value' in input ? input.value : input.innerText);
        if('value' in input) input.value=''; else input.innerText='';
      }
      let attempts=0;
      document.querySelector('button').addEventListener('click',event=>{
        event.preventDefault();
        if (${manualSend} && ++attempts===1) { window.waitingManualSend=true; return; }
        if (${deferredSubmit}) setTimeout(()=>document.querySelector('form').dispatchEvent(new Event('submit',{bubbles:true,cancelable:true})),40);
        else record(event);
      });
      document.querySelector('form').addEventListener('submit',record);
    </script></body></html>`;
    await cdp.send('Fetch.enable', { patterns: [{ urlPattern: '*', requestStage: 'Request' }] }, sessionId);
    cdp.on('Fetch.requestPaused', (event, session) => {
      if (session !== sessionId) return;
      void cdp.send('Fetch.fulfillRequest', {
        requestId: event.requestId, responseCode: 200,
        responseHeaders: [{ name: 'Content-Type', value: 'text/html; charset=utf-8' }],
        body: Buffer.from(html).toString('base64'),
      }, sessionId).catch(() => {});
    });
    const handoff = new ChatHandoff(call);
    await attachChat(browser, provider, handoff);
    const evaluate = async expression => {
      const result = await cdp.send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true }, sessionId);
      if (result.exceptionDetails) throw new Error('Fixture page evaluation failed');
      return result.result?.value;
    };
    async function waitFor(expression) {
      for (let i = 0; i < 80; i++) { const result = await evaluate(expression); if (result) return result; await delay(150); }
      const status = await evaluate('document.querySelector("#concat-chat-status")?.innerText');
      throw new Error(`Browser fixture timed out: ${status || 'no adapter badge'}`);
    }
    await waitFor('window.__concatChatReady === true');
    if (!headless) {
      // Windows occlusion can mark a shown Chrome tab hidden when another app
      // is in front. Check native window state, not tab visibility.
      const { bounds } = await cdp.send('Browser.getWindowForTarget', { targetId: browser.targetId });
      assert.notEqual(bounds.windowState, 'minimized');
      assert.ok(bounds.width > 0 && bounds.height > 0);
    }
    const instruction = 'Ở clip đầu tiên, bỏ 2 giây đầu. Thêm tiêu đề Xin chào.';
    await evaluate(`{
      const input=document.querySelector('[contenteditable],textarea');
      if('value' in input) input.value=${JSON.stringify(instruction)}; else input.innerText=${JSON.stringify(instruction)};
      ${enter ? "input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true,cancelable:true}))" : "document.querySelector('button').click()"};
    }`);
    if (manualSend) {
      await waitFor('window.waitingManualSend === true');
      await evaluate('document.querySelector("button").click()');
    }
    await waitFor('window.sent?.length === 1');
    const message = await evaluate('window.sent[0]');
    assert.equal(message.split('CONCAT_EDIT_REQUEST_V1').length - 1, 1, 'context must never be wrapped twice');
    assert.match(message, /CONCAT_EDIT_REQUEST_V1/);
    assert.match(message, /Ở clip đầu tiên/);
    const context = JSON.parse(message.split('CONCAT PROJECT:\n')[1]);
    const envelope = {
      requestId: handoff.pending.requestId, scope: context.scope,
      plan: {
        version: 1, units: 'seconds', timelineId: context.timelineId, baseRevision: context.baseRevision,
        operations: operationsForContext ? operationsForContext(context) : [{ op: 'split', clipId: context.clipId || 'clip1', time: 4, as: 'tail' }, { op: 'remove', clipId: '@tail', ripple: false }],
      },
    };
    if (informational) { delete envelope.plan; envelope.message = 'Có, đã nhận cấu trúc project Concat từ adapter.'; }
    if (reloadBeforeResponse) {
      await cdp.send('Page.reload', {}, sessionId);
      await waitFor('window.__concatChatReady === true');
    }
    const replyTag = provider === 'gemini' ? 'model-response' : mobileComposer ? 'li' : 'div';
    await evaluate(`{
      const reply=document.createElement(${JSON.stringify(replyTag)}); reply.innerHTML=${JSON.stringify(provider === 'gemini' ? '<code data-test-id="code-content"></code>' : '<pre><code></code></pre>')};
      ${provider === 'chatgpt' ? mobileComposer ? "reply.setAttribute('data-message-role','assistant'); reply.setAttribute('data-message-complete','');" : "reply.setAttribute('data-message-author-role','assistant');" : provider === 'claude' ? "reply.className='font-claude-response';" : ''}
      reply.querySelector('code').textContent=${JSON.stringify(JSON.stringify(envelope))}; document.querySelector('main').append(reply);
    }`);
    await waitFor(`document.querySelector("#concat-chat-status")?.innerText.includes(${JSON.stringify(informational ? 'AI đã đọc thông tin project' : 'Đã chỉnh project')})`);
    // A DOM re-render of the same response must not apply twice.
    await evaluate('document.querySelector("main").append(document.querySelector("main").lastElementChild.cloneNode(true))');
    await delay(2500);
    return { message, context, envelope };
  } finally {
    await browser?.close();
    // This exact mkdtemp directory is our fixture, never a user browser profile.
    for (let attempt = 0; attempt < 30; attempt++) {
      try { await rm(scratch, { recursive: true }); break; }
      catch { await delay(200); }
    }
  }
}

if (!process.env.CONCAT_NATIVE_BROWSER_SMOKE) {
  const cases = [...Object.keys(PROVIDERS).map(provider => ({ provider, headless: true })),
    { provider: 'chatgpt', headless: false },
    { provider: 'chatgpt', headless: false, mobileComposer: true },
    { provider: 'chatgpt', headless: true, mobileComposer: true, enter: true },
    { provider: 'chatgpt', headless: true, mobileComposer: true, informational: true },
    { provider: 'chatgpt', headless: true, mobileComposer: true, reloadBeforeResponse: true },
    { provider: 'chatgpt', headless: true, mobileComposer: true, deferredSubmit: true },
    { provider: 'chatgpt', headless: true, mobileComposer: true, manualSend: true },
    { provider: 'chatgpt', headless: true, mobileComposer: true, delayedButton: true },
  ];
  for (const options of cases) {
    const { provider, headless, mobileComposer, enter, informational, reloadBeforeResponse, deferredSubmit, manualSend, delayedButton } = options;
    test(`${provider} ${headless ? 'headless' : 'visible window'}${mobileComposer ? ' mobile textarea' : ''}${enter ? ' Enter' : ''}${informational ? ' question' : ''}${reloadBeforeResponse ? ' reload' : ''}${deferredSubmit ? ' deferred submit' : ''}${manualSend ? ' manual send' : ''}${delayedButton ? ' delayed button' : ''}: project context and correlated response`, { timeout: 45000 }, async () => {
    const calls = [];
    const server = net.createServer(socket => socket.once('data', data => {
      const { request } = JSON.parse(data.toString());
      calls.push(request);
      socket.end(JSON.stringify(request.action === 'context'
        ? { ok: true, context: { scope: 'fixture:4', timelineId: 'timeline1', baseRevision: 4, clipId: 'clip1', editPlanSchema: {} } }
        : { ok: true, applied: true, count: 2 }) + '\n');
    }));
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    try {
      const connection = { version: 1, host: '127.0.0.1', port: server.address().port, token: 'a'.repeat(64) };
      await browserFixture(request => callBridge(connection, request), provider, undefined, options);
      assert.deepEqual(calls.map(call => call.action), informational ? ['context', 'context'] : reloadBeforeResponse ? ['context', 'context', 'context', 'propose'] : ['context', 'context', 'propose']);
      if (!informational) {
        const proposal = calls[calls.length - 1];
        assert.equal(proposal.scope, 'fixture:4');
        assert.equal(proposal.plan.operations.length, 2);
      }
    } finally { await new Promise(resolve => server.close(resolve)); }
  });
  }
}
