// Read public covers only from an explicitly supplied app-owned QA profile.
import fs from 'node:fs';
const [profile, account] = process.argv.slice(2);
if (!profile || account !== 'https://www.tiktok.com/@oclon2026') throw Error('Explicit QA account/profile required');
const [port, browserPath] = fs.readFileSync(profile + '/DevToolsActivePort', 'utf8').trim().split(/\r?\n/);
const origin = 'http://127.0.0.1:' + Number(port);
const version = await (await fetch(origin + '/json/version')).json();
if (new URL(version.webSocketDebuggerUrl).pathname !== browserPath) throw Error('Browser ownership mismatch');
const pages = await (await fetch(origin + '/json/list')).json();
const page = pages.find(p => p.type === 'page' && p.url === account);
if (!page) throw Error('QA account tab not found');
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
const expression = `JSON.stringify(Array.from(document.querySelectorAll('a[href*="/video/"]'))
  .filter(a => a.href.startsWith('https://www.tiktok.com/@oclon2026/video/'))
  .map(a => ({url:a.href,thumbnail:a.querySelector('img')?.currentSrc || a.querySelector('img')?.src || ''}))
  .filter(a => a.thumbnail.startsWith('https://')).slice(0,10))`;
const result = await new Promise((resolve, reject) => {
  const timer = setTimeout(() => reject(Error('QA account timeout')), 5000);
  ws.onmessage = e => {
    const message = JSON.parse(e.data);
    if (message.id !== 1) return;
    clearTimeout(timer);
    if (message.result?.exceptionDetails) reject(Error('Public cover expression failed'));
    else resolve(message.result?.result?.value);
  };
  ws.send(JSON.stringify({id:1,method:'Runtime.evaluate',params:{expression,returnByValue:true}}));
});
ws.close();
if (!result || JSON.parse(result).length !== 10) throw Error('Ten public covers not available');
console.log(result);
