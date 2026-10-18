// Read-only adapter diagnostics: no prompts, conversation text, cookies or keys.
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { Cdp } from './browser-cdp.mjs';
const [port, endpoint] = (await readFile(path.join(process.argv[2], 'DevToolsActivePort'), 'utf8')).trim().split(/\r?\n/);
if (!/^\d+$/.test(port) || !/^\/devtools\/browser\/[a-f0-9-]+$/i.test(endpoint)) throw new Error('Invalid owned browser endpoint');
const cdp = await Cdp.connect(`ws://127.0.0.1:${port}${endpoint}`);
try {
  const { targetInfos } = await cdp.send('Target.getTargets');
  if (process.argv[3] === '--startup') {
    for (const target of targetInfos.filter(target => target.type === 'page')) {
      const { sessionId } = await cdp.send('Target.attachToTarget', { targetId: target.targetId, flatten: true });
      const result = await cdp.send('Runtime.evaluate', { returnByValue: true, expression: `(() => ({
        origin:location.origin,path:location.pathname,ready:document.readyState,
        visibility:document.visibilityState,focused:document.hasFocus(),size:[innerWidth,innerHeight],
        installed:!!window.__concatChatInstalled,adapterReady:!!window.__concatChatReady,
        badge:document.querySelector('#concat-chat-status')?.innerText,
        signInRequired:/sign in|đăng nhập/i.test(document.body?.innerText || ''),
        unsupported:/not supported|unsupported|không được hỗ trợ/i.test(document.body?.innerText || ''),
        inputCount:document.querySelectorAll('textarea,[contenteditable=true]').length,
        bodyLength:document.body?.innerText.length
      }))()` }, sessionId);
      console.log(JSON.stringify(result.result?.value || { error: result.exceptionDetails?.text }));
      await cdp.send('Target.detachFromTarget', { sessionId });
    }
  }
  for (const target of targetInfos.filter(target => target.type === 'page' && target.url.startsWith('https://chatgpt.com/'))) {
    const { sessionId } = await cdp.send('Target.attachToTarget', { targetId: target.targetId, flatten: true });
    const expression = process.argv[3] === '--actions' ? `(() => {
      const describe=element=>({tag:element.tagName,attrs:[...element.attributes].filter(attr=>attr.name!=='class'&&attr.name!=='style').map(attr=>[attr.name,attr.value.slice(0,100)]),children:[...element.children].map(child=>child.tagName).slice(0,15)});
      return [...document.querySelectorAll('main button[aria-label]')].filter(button=>/copy|response|read aloud|sao chép|đọc to/i.test(button.getAttribute('aria-label'))).slice(0,12).map(button=>({button:describe(button),parents:[button.parentElement,button.parentElement?.parentElement,button.parentElement?.parentElement?.parentElement,button.parentElement?.parentElement?.parentElement?.parentElement].filter(Boolean).map(describe)}));
    })()` : process.argv[3] === '--structure' ? `(() => {
      const describe=element=>({tag:element.tagName,testid:element.getAttribute('data-testid'),role:element.getAttribute('role'),classes:element.className,author:element.getAttribute('data-message-author-role'),parent:element.parentElement?.className,children:[...element.children].map(child=>({tag:child.tagName,testid:child.getAttribute('data-testid'),classes:child.className})).slice(0,8)});
      const nodes=[...document.querySelectorAll('main p,main section,main article,main h1,main h2,main pre,main code')].slice(0,20).map(describe);
      const tree=element=>({tag:element.tagName,classes:element.className,children:[...element.children].slice(0,8).map(child=>({tag:child.tagName,classes:child.className,children:[...child.children].slice(0,5).map(describe)}))});
      return {origin:location.origin,path:location.pathname,messages:[...document.querySelectorAll('article,[role="article"],[data-message-id],[data-message-author-role],.markdown,.prose,[data-testid*="message"],[data-testid*="turn"]')].slice(0,30).map(describe),nodes,main:[...document.querySelectorAll('main')].map(tree),testids:[...new Set([...document.querySelectorAll('[data-testid]')].map(element=>element.getAttribute('data-testid')))].filter(id=>/message|turn|markdown|response|assistant/.test(id))};
    })()` : `(() => {
      const attributes = element => ({tag:element.tagName,id:element.id,testid:element.getAttribute('data-testid'),role:element.getAttribute('role'),label:element.getAttribute('aria-label'),type:element.getAttribute('type'),editable:element.getAttribute('contenteditable'),visible:!!element.getClientRects().length});
      const inputs=[...document.querySelectorAll('[contenteditable=true],textarea')];
      const forms=[...new Set(inputs.map(input=>input.closest('form')).filter(Boolean))];
      return {origin:location.origin,ready:document.readyState,installed:!!window.__concatChatInstalled,binding:typeof window.__concatHost,deliver:typeof window.__concatDeliver,badge:document.querySelector('#concat-chat-status')?.innerText,assistantContainers:document.querySelectorAll('[data-message-author-role="assistant"]').length,inputs:inputs.map(attributes),buttons:forms.flatMap(form=>[...form.querySelectorAll('button')].map(attributes))};
    })()`;
    const result = await cdp.send('Runtime.evaluate', { returnByValue: true, expression }, sessionId);
    console.log(JSON.stringify(result.result?.value || { error: result.exceptionDetails?.text }, null, 2));
    await cdp.send('Target.detachFromTarget', { sessionId });
  }
} finally { cdp.close(); }
