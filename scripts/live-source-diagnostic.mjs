// Prints only structural error codes, never messages, cookies or response bodies.
import { pathToFileURL } from 'node:url';
import { resolve } from 'node:path';
const { TikTokLiveConnection } = await import(pathToFileURL(resolve('.deps/tiktok-live-connector/dist/index.js')).href);
const username = process.argv[2]?.replace(/^@/, '');
if (!username || !/^[\w.]{1,24}$/.test(username)) throw new Error('Supply a public username');
const safe = (error, depth = 0) => depth > 3 ? {} : ({
  class: error?.constructor?.name,
  code: /^[A-Z0-9_]{1,64}$/.test(error?.code || '') ? error.code : undefined,
  status: error?.response?.status || error?.response?.statusCode,
  route: error?.config?.routeId,
  causes: error?.config?.requestErrs?.slice(0, 5).map(e => safe(e, depth + 1)),
  cause: error?.cause ? safe(error.cause, depth + 1) : undefined,
});
const connection = new TikTokLiveConnection(username, { processInitialData: true, fetchRoomInfoOnConnect: true, enableExtendedGiftInfo: true });
connection.on('error', () => {});
const deadline = setTimeout(() => { console.log('DIAGNOSTIC=TIMEOUT'); process.exit(1); }, 55000);
try { const state = await connection.connect(); console.log(JSON.stringify({ connected: state.isConnected })); }
catch (error) { console.log(JSON.stringify(safe(error))); process.exitCode = 1; }
finally { clearTimeout(deadline); await connection.disconnect(); }
