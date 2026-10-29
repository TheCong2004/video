import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { compileController } from '../../scripts/patch-live-controller.mjs';
const { ArtCraftLiveRuntimeController: Controller } = await import(`data:text/javascript;base64,${Buffer.from(compileController().standalone).toString('base64')}`);
const events = { webcast: { chat: 'chat', stream_end: 'streamEnd' }, disconnected: 'disconnected', error: 'error' };
const start = { protocolVersion: 1, type: 'start', requestId: 'start', username: 'fixture' };
function fixture(t) {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const frames = [];
  const connection = new EventEmitter();
  connection.isConnected = false;
  connection.calls = 0;
  connection.connect = async () => { connection.calls++; connection.isConnected = true; return { isConnected: true }; };
  connection.disconnect = async () => { connection.isConnected = false; };
  const controller = new Controller(() => connection, events, frame => frames.push(frame));
  return { connection, frames, controller };
}
const flush = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };

test('a malformed packet does not disable a healthy connection or leak its content', async t => {
  const { connection, frames, controller } = fixture(t);
  await controller.handle(start);
  connection.emit('error', { info: 'Websocket message decoding failed', exception: new Error('private-cookie') });
  assert.equal(frames.filter(f => f.type === 'runtime.status').at(-1).status, 'connected');
  assert.equal(connection.calls, 1);
  assert.ok(!JSON.stringify(frames).includes('private-cookie'));
});

test('error-only loss recovers once, ignores repeated errors, and preserves event deduplication', async t => {
  const { connection, frames, controller } = fixture(t);
  await controller.handle(start);
  const chat = { common: { msgId: 'm1', roomId: 'r1' }, content: 'hello' };
  connection.emit('chat', chat);
  connection.isConnected = false;
  connection.emit('error', { exception: new Error('private-token') });
  connection.emit('error', { exception: new Error('private-token') });
  assert.equal(frames.at(-1).code, 'LIVE_RECONNECT_SCHEDULED');
  t.mock.timers.tick(1000); await flush();
  assert.equal(connection.calls, 2);
  assert.equal(frames.at(-1).status, 'connected');
  connection.emit('chat', chat);
  assert.equal(frames.filter(f => f.type === 'live.event').length, 1);
});

test('manual stop cancels retries and stream end never reconnects', async t => {
  const { connection, frames, controller } = fixture(t);
  await controller.handle(start);
  connection.isConnected = false;
  connection.emit('disconnected', { code: 1006 });
  await controller.handle({ protocolVersion: 1, type: 'stop', requestId: 'stop' });
  t.mock.timers.tick(30000); await flush();
  assert.equal(connection.calls, 1);
  assert.equal(frames.at(-1).status, 'stopped');
  await controller.handle(start);
  connection.emit('streamEnd', { action: 1 });
  connection.emit('disconnected', { code: 1000 });
  t.mock.timers.tick(30000); await flush();
  assert.equal(connection.calls, 2);
  assert.equal(frames.at(-1).status, 'stopped');
});

test('the production streamEnd event spelling stops recovery', async t => {
  const { connection, frames } = fixture(t);
  const controller = new Controller(() => connection, { ...events, webcast: { streamEnd: 'streamEnd' } }, f => frames.push(f));
  await controller.handle(start);
  connection.emit('streamEnd', { action: 1 });
  connection.isConnected = false;
  connection.emit('disconnected', { code: 1000 });
  t.mock.timers.tick(30000); await flush();
  assert.equal(connection.calls, 1);
  assert.equal(frames.at(-1).status, 'stopped');
});

test('reconnect errors cannot launch overlapping attempts and late sockets close after stop', async t => {
  const { connection, frames, controller } = fixture(t);
  await controller.handle(start);
  let resolve;
  connection.connect = () => { connection.calls++; return new Promise(r => { resolve = r; }); };
  connection.isConnected = false;
  connection.emit('disconnected', { code: 1006 });
  t.mock.timers.tick(1000); await flush();
  connection.emit('error', { exception: new Error('private') });
  connection.emit('disconnected', { code: 1006 });
  t.mock.timers.tick(30000); await flush();
  assert.equal(connection.calls, 2);
  await controller.handle({ protocolVersion: 1, type: 'stop', requestId: 'stop' });
  connection.isConnected = true;
  resolve({ isConnected: true }); await flush();
  assert.equal(connection.isConnected, false);
  assert.equal(frames.at(-1).status, 'stopped');
});

test('retry budget is bounded, sanitized, and permits a later manual start', async t => {
  const { connection, frames, controller } = fixture(t);
  await controller.handle(start);
  connection.connect = async () => { connection.calls++; throw new Error('private-response'); };
  connection.isConnected = false;
  connection.emit('disconnected', { code: 1006 });
  for (let attempt = 0; attempt < 6; attempt++) { t.mock.timers.tick(30000); await flush(); }
  assert.equal(connection.calls, 6);
  assert.equal(frames.at(-1).code, 'LIVE_RECONNECT_EXHAUSTED');
  assert.ok(!JSON.stringify(frames).includes('private-response'));
  connection.connect = async () => ({ isConnected: true });
  await controller.handle(start);
  assert.equal(frames.at(-1).status, 'connected');
});
