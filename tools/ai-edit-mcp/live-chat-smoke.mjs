// Opt-in, ONE real guest ChatGPT question using synthetic context only.
// Never signs in, accesses a user profile or edits a real project.
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import path from 'node:path';
import assert from 'node:assert/strict';
import { launchBrowser } from './browser-cdp.mjs';
import { ChatHandoff, attachChat } from './web-chat.mjs';
if (process.argv[2] !== '--allow-live-guest') throw new Error('Explicit --allow-live-guest required; sends one synthetic prompt.');
const testRoot = fileURLToPath(new URL('../../.tmp/', import.meta.url));
await mkdir(testRoot, { recursive: true });
const profile = await mkdtemp(path.join(testRoot, 'concat-live-guest-'));
let browser;
try {
  browser = await launchBrowser(profile);
  const context = {
    scope: 'synthetic:0', timelineId: 'synthetic-timeline', baseRevision: 0,
    timeline: { clips: [{ id: 'fixture-first', start: 0, duration: 10 }, { id: 'fixture-second', start: 10, duration: 5 }] },
    editPlanSchema: { type: 'object', description: 'This smoke check is read-only. Answer the informational question with a message envelope.' },
  };
  const handoff = new ChatHandoff(async request => request.action === 'context'
    ? { ok: true, context } : { ok: false, applied: false, error: 'Read-only synthetic test' });
  let answer;
  const acceptAnswer = handoff.answer.bind(handoff);
  handoff.answer = envelope => { const result = acceptAnswer(envelope); if (result.ok) answer = envelope; return result; };
  await attachChat(browser, 'chatgpt', handoff);
  const evaluate = async expression => {
    const result = await browser.cdp.send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true }, browser.sessionId);
    if (result.exceptionDetails) throw new Error('Live page evaluation failed');
    return result.result?.value;
  };
  async function waitFor(expression, seconds) {
    for (let i = 0; i < seconds * 2; i++) { if (await evaluate(expression)) return; await delay(500); }
    throw new Error(`Live check timed out: ${await evaluate('document.querySelector("#concat-chat-status")?.innerText')}`);
  }
  await waitFor('window.__concatChatReady === true', 45);
  console.log('REAL_PAGE_READY_WITH_SYNTHETIC_CONTEXT');
  const sent = await evaluate(`(() => {
    const input=[...document.querySelectorAll('#prompt-textarea,#mobile-composer-prompt,textarea[data-id="root"]')].find(element=>element.getClientRects().length);
    if(!input) return false;
    input.focus();
    const text='Bạn có nhận được thông tin project Concat mẫu này không? Cho biết project có bao nhiêu clip. Không chỉnh sửa gì.';
    if(input instanceof HTMLTextAreaElement) { Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set.call(input,text); input.dispatchEvent(new Event('input',{bubbles:true})); }
    else { input.textContent=text; input.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:text})); }
    input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true,cancelable:true}));
    return true;
  })()`);
  assert.equal(sent, true);
  console.log('ONE_REAL_QUESTION_DISPATCHED');
  await waitFor('document.querySelector("#concat-chat-status")?.innerText.includes("AI đã đọc thông tin project")', 90);
  assert.equal(answer?.scope, context.scope);
  assert.match(answer.message, /2|hai/i);
  console.log('REAL_CHATGPT_CORRELATED_ANSWER_PASS: ' + answer.message.slice(0, 500));
} finally {
  await browser?.close();
  // Only the fresh synthetic guest test profile, never the user's AI profile.
  for (let attempt = 0; attempt < 20; attempt++) {
    try { await rm(profile, { recursive: true, force: true }); break; } catch { await delay(200); }
  }
}
