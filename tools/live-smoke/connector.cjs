// Explicit GUI bridge fixture, not a TikTok implementation or production runtime.
const input = require('node:readline').createInterface({ input: process.stdin });
const send = value => process.stdout.write(JSON.stringify({ protocolVersion: 1, ...value }) + '\n');
let generation = 0;
send({ type: 'runtime.ready', runtime: 'artcraft-tiktok-live', runtimeVersion: 'gui-fixture', nodeVersion: process.version });
input.on('line', line => {
  const command = JSON.parse(line);
  if (command.type === 'start') {
    const current = ++generation;
    send({ type: 'runtime.status', status: 'connecting', requestId: command.requestId });
    // Longer than multiple 80ms monitor iterations: old cross-thread Weak check
    // made the UI stop listening before this valid connected frame arrived.
    setTimeout(() => {
      if (current === generation) send({ type: 'runtime.status', status: 'connected', requestId: command.requestId });
    }, 600);
  } else if (command.type === 'stop') {
    ++generation;
    setTimeout(() => send({ type: 'runtime.status', status: 'stopped', requestId: command.requestId }), 100);
  } else if (command.type === 'shutdown') {
    process.exit(0);
  } else {
    send({ type: 'command.error', code: 'FIXTURE_UNSUPPORTED_COMMAND' });
  }
});
input.on('close', () => process.exit(0));
