import test from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { createServer } from 'node:net';
import { mkdir, mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { fileURLToPath } from 'node:url';
import { aiBrowserProfile, aiWorkBrowserProfile, interactiveLoginArguments, isManualLoginCommand, runInteractiveLogin } from './browser-login.mjs';
import { acquireBrowserProfile } from './browser-profile-lock.mjs';
import { launchBrowser } from './browser-cdp.mjs';

async function fixture(run) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'concat-interactive-login-'));
  try { await run(root); } finally { await rm(root, { recursive: true, force: true }); }
}
const selection = { executable: process.execPath, kind: 'chrome' };
test('only the exact manual login process and matching provider profile can be reopened', () => {
  const root = path.resolve('fixture-root');
  const script = fileURLToPath(new URL('./control-web.mjs', import.meta.url));
  const profile = aiBrowserProfile(root, 'chrome', 'gemini');
  const command = `"node.exe" "${script}" "${root}" gemini --login`;
  assert.equal(isManualLoginCommand('node.exe', command, profile), true);
  assert.equal(isManualLoginCommand('node.exe', command.replace('--login', '--run'), profile), false);
  assert.equal(isManualLoginCommand('node.exe', command, aiBrowserProfile(root, 'chrome', 'claude')), false);
  assert.equal(isManualLoginCommand('chrome.exe', command, profile), false);
  assert.equal(isManualLoginCommand('node.exe', command + ' extra', profile), false);
});
test('repeated login reopens the verified manual browser and preserves its exclusive lease', async () => fixture(async root => {
  const release = await acquireBrowserProfile(root);
  const before = await readFile(path.join(root, 'concat-browser-owner.json'), 'utf8');
  const events = [];
  const login = runInteractiveLogin(root, { selection, url: 'https://gemini.google.com/app', readyDelayMs: 1, pollMs: 2,
    verifyManualOwner: async pid => pid === process.pid,
    spawnProcess: () => { const child = new EventEmitter(); queueMicrotask(() => child.emit('exit', 0, null)); return child; },
    notify: event => events.push(event.kind) });
  for (let i = 0; !events.length && i < 200; i++) await new Promise(resolve => setTimeout(resolve, 5));
  assert.deepEqual(events, ['login_ready']);
  assert.equal(await readFile(path.join(root, 'concat-browser-owner.json'), 'utf8'), before);
  await assert.rejects(launchBrowser(root, { selection }), { code: 'PROFILE_BUSY' });
  await release(); await login;
  assert.deepEqual(events, ['login_ready', 'login_closed']);
}));
test('a busy automation or unverifiable owner is never reopened or unlocked', async () => fixture(async root => {
  const release = await acquireBrowserProfile(root);
  try {
    await assert.rejects(runInteractiveLogin(root, { selection, url: 'https://gemini.google.com/app', verifyManualOwner: async () => false,
      spawnProcess: () => assert.fail('do not reopen an automation owner') }), { code: 'PROFILE_BUSY' });
    await assert.rejects(acquireBrowserProfile(root), { code: 'PROFILE_BUSY' });
  } finally { await release(); }
}));
test('even a manual owner cannot reopen while a controlled browser listener is live', async () => fixture(async root => {
  const release = await acquireBrowserProfile(root);
  const server = createServer(socket => { socket.on('data', () => assert.fail('no browser commands')); });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  try {
    await writeFile(path.join(root, 'DevToolsActivePort'), `${server.address().port}\n/devtools/browser/12345678-abcd-1234-abcd-123456789012`);
    await assert.rejects(runInteractiveLogin(root, { selection, url: 'https://gemini.google.com/app', verifyManualOwner: async () => true,
      spawnProcess: () => assert.fail('never reopen a controlled profile') }), { code: 'PROFILE_BUSY' });
  } finally { await release(); await new Promise(resolve => server.close(resolve)); }
}));
test('login and work use one stable profile isolated from the legacy controlled sign-in', () => {
  const root = path.resolve('fixture-root');
  const profile = aiBrowserProfile(root, 'chrome', 'gemini');
  assert.equal(profile, path.join(root, 'interactive-v1', 'chrome', 'gemini'));
  assert.equal(aiBrowserProfile(root, 'chrome', 'gemini'), profile);
  assert.notEqual(profile, path.join(root, 'chrome', 'gemini'));
  assert.throws(() => aiBrowserProfile(root, '../escape', 'gemini'));
});
test('working sessions migrate only for the provider whose manual sign-in was opened', async () => fixture(async root => {
  assert.equal(await aiWorkBrowserProfile(root, 'chrome', 'chatgpt'), path.join(root, 'chrome', 'chatgpt'));
  const normal = aiBrowserProfile(root, 'chrome', 'gemini');
  await mkdir(normal, { recursive: true });
  assert.equal(await aiWorkBrowserProfile(root, 'chrome', 'gemini'), normal);
  assert.equal(await aiWorkBrowserProfile(root, 'chrome', 'chatgpt'), path.join(root, 'chrome', 'chatgpt'));
}));
test('an orphaned controlled profile is refused before launching into its auth window', async () => fixture(async root => {
  const server = createServer(socket => { socket.on('data', () => assert.fail('no browser commands during sign-in preflight')); });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  try {
    await writeFile(path.join(root, 'DevToolsActivePort'), `${server.address().port}\n/devtools/browser/12345678-abcd-1234-abcd-123456789012`);
    await assert.rejects(runInteractiveLogin(root, { selection, url: 'https://gemini.google.com/app', spawnProcess: () => assert.fail('must not reuse the controlled browser') }), { code: 'PROFILE_BUSY' });
  } finally { await new Promise(resolve => server.close(resolve)); }
}));
test('manual sign-in has normal browser chrome and no automation or remote-control flags', () => {
  const profile = path.resolve('fixture-profile');
  for (const url of ['https://chatgpt.com/', 'https://gemini.google.com/app', 'https://claude.ai/new']) {
    const args = interactiveLoginArguments(profile, url);
    assert.ok(args.includes(`--user-data-dir=${profile}`));
    assert.ok(args.includes('--new-window'));
    assert.ok(args.includes('--disable-background-mode'), 'closing the final login window must not leave background apps holding the lease');
    assert.equal(args.at(-1), url);
    assert.ok(args.every(arg => !/remote-debug|automation|headless|disable.*security|user-agent|load-extension|blink-features/i.test(arg)));
  }
  assert.throws(() => interactiveLoginArguments('relative', 'https://gemini.google.com/app'));
  assert.throws(() => interactiveLoginArguments(profile, 'https://accounts.google.com/anything'));
});
test('normal sign-in emits ready and closed, retains profile data, and excludes controlled work', async () => fixture(async root => {
  const events = [];
  const child = new EventEmitter();
  child.kill = () => assert.fail('sign-in must never kill the user browser');
  await writeFile(path.join(root, 'profile-fixture'), 'browser-owned session fixture');
  const login = runInteractiveLogin(root, { selection, url: 'https://gemini.google.com/app', readyDelayMs: 1,
    spawnProcess: (_exe, _args, options) => { assert.equal(options.windowsHide, false); return child; },
    notify: event => events.push(event.kind) });
  for (let attempt = 0; !events.length && attempt < 400; attempt++) await new Promise(resolve => setTimeout(resolve, 5));
  if (!events.length) { child.emit('exit', 0, null); await login; assert.fail('login readiness timed out'); }
  assert.deepEqual(events, ['login_ready']);
  await assert.rejects(launchBrowser(root, { selection }), { code: 'PROFILE_BUSY' });
  child.emit('exit', 0, null);
  await login;
  assert.deepEqual(events, ['login_ready', 'login_closed']);
  assert.equal(await readFile(path.join(root, 'profile-fixture'), 'utf8'), 'browser-owned session fixture');
  const release = await acquireBrowserProfile(root); await release();
}));
test('a reused profile cannot be reported as an opened login window', async () => fixture(async root => {
  const events = [];
  await assert.rejects(runInteractiveLogin(root, { selection, url: 'https://chatgpt.com/', readyDelayMs: 30,
    notify: event => events.push(event.kind), spawnProcess: () => {
      const child = new EventEmitter(); queueMicrotask(() => child.emit('exit', 0, null)); return child;
    } }), { code: 'PROFILE_BUSY' });
  assert.deepEqual(events, []);
  const release = await acquireBrowserProfile(root); await release();
}));
test('spawn failures release the profile without claiming readiness', async () => fixture(async root => {
  await assert.rejects(runInteractiveLogin(root, { selection, url: 'https://claude.ai/new', readyDelayMs: 30,
    spawnProcess: () => { const child = new EventEmitter(); queueMicrotask(() => child.emit('error', new Error('fixture spawn failure'))); return child; } }), { code: 'BROWSER_LAUNCH_FAILED' });
  const release = await acquireBrowserProfile(root); await release();
}));
test('profile leases are exclusive and stale recovery has one winner', async () => fixture(async root => {
  const release = await acquireBrowserProfile(root);
  await assert.rejects(acquireBrowserProfile(root), { code: 'PROFILE_BUSY' });
  await release(); await release();
  await writeFile(path.join(root, 'concat-browser-owner.json'), JSON.stringify({ pid: 123456789, token: 'stale fixture' }));
  const results = await Promise.allSettled([acquireBrowserProfile(root, { isAlive: pid => pid === process.pid }), acquireBrowserProfile(root, { isAlive: pid => pid === process.pid })]);
  const owners = results.filter(result => result.status === 'fulfilled');
  assert.equal(owners.length, 1);
  await owners[0].value();
}));
test('unknown profile lock data fails closed and stays intact', async () => fixture(async root => {
  const file = path.join(root, 'concat-browser-owner.json');
  await writeFile(file, 'fixture invalid lock');
  await assert.rejects(acquireBrowserProfile(root), { code: 'PROFILE_BUSY' });
  assert.equal(await readFile(file, 'utf8'), 'fixture invalid lock');
}));
