// Manual visual regression probe: own temporary profile, no AI/network request.
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { launchBrowser } from './browser-cdp.mjs';
const root = fileURLToPath(new URL('../../.tmp/', import.meta.url));
await mkdir(root, { recursive: true });
const profile = await mkdtemp(path.join(root, 'visible-browser-'));
let browser;
try {
  browser = await launchBrowser(profile);
  await browser.cdp.send('Page.navigate', { url: 'data:text/html,<title>Concat visible browser probe</title><h1>Concat Chrome display test</h1><p>No AI request. This window closes automatically.</p>' }, browser.sessionId);
  await browser.cdp.send('Page.bringToFront', {}, browser.sessionId);
  console.log('VISIBLE_WINDOW_PROBE_STARTED');
  await delay(30000);
} finally {
  await browser?.close();
  await rm(profile, { recursive: true, force: true });
}
