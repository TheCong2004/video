// Local fixture only. No model request, sign-in or real user project.
import assert from 'node:assert/strict';
import { browserFixture } from './web-chat-browser.test.mjs';
import { readConnection, callBridge } from './server.mjs';
const connection = await readConnection(process.argv[2]);
const result = await browserFixture(request => callBridge(connection, request), 'chatgpt', context => process.env.CONCAT_NATIVE_FIVE_SPLITS
  ? [8, 6, 4, 2].map((time, index) => ({ op: 'split', clipId: context.clipId, time, as: `tail_${index}` }))
  : [
  { op: 'trim', clipId: context.clipId, edge: 'start', delta: 2, ripple: true },
  { op: 'title', text: 'Xin chào', start: 1, duration: 3, as: 'greeting' },
]);
process.stderr.write('fixture: browser edit and cleanup finished; checking stale scope\n');
const stale = await callBridge(connection, { action: 'propose', scope: result.context.scope, plan: result.envelope.plan });
assert.equal(stale.ok, false);
assert.equal(stale.applied, false);
process.stdout.write('BROWSER_NATIVE_AUTO_APPLY_PASSED\n');
