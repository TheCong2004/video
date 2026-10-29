// No AI/TTS calls, outbound comments, credentials or comment contents in output.
import { spawn } from 'node:child_process';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
const username = (process.argv[2] || '').replace(/^@/, '');
if (!/^[a-zA-Z0-9_.]{1,64}$/.test(username)) throw new Error('Provide TikTok username');
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const child = spawn(process.execPath, ['dist/artcraftLiveRuntime.js'], {
  cwd: resolve(root, '.deps/tiktok-live-connector'), stdio: ['pipe', 'pipe', 'pipe'],
});
const send = command => { if (!child.stdin.destroyed) child.stdin.write(JSON.stringify({ protocolVersion: 1, ...command }) + '\n'); };
let buffer = '', connected = false;
const counts = {};
const stop = setTimeout(() => send({ type: 'shutdown', requestId: 'watch-stop' }), 45000);
const deadline = setTimeout(() => child.kill(), 50000);
child.stdout.on('data', data => {
  buffer += data;
  let index;
  while ((index = buffer.indexOf('\n')) >= 0) {
    const line = buffer.slice(0, index); buffer = buffer.slice(index + 1);
    const frame = JSON.parse(line);
    if (frame.type === 'runtime.ready') send({ type: 'start', requestId: 'watch-start', username });
    else if (frame.type === 'runtime.status' || frame.type === 'runtime.warning') {
      console.log(JSON.stringify({ type: frame.type, status: frame.status, code: frame.code }));
      if (frame.status === 'connected') connected = true;
    } else if (frame.type === 'live.event') counts[frame.eventKind] = (counts[frame.eventKind] || 0) + 1;
  }
});
child.stderr.resume();
child.stdin.on('error', () => {});
child.on('error', () => { process.exitCode = 1; });
child.on('exit', code => {
  clearTimeout(stop); clearTimeout(deadline);
  console.log(JSON.stringify({ exit: code, connected, eventCounts: counts }));
  if (!connected || code !== 0) process.exitCode = 1;
});
