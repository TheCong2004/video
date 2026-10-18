// SPDX-License-Identifier: AGPL-3.0-or-later
// CDP is used only for the dedicated browser process launched by Concat.
import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdir, mkdtemp, readFile, rm, unlink } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

export function selectBrowser(env = process.env, available = existsSync) {
  const candidates = [
    [env.ProgramFiles, 'Google/Chrome/Application/chrome.exe', 'chrome'],
    [env['ProgramFiles(x86)'], 'Google/Chrome/Application/chrome.exe', 'chrome'],
    [env.LOCALAPPDATA, 'Google/Chrome/Application/chrome.exe', 'chrome'],
    [env['ProgramFiles(x86)'], 'Microsoft/Edge/Application/msedge.exe', 'edge'],
    [env.ProgramFiles, 'Microsoft/Edge/Application/msedge.exe', 'edge'],
  ];
  return candidates.map(([root, file, kind]) => root && ({ executable: path.join(root, file), kind }))
    .find(browser => browser && available(browser.executable));
}

export function browserExecutable() { return selectBrowser()?.executable; }

export class Cdp {
  constructor(socket) {
    this.socket = socket;
    this.sequence = 0;
    this.pending = new Map();
    this.listeners = new Map();
    socket.addEventListener('message', event => {
      const message = JSON.parse(event.data);
      if (message.id) {
        const pending = this.pending.get(message.id);
        if (!pending) return;
        this.pending.delete(message.id);
        clearTimeout(pending.timer);
        if (message.error) pending.reject(new Error('Browser command failed.'));
        else pending.resolve(message.result);
      } else {
        for (const listener of this.listeners.get(message.method) || []) listener(message.params, message.sessionId);
      }
    });
    socket.addEventListener('close', () => this.fail());
    socket.addEventListener('error', () => this.fail());
  }
  fail() {
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(new Error('Browser connection closed.'));
    }
    this.pending.clear();
  }
  static async connect(url) {
    const socket = new WebSocket(url);
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => { socket.close(); reject(new Error('Browser connection timed out.')); }, 8000);
      socket.addEventListener('open', () => { clearTimeout(timer); resolve(); }, { once: true });
      socket.addEventListener('error', () => { clearTimeout(timer); reject(new Error('Cannot connect to browser.')); }, { once: true });
    });
    return new Cdp(socket);
  }
  on(method, listener) {
    const listeners = this.listeners.get(method) || [];
    listeners.push(listener);
    this.listeners.set(method, listeners);
  }
  send(method, params = {}, sessionId) {
    if (this.socket.readyState !== WebSocket.OPEN) return Promise.reject(new Error('Browser connection closed.'));
    const id = ++this.sequence;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.pending.delete(id); reject(new Error('Browser command timed out.')); }, 10000);
      this.pending.set(id, { resolve, reject, timer });
      this.socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
  }
  close() { this.socket.close(); this.fail(); }
}

export async function ensureChatTarget(cdp) {
  const { targetInfos } = await cdp.send('Target.getTargets');
  // Only this process's dedicated profile is reachable here. Edge app mode can
  // expose no page yet, or a startup URL other than about:blank. Do not rely on
  // that bootstrap page existing: create our own target when none is present.
  const existing = targetInfos.find(target => target.type === 'page');
  if (existing) return existing.targetId;
  const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank', newWindow: true });
  if (!targetId) throw new Error('Không tạo được trang chat của Concat.');
  return targetId;
}

export async function showBrowser(browser) {
  const { cdp, targetId, sessionId } = browser;
  const { windowId } = await cdp.send('Browser.getWindowForTarget', { targetId });
  await cdp.send('Browser.setWindowBounds', { windowId, bounds: { windowState: 'normal' } });
  await cdp.send('Page.bringToFront', {}, sessionId);
}

export async function launchBrowser(profile, { headless = false, selection = selectBrowser() } = {}) {
  const executable = selection?.executable;
  if (!executable) throw new Error('Cần Microsoft Edge hoặc Google Chrome trên máy.');
  await mkdir(profile, { recursive: true });
  const portFile = path.join(profile, 'DevToolsActivePort');
  // Only remove this own browser metadata file, never profile/session data.
  await unlink(portFile).catch(error => { if (error.code !== 'ENOENT') throw error; });
  const scratch = await mkdtemp(path.join(profile, 'concat-browser-temp-'));
  const child = spawn(executable, [
    `--user-data-dir=${profile}`, '--remote-debugging-address=127.0.0.1', '--remote-debugging-port=0',
    '--no-first-run', '--no-default-browser-check', '--window-size=1080,850',
    // Keep normal browser chrome available for first-login/profile dialogs.
    // App mode can leave the controlled AI page hidden behind a blank shell.
    ...(headless ? ['--headless=new', 'about:blank'] : ['--new-window', 'about:blank']),
  ], { windowsHide: headless, stdio: 'ignore', env: { ...process.env, TMP: scratch, TEMP: scratch } });
  let failed = false;
  child.once('error', () => { failed = true; });
  let cdp;
  async function cleanup() {
    for (let attempt = 0; child.exitCode === null && attempt < 30; attempt++) await delay(100);
    if (child.exitCode === null) child.kill();
    // Only this exact mkdtemp browser scratch; persistent login/profile stays.
    for (let attempt = 0; attempt < 20; attempt++) {
      try { await rm(scratch, { recursive: true, force: true }); break; }
      catch { await delay(150); }
    }
  }
  try {
    for (let attempt = 0; attempt < 100; attempt++) {
      if (failed || child.exitCode !== null) throw new Error('Hãy đóng cửa sổ chat Concat cũ rồi kết nối lại.');
      const content = await readFile(portFile, 'utf8').catch(() => '');
      const [port, endpoint] = content.trim().split(/\r?\n/);
      if (/^\d+$/.test(port || '') && Number(port) > 0 && Number(port) < 65536 && /^\/devtools\/browser\/[a-f0-9-]+$/i.test(endpoint || '')) {
        cdp = await Cdp.connect(`ws://127.0.0.1:${port}${endpoint}`);
        break;
      }
      await delay(150);
    }
    if (!cdp) throw new Error('Không kết nối được cửa sổ chat. Đóng cửa sổ chat Concat cũ rồi thử lại.');
    const targetId = await ensureChatTarget(cdp);
    const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
    await cdp.send('Page.enable', {}, sessionId);
    await cdp.send('Runtime.enable', {}, sessionId);
    return {
      cdp, sessionId, targetId, browserKind: selection.kind,
      async close() {
        await cdp.send('Browser.close').catch(() => {});
        cdp.close();
        await cleanup();
      },
    };
  } catch (error) {
    await cdp?.send('Browser.close').catch(() => {});
    cdp?.close();
    if (child.exitCode === null) child.kill();
    await cleanup();
    throw error;
  }
}
