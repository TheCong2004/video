// Read-only connection probe: never sends chat, invokes AI, or prints vendor payloads.
import { spawn } from 'node:child_process';
import { resolve } from 'node:path';
const username = process.argv[2]?.replace(/^@/, '');
if (!username || !/^[\w.]{1,24}$/.test(username)) throw new Error('Supply a public TikTok username');
const entry = resolve(process.argv[3] || '.deps/tiktok-live-connector/dist/artcraftLiveRuntime.js');
const child = spawn(process.execPath, [entry], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
let buffer = '', connected = false, settled = false;
const stop = () => {
  if (settled) return;
  settled = true;
  child.stdin.write(JSON.stringify({ protocolVersion: 1, type: 'shutdown', requestId: 'probe-stop' }) + '\n');
  setTimeout(() => child.kill(), 1500).unref();
};
const deadline = setTimeout(() => { console.log('PROBE=TIMEOUT'); stop(); }, 55000);
child.stdout.on('data', chunk => {
  buffer += chunk;
  let index;
  while ((index = buffer.indexOf('\n')) >= 0) {
    const line = buffer.slice(0, index); buffer = buffer.slice(index + 1);
    let frame;
    try { frame = JSON.parse(line); } catch { console.log('NON_PROTOCOL_LINE'); continue; }
    if (frame.type === 'runtime.ready') {
      console.log('READY=' + frame.runtimeVersion);
      child.stdin.write(JSON.stringify({ protocolVersion: 1, type: 'start', requestId: 'probe-start', username }) + '\n');
    }
    if (['runtime.status', 'command.error'].includes(frame.type)) {
      console.log(JSON.stringify({ type: frame.type, status: frame.status, code: /^[A-Z0-9_]{1,80}$/.test(frame.code || '') ? frame.code : undefined }));
      if (frame.status === 'connected') connected = true;
      if (['connected', 'failed'].includes(frame.status) || frame.type === 'command.error') stop();
    }
  }
});
child.stderr.resume();
child.on('error', () => { console.log('PROBE=SPAWN_FAILED'); });
child.on('exit', () => { clearTimeout(deadline); console.log(connected ? 'PROBE=CONNECTED' : 'PROBE=NOT_CONNECTED'); process.exitCode = connected ? 0 : 1; });
