// Human sign-in in a normal installed browser. No CDP, page inspection, input
// automation, credential access, or security-setting changes during sign-in.
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import { readFile, stat } from 'node:fs/promises';
import { connect } from 'node:net';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { selectBrowser } from './browser-cdp.mjs';
import { acquireBrowserProfile } from './browser-profile-lock.mjs';

// One fixed migration from the legacy CDP sign-in profile, not a new profile on
// each failure. The old browser-owned session is preserved and never copied.
export function aiBrowserProfile(root, kind, provider) {
  if (!path.isAbsolute(root) || !['chrome', 'edge'].includes(kind) || !['chatgpt', 'gemini', 'claude'].includes(provider)) throw new Error('Invalid AI browser profile');
  return path.join(root, 'interactive-v1', kind, provider);
}

export async function aiWorkBrowserProfile(root, kind, provider) {
  const preferred = aiBrowserProfile(root, kind, provider);
  try {
    if (!(await stat(preferred)).isDirectory()) throw new Error('Invalid AI browser profile directory');
    return preferred;
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
    // Existing working sessions remain in use until the user opens the new
    // manual sign-in flow for this specific provider.
    return path.join(root, kind, provider);
  }
}

export async function hasControlledBrowser(profile) {
  const metadata = await readFile(path.join(profile, 'DevToolsActivePort'), 'utf8').catch(error => {
    if (error.code === 'ENOENT') return ''; throw error;
  });
  if (metadata.length > 1024) return true;
  const [port, endpoint] = metadata.trim().split(/\r?\n/);
  if (!/^\d+$/.test(port || '') || Number(port) < 1 || Number(port) > 65535 || !/^\/devtools\/browser\/[a-f0-9-]+$/i.test(endpoint || '')) return false;
  // Liveness only: no CDP connection, page inspection, or browser command.
  return new Promise(resolve => {
    const socket = connect({ host: '127.0.0.1', port: Number(port) });
    const finish = live => { socket.destroy(); resolve(live); };
    socket.once('connect', () => finish(true));
    socket.once('error', error => finish(error.code !== 'ECONNREFUSED'));
    socket.setTimeout(300, () => finish(true));
  });
}

export function interactiveLoginArguments(profile, url) {
  if (!path.isAbsolute(profile) || !['https://chatgpt.com/', 'https://gemini.google.com/app', 'https://claude.ai/new'].includes(url)) throw new Error('Invalid login configuration');
  // Background extensions must not keep the sign-in owner/lease alive after
  // the user closes its final window. This changes process lifetime only.
  return [`--user-data-dir=${profile}`, '--no-first-run', '--no-default-browser-check', '--disable-background-mode', '--new-window', url];
}
export function isManualLoginCommand(name, commandLine, profile) {
  if (name?.toLowerCase() !== 'node.exe' || typeof commandLine !== 'string') return false;
  const args = [...commandLine.matchAll(/"([^"]*)"|([^\s"]+)/g)].map(match => match[1] ?? match[2]);
  try { return args.length === 5 && args[4] === '--login'
    && path.resolve(args[1]) === fileURLToPath(new URL('./control-web.mjs', import.meta.url))
    && aiBrowserProfile(path.resolve(args[2]), path.basename(path.dirname(profile)), args[3]) === profile;
  } catch { return false; }
}
function processAlive(pid) {
  try { process.kill(pid, 0); return true; } catch (error) { return error.code !== 'ESRCH'; }
}
async function manualLoginOwner(pid, profile) {
  if (process.platform !== 'win32' || !Number.isSafeInteger(pid) || pid < 1) return false;
  try {
    // Read only the exact lease owner's process metadata, never browser pages.
    const { stdout } = await promisify(execFile)('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
      `Get-CimInstance Win32_Process -Filter 'ProcessId=${pid}' | Select-Object Name,CommandLine | ConvertTo-Json -Compress`],
      { windowsHide: true, timeout: 5000, maxBuffer: 65536 });
    const owner = JSON.parse(stdout);
    return isManualLoginCommand(owner.Name, owner.CommandLine, profile);
  } catch { return false; }
}
async function reopenManualLogin(profile, { args, selection, notify, spawnProcess, readyDelayMs, verifyManualOwner, pollMs }) {
  const leaseFile = path.join(profile, 'concat-browser-owner.json');
  const readOwner = () => readFile(leaseFile, 'utf8').then(JSON.parse).catch(() => null);
  const owner = await readOwner();
  if (!owner || typeof owner.token !== 'string' || !await verifyManualOwner(owner.pid, profile)
      || await hasControlledBrowser(profile)) throw Object.assign(new Error('Profile is not a verified manual login'), { code: 'PROFILE_BUSY' });
  const current = await readOwner();
  if (current?.token !== owner.token || current?.pid !== owner.pid) throw Object.assign(new Error('Profile owner changed'), { code: 'PROFILE_BUSY' });
  // A second normal Chrome launch forwards --new-window to the existing manual
  // process. It does not acquire/release its lease, inspect pages or send prompts.
  const child = spawnProcess(selection.executable, args, { windowsHide: false, stdio: 'ignore' });
  const ended = new Promise(resolve => {
    child.once('error', error => resolve({ error }));
    child.once('exit', (code, signal) => resolve({ code, signal }));
  });
  const early = await Promise.race([ended, delay(readyDelayMs).then(() => null)]);
  if (early?.error || early?.signal || (early && early.code !== 0)) throw Object.assign(new Error('Could not reopen browser'), { code: 'BROWSER_LAUNCH_FAILED' });
  notify({ kind: 'login_ready' });
  while (true) {
    const active = await readOwner();
    if (active?.token !== owner.token || active?.pid !== owner.pid || !processAlive(owner.pid)) break;
    await delay(pollMs);
  }
  notify({ kind: 'login_closed' });
}
export async function runInteractiveLogin(profile, { url, selection = selectBrowser(), notify = () => {}, spawnProcess = spawn, readyDelayMs = 1000, verifyManualOwner = manualLoginOwner, pollMs = 1000 } = {}) {
  if (!selection?.executable) throw Object.assign(new Error('Browser missing'), { code: 'BROWSER_MISSING' });
  const args = interactiveLoginArguments(profile, url);
  let release;
  try { release = await acquireBrowserProfile(profile); }
  catch (error) {
    if (error.code !== 'PROFILE_BUSY') throw error;
    return reopenManualLogin(profile, { args, selection, notify, spawnProcess, readyDelayMs, verifyManualOwner, pollMs });
  }
  try {
    if (await hasControlledBrowser(profile)) throw Object.assign(new Error('Close the existing controlled browser before signing in'), { code: 'PROFILE_BUSY' });
    const child = spawnProcess(selection.executable, args, { windowsHide: false, stdio: 'ignore' });
    const ended = new Promise(resolve => {
      child.once('error', error => resolve({ error }));
      child.once('exit', (code, signal) => resolve({ code, signal }));
    });
    const early = await Promise.race([ended, delay(readyDelayMs).then(() => null)]);
    if (early) throw Object.assign(new Error('Browser did not open a new owned window'), { code: early.error || early.code !== 0 ? 'BROWSER_LAUNCH_FAILED' : 'PROFILE_BUSY' });
    notify({ kind: 'login_ready' });
    const result = await ended;
    if (result.error || result.signal || result.code !== 0) throw Object.assign(new Error('Login window closed unexpectedly'), { code: 'LOGIN_CLOSED_UNEXPECTEDLY' });
    // Closing a browser is not proof that authentication succeeded.
    notify({ kind: 'login_closed' });
  } finally { await release(); }
}
