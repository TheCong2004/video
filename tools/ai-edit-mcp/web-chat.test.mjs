import test from 'node:test';
import assert from 'node:assert/strict';
import { ChatHandoff, composeRequest } from './web-chat.mjs';
import { ensureChatTarget, selectBrowser, showBrowser } from './browser-cdp.mjs';

test('focus restores the owned native browser window before bringing its page forward', async () => {
  const calls = [];
  await showBrowser({ targetId: 'owned-page', sessionId: 'owned-session', cdp: {
    send: async (method, params, session) => {
      calls.push({ method, params, session });
      return method === 'Browser.getWindowForTarget' ? { windowId: 42 } : {};
    },
  } });
  assert.deepEqual(calls, [
    { method: 'Browser.getWindowForTarget', params: { targetId: 'owned-page' }, session: undefined },
    { method: 'Browser.setWindowBounds', params: { windowId: 42, bounds: { windowState: 'normal' } }, session: undefined },
    { method: 'Page.bringToFront', params: {}, session: 'owned-session' },
  ]);
});

test('Chrome is preferred even when Edge is installed', () => {
  const env = { ProgramFiles: 'D:/programs', 'ProgramFiles(x86)': 'D:/programs-x86', LOCALAPPDATA: 'D:/local' };
  assert.equal(selectBrowser(env, () => true).kind, 'chrome');
  const userChrome = selectBrowser(env, file => file.includes('local') || file.endsWith('msedge.exe'));
  assert.equal(userChrome.kind, 'chrome');
  assert.match(userChrome.executable, /chrome\.exe$/);
});

test('Edge is only a fallback when Chrome is absent', () => {
  const env = { ProgramFiles: 'D:/programs', 'ProgramFiles(x86)': 'D:/programs-x86' };
  assert.equal(selectBrowser(env, file => file.endsWith('msedge.exe')).kind, 'edge');
  assert.equal(selectBrowser(env, () => false), undefined);
});

test('visible Edge app mode with no startup page creates a controlled page', async () => {
  const calls = [];
  const target = await ensureChatTarget({ send: async (method, params) => {
    calls.push({ method, params });
    return method === 'Target.getTargets' ? { targetInfos: [] } : { targetId: 'created-page' };
  } });
  assert.equal(target, 'created-page');
  assert.equal(calls[1].method, 'Target.createTarget');
  assert.deepEqual(calls[1].params, { url: 'about:blank', newWindow: true });
});

test('startup page need not have the exact about:blank URL', async () => {
  let calls = 0;
  const target = await ensureChatTarget({ send: async method => {
    calls++;
    assert.equal(method, 'Target.getTargets');
    return { targetInfos: [{ type: 'browser', targetId: 'not-a-page' }, { type: 'page', targetId: 'startup-page', url: 'edge://newtab/' }] };
  } });
  assert.equal(target, 'startup-page');
  assert.equal(calls, 1);
});

const context = { scope: '1:9', timelineId: 'timeline-fixture', baseRevision: 9, timeline: { clips: [{ id: 'clip1', start: 0, duration: 10 }] }, editPlanSchema: { type: 'object' }, instruction: 'old API instruction', rules: 'Return {scope,plan}.' };
const plan = { version: 1, timelineId: context.timelineId, baseRevision: 9, units: 'seconds', operations: [] };

test('browser instructions explain sequential split IDs and absolute descending cuts', () => {
  const message = composeRequest('Cắt clip đầu thành 5 đoạn', context, 'split-request');
  const snapshot = JSON.parse(message.split('CONCAT PROJECT:\n')[1]);
  assert.match(snapshot.rules, /absolute timeline seconds/);
  assert.match(snapshot.rules, /original ID keeps the LEFT/);
  assert.match(snapshot.rules, /@alias/);
  assert.match(snapshot.rules, /descending/);
  assert.deepEqual(snapshot.splitExample.operations.map(op => op.time), [18, 16, 14, 12]);
  assert.ok(snapshot.splitExample.operations.every(op => op.clipId === 'EXAMPLE_CLIP'));
  assert.equal(new Set(snapshot.splitExample.operations.map(op => op.as)).size, 4);
  assert.match(snapshot.rules, /final total/);
});

test('each user send gets live project context and a unique correlated request', async () => {
  const calls = [];
  const handoff = new ChatHandoff(async request => {
    calls.push(request);
    if (request.action === 'context') return { ok: true, context };
    return { ok: true, applied: true, count: 2 };
  });
  const first = await handoff.context('Bỏ 2 giây đầu và thêm chữ Xin chào.');
  assert.equal(first.ok, true);
  assert.match(first.message, /Bỏ 2 giây đầu/);
  assert.match(first.message, /editPlanSchema/);
  assert.match(first.message, /"baseRevision":9/);
  assert.doesNotMatch(first.message, /old API instruction|Return \{scope,plan\}/);
  assert.equal((await handoff.context('second simultaneous instruction')).ok, false);
  const envelope = { requestId: first.requestId, scope: first.scope, plan };
  assert.deepEqual(await handoff.propose(envelope), { ok: true, applied: true, count: 2 });
  assert.equal((await handoff.propose(envelope)).ok, false);
  assert.equal(calls.filter(call => call.action === 'propose').length, 1);
  const second = await handoff.context('Chỉnh tiếp.');
  assert.notEqual(first.requestId, second.requestId);
});

test('unrelated, malformed and stale responses never reach the native edit endpoint', async () => {
  const calls = [];
  const handoff = new ChatHandoff(async request => { calls.push(request); return { ok: true, context }; });
  assert.equal((await handoff.propose({ requestId: 'unsolicited', scope: context.scope, plan })).ok, false);
  const first = await handoff.context('edit');
  assert.equal((await handoff.propose({ requestId: 'other', scope: first.scope, plan })).ok, false);
  assert.equal((await handoff.propose({ requestId: first.requestId, scope: 'stale', plan })).ok, false);
  const next = await handoff.context('try again');
  assert.equal((await handoff.propose({ requestId: next.requestId, scope: next.scope, plan, shell: 'no' })).ok, false);
  assert.equal(calls.filter(call => call.action === 'propose').length, 0);
});

test('unknown native apply outcome is not replayed', async () => {
  let applies = 0;
  const handoff = new ChatHandoff(async request => {
    if (request.action === 'context') return { ok: true, context };
    applies++;
    throw new Error('timeout after native commit');
  });
  const request = await handoff.context('edit');
  const envelope = { requestId: request.requestId, scope: request.scope, plan };
  await assert.rejects(handoff.propose(envelope));
  assert.equal((await handoff.propose(envelope)).ok, false);
  assert.equal(applies, 1);
});

test('closed/no project is reported and a user can retry after opening project', async () => {
  let open = false;
  const handoff = new ChatHandoff(async () => open ? { ok: true, context } : { ok: false, error: 'No project' });
  assert.equal((await handoff.context('edit')).ok, false);
  open = true;
  const request = await handoff.context('edit');
  assert.equal(request.ok, true);
  handoff.cancel(request.requestId);
  assert.equal((await handoff.context('new edit')).ok, true);
});

test('a connection question answers from context without editing or blocking the next instruction', async () => {
  const calls = [];
  const handoff = new ChatHandoff(async request => { calls.push(request); return { ok: true, context }; });
  const request = await handoff.context('Bạn đang kết nối với dự án của tôi?');
  assert.match(request.message, /LIVE snapshot/);
  assert.match(request.message, /informational answer/);
  assert.deepEqual(handoff.answer({ requestId: request.requestId, scope: request.scope, message: 'Có, nhận được timeline của project.' }), { ok: true, applied: false, state: 'answered' });
  assert.equal((await handoff.context('Cắt 2 giây đầu')).ok, true);
  assert.equal(calls.some(call => call.action === 'propose'), false);
});
