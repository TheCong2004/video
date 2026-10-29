// SPDX-License-Identifier: AGPL-3.0-or-later
// One correlated model turn in an owned, visible browser. No private APIs.
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import { mkdir,writeFile,rename } from 'node:fs/promises';
import { setTimeout as delay } from 'node:timers/promises';
import { launchBrowser, selectBrowser, showBrowser } from './browser-cdp.mjs';
import { aiBrowserProfile, aiWorkBrowserProfile, runInteractiveLogin } from './browser-login.mjs';
import { PROVIDERS } from './web-chat.mjs';

export const SELECTORS = Object.freeze({
  chatgpt: {input:'#prompt-textarea, #mobile-composer-prompt, textarea[data-id="root"], textarea[aria-label="Chat with ChatGPT"]',send:'button[data-testid="send-button"], button[data-testid="composer-submit-button"], button[aria-label="Send prompt"], button[aria-label="Send message"]',reply:'[data-message-author-role="assistant"], [data-message-role="assistant"], [data-turn="assistant"]',stop:'button[data-testid="stop-button"], button[aria-label="Stop streaming"], button[aria-label="Stop response"]'},
  gemini: {input:'rich-textarea [contenteditable="true"]:not(.ql-clipboard):not([aria-hidden="true"]), .ql-editor[contenteditable="true"]:not(.ql-clipboard):not([aria-hidden="true"])',send:'button.send-button, button[data-test-id="send-button"], button[aria-label="Send message"], button[aria-label="Gửi tin nhắn"], button[aria-label="Gửi"]',reply:'model-response',stop:'button[aria-label="Stop response"], button[aria-label="Dừng phản hồi"]'},
  claude: {input:'[contenteditable="true"].ProseMirror, [contenteditable="true"][role="textbox"]',send:'button[aria-label="Send message"], button[aria-label="Send Message"]',reply:'.font-claude-response, [data-testid="assistant-message"]',stop:'button[aria-label="Stop response"], button[aria-label="Stop Response"]'},
});

export const AUTH_BROWSER_REJECTED='GOOGLE_AUTH_BROWSER_REJECTED';
export function isModelRefusal(text) {
  return typeof text==='string' && text.length<=4000
    && /^(?:tôi (?:không thể|chưa thể) (?:trợ giúp|giúp|hỗ trợ)|(?:i(?:'m| am) sorry[, ]+)?i (?:cannot|can't|am unable to) (?:help|assist)|as (?:an? )?(?:ai|language model)[, ])/iu.test(text.trim());
}
export function isRejectedAuthPage(raw) {
  try {
    const url=new URL(raw);
    return url.origin==='https://accounts.google.com'
      && /^\/(?:v\d+\/)?signin\/(?:rejected|denied)(?:\/|$)/.test(url.pathname);
  } catch {return false;}
}

export function composeControlRequest(prompt, requestId) {
  if (typeof prompt!=='string' || !prompt.trim() || Buffer.byteLength(prompt)>131072
    || !/^[a-zA-Z0-9_-]{1,128}$/.test(requestId)) throw new Error('Invalid control request');
  return `CONCAT_CONTROL_REQUEST_V1\nCorrelation ID: ${requestId}\nPerform the task below as a model response only. Concat executes validated steps separately; do not claim to have edited the project. Return exactly one JSON object with exactly two fields: {"requestId":"${requestId}","result":YOUR_TASK_OUTPUT}. For a JSON task, result is the complete JSON object. For a writing task, result is a string. Preserve the correlation ID exactly. Preserve JSON escaping in all template paths: Windows backslashes must be doubled in JSON strings. No prose outside this envelope.\n\nTASK:\n${prompt}\n\nRESPONSE FORMAT: Put the task output INSIDE result, even when the task requests JSON only. Return {"requestId":"${requestId}","result":YOUR_TASK_OUTPUT}.`;
}

export function templateWindowsPaths(prompt) {
  const paths=new Set();
  for(const match of prompt.matchAll(/"(?:\\.|[^"\\])*"/g)) {
    try {
      const value=JSON.parse(match[0]);
      if(/^[a-z]:\\/i.test(value) && value.length<=4096 && !/["\x00-\x1f]/.test(value)) paths.add(value);
    } catch {}
  }
  return [...paths].slice(0,128);
}
function parseTaskJson(text, allowedPaths=[]) {
  // Repair only an exact path already present in the native task template.
  // Do not invent paths, fix arbitrary JSON, or reinterpret narration escapes.
  let repaired=text;
  for(const value of allowedPaths) {
    const literal=('"'+value+'"').replace(/[.*+?^${}()|[\]\\]/g,'\\$&');
    repaired=repaired.replace(new RegExp('([:\\[,]\\s*)'+literal+'(?=\\s*[,}\\]])','g'),
      (_match,prefix)=>prefix+JSON.stringify(value));
  }
  return JSON.parse(repaired);
}

export function parseControlResponse(raw, requestId) {
  if (typeof raw!=='string' || Buffer.byteLength(raw)>262144) return null;
  let text=raw.trim();
  if (text.startsWith('```')) text=text.replace(/^```(?:json)?\s*/, '').replace(/\s*```$/, '');
  let value;
  try {value=JSON.parse(text);} catch {return null;}
  if (!value || Object.keys(value).sort().join(',')!=='requestId,result' || value.requestId!==requestId
    || !(typeof value.result==='string' || (value.result && typeof value.result==='object' && !Array.isArray(value.result)))) return null;
  const answer=typeof value.result==='string' ? value.result : JSON.stringify(value.result);
  return answer.trim() && Buffer.byteLength(answer)<=65536 ? answer : null;
}

// Extract complete JSON objects without evaluating model code. Code fences and
// provider copy/language labels may surround the actual envelope in rendered DOM.
function objects(raw, allowedPaths=[], recoverEnvelope=true) {
  const found=[];
  let start=-1,depth=0,quoted=false,escaped=false;
  for(let i=0;i<raw.length;i++) {
    const c=raw[i];
    if(start<0) {if(c==='{'){start=i;depth=1;}continue;}
    if(quoted) {if(escaped)escaped=false;else if(c==='\\')escaped=true;else if(c==='"')quoted=false;continue;}
    if(c==='"')quoted=true;
    else if(c==='{')depth++;
    else if(c==='}' && --depth===0) {
      try {found.push(parseTaskJson(raw.slice(start,i+1),allowedPaths));} catch {}
      start=-1;if(found.length>=16)break;
    }
  }
  // A rendered reply can contain a truncated first envelope, then a complete
  // replacement envelope inside the unterminated string. Restart only at exact
  // envelope boundaries if the ordinary scanner found NO valid JSON at all.
  // Nonce, schema, turn binding and conflicting-result checks still apply later.
  if(!found.length && recoverEnvelope) {
    let count=0;
    for(const match of raw.matchAll(/\{\s*"requestId"\s*:/g)) {
      if(++count>64)break;
      if(match.index===0)continue;
      found.push(...objects(raw.slice(match.index),allowedPaths,false));
      if(found.length>=16)break;
    }
  }
  return found;
}

export function readControlResponse(raw,requestId,{boundToTurn=false,complete=false,allowedPaths=[]}={}) {
  if(typeof raw!=='string' || Buffer.byteLength(raw)>262144)return null;
  const candidates=[...new Map(objects(raw,boundToTurn?allowedPaths:[]).map(value=>[JSON.stringify(value),value])).values()];
  const envelopes=candidates.filter(v=>Object.hasOwn(v,'requestId') || Object.hasOwn(v,'result'));
  if(envelopes.length) {
    // A conflicting/malformed ID never falls back to an uncorrelated answer.
    if(envelopes.some(v=>v.requestId!==requestId))return null;
    const answers=envelopes.map(v=>parseControlResponse(JSON.stringify(v),requestId));
    if(answers.some(v=>!v) || new Set(answers).size!==1)return null;
    return answers[0];
  }
  // Some models obey the inner task's JSON format and omit the outer envelope.
  // Accept only when the DOM proves this is the assistant reply immediately
  // after our exact submitted user message. The native schema still validates it.
  if(!boundToTurn)return null;
  if(candidates.length===1) {
    const answer=JSON.stringify(candidates[0]);
    return Buffer.byteLength(answer)<=65536 ? answer : null;
  }
  if(candidates.length)return null;
  // A plain reply additionally needs a final-message copy control. Without it
  // the text can still be a thinking/status paragraph, even if it is stable.
  const plain=raw.trim();
  return complete && plain && Buffer.byteLength(plain)<=16000 && !/^[\[{`]/.test(plain)
    && !/^(thinking|analyzing|đang suy nghĩ)\b/i.test(plain) ? plain : null;
}

export function responseProblem(raw,requestId,{boundToTurn=false,complete=false,allowedPaths=[]}={}) {
  if(!boundToTurn || !complete || typeof raw!=='string' || Buffer.byteLength(raw)>262144)return null;
  const values=objects(raw,allowedPaths).filter(value=>value.requestId===requestId);
  const answers=new Set(values.map(value=>parseControlResponse(JSON.stringify(value),requestId)).filter(Boolean));
  if(answers.size>1)return 'AMBIGUOUS_MODEL_RESPONSE';
  if(/^sorry,? something went wrong\.?\s*please try (?:your request )?again\.?$/i.test(raw.trim()))return 'PROVIDER_RESPONSE_ERROR';
  return null;
}

// Serialized into the page. Only known composer/send/assistant elements are used.
function pageAction(settings, action, message) {
  // Read only URL metadata on authentication pages, never credentials or forms.
  if(location.origin==='https://accounts.google.com' && /^\/(?:v\d+\/)?signin\/(?:rejected|denied)(?:\/|$)/.test(location.pathname)) return {ready:false,authBlocked:true};
  if (location.origin!==settings.origin) {
    return {ready:false,authRequired:location.hostname==='accounts.google.com'||/\/(?:login|signin|sign-in|auth)(?:\/|$)/i.test(location.pathname)};
  }
  const isVisible=e=>!!e.getClientRects().length && !e.closest('[hidden],[aria-hidden="true"]')
    && getComputedStyle(e).visibility!=='hidden' && getComputedStyle(e).display!=='none';
  const visible=selector=>[...document.querySelectorAll(selector)].find(isVisible);
  const loginButton=[...document.querySelectorAll('button,a,[role="button"]')].find(e=>{
    if(!e.getClientRects().length)return false;
    const label=(e.getAttribute('aria-label')||e.innerText||e.textContent||'').replace(/\s+/g,' ').trim();
    return /^(log in|login|sign in|sign up|log in to chatgpt|sign in with google|continue with google|đăng nhập|đăng ký)$/i.test(label);
  });
  if(loginButton)return {ready:false,authRequired:true};
  const input=visible(settings.input), button=visible(settings.send);
  // Contenteditable paragraphs may serialize blank lines/NBSP differently
  // from a textarea. Compare rendered composer text without changing payloads.
  const canonical=s=>s.replace(/\u00a0/g,' ').replace(/\r\n/g,'\n').replace(/\n{2,}/g,'\n').trim();
  const text=e=>e ? canonical('value' in e ? e.value : e.innerText) : '';
  if(action==='read') {
    const userSelector={chatgpt:'[data-message-author-role="user"], [data-message-role="user"], [data-turn="user"]',claude:'[data-testid="user-message"]',gemini:'user-query'}[settings.provider];
    const users=[...document.querySelectorAll(userSelector)].slice(-16);
    // Rendered user bubbles may collapse line breaks, while the composer keeps
    // them. Compare the complete submitted text with whitespace normalized only.
    const turnText=s=>s.replace(/\s+/gu,' ').trim();
    const matched=users.filter(e=>turnText(text(e)).includes(turnText(message))).at(-1);
    const lastUser=users.at(-1);
    const conflict=!!matched && matched!==lastUser;
    const elements=[...document.querySelectorAll(settings.reply)].slice(-8);
    const replies=elements.map(e=>{
      // Gemini can place a thoughts markdown BEFORE its actual answer. Prefer
      // the final message container; never parse JSON from a thoughts panel.
      const content=settings.provider==='gemini'
        ? e.querySelector('message-content .markdown-main-panel')||e.querySelector('message-content .markdown')||e.querySelector('message-content')||e.querySelector('.markdown-main-panel')||e
        : e.querySelector('.markdown, .prose')||e;
      const clone=content.cloneNode(true);
      clone.querySelectorAll('button,nav,[role="toolbar"],model-thoughts,.thoughts-container,.thoughts-content,[data-test-id="thoughts-container"],[aria-hidden="true"]').forEach(n=>n.remove());
      const container=e.closest('[data-turn],article,[data-testid="assistant-message"]')||e;
      const complete=!!container.querySelector('[data-testid="copy-turn-action-button"],button[aria-label="Copy"],button[aria-label="Copy response"],button[aria-label="Sao chép"]');
      return {
      // Read every code block AND the entire reply, not only its first block.
      text:((clone.textContent||'')+'\n'+[...clone.querySelectorAll('pre code')].map(c=>c.textContent).join('\n')).slice(0,262145),
      complete,
      finalContent:settings.provider==='gemini' && !!e.querySelector('message-content') && !content.closest('model-thoughts,.thoughts-container,.thoughts-content'),
      bound:!!matched && !conflict && !!(matched.compareDocumentPosition(e)&Node.DOCUMENT_POSITION_FOLLOWING),
    };});
    const latest=elements.at(-1);
    const activeStop=[...document.querySelectorAll(settings.stop)].some(e=>isVisible(e) && !e.disabled && e.getAttribute('aria-disabled')!=='true');
    const busy=!!latest && [latest,...latest.querySelectorAll('[aria-busy="true"], [data-is-streaming="true"]')]
      .some(e=>e.matches('[aria-busy="true"], [data-is-streaming="true"]') && isVisible(e)
        && !e.closest('model-thoughts,.thoughts-container,.thoughts-content,[data-test-id="thoughts-container"]'));
    return {streaming:activeStop||busy,replies,conflict,origin:location.origin,assistantCount:elements.length};
  }
  // Empty Gemini (and other composers) may show only a microphone. The Send
  // button is created AFTER input, so it cannot be a readiness prerequisite.
  if(!input || input.disabled || input.readOnly || input.getAttribute('aria-disabled')==='true' || visible(settings.stop)) return {ready:false};
  if(action==='ready') return {ready:true,draft:!!text(input)};
  if(action==='prepare') {
    if(text(input)) return {ready:false,draft:true};
    input.focus();
    if(input instanceof HTMLTextAreaElement || input instanceof HTMLInputElement) {
      const proto=input instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto,'value').set.call(input,message);
      input.dispatchEvent(new Event('input',{bubbles:true}));
    } else {
      const range=document.createRange();range.selectNodeContents(input);
      const selection=window.getSelection();selection.removeAllRanges();selection.addRange(range);
      if(!document.execCommand('insertText',false,message)) {
        input.textContent=message;
        input.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'insertText',data:message}));
      }
    }
    return {ready:true};
  }
  if(!button)return {ready:false};
  if(action==='sendable') return {ready:text(input)===canonical(message) && !button.disabled && button.getAttribute('aria-disabled')!=='true'};
  if(action==='send') {
    if(text(input)!==canonical(message) || button.disabled || button.getAttribute('aria-disabled')==='true') return {ready:false};
    button.click();return {ready:true};
  }
  return {ready:false};
}

export async function runControlTurn(browser, provider, prompt, {signal,notify=()=>{},trace=()=>{},timeoutMs=240000,requestId=randomUUID()}={}) {
  if(!PROVIDERS[provider]) throw new Error('Unsupported AI website');
  const settings={...SELECTORS[provider],provider,origin:PROVIDERS[provider].origin};
  const message=composeControlRequest(prompt,requestId);
  const allowedPaths=templateWindowsPaths(prompt);
  const {cdp,sessionId}=browser;
  const evaluate=async(action,text)=> {
    const response=await cdp.send('Runtime.evaluate',{expression:`(${pageAction.toString()})(${JSON.stringify(settings)},${JSON.stringify(action)},${JSON.stringify(text)})`,returnByValue:true},sessionId);
    if(response.exceptionDetails) throw new Error('Website controls unavailable');
    const state=response.result?.value || {};
    if(state.authBlocked)throw new Error(AUTH_BROWSER_REJECTED);
    return state;
  };
  let nextAuthProbe=0;
  const checkAuth=async()=>{
    if(Date.now()<nextAuthProbe)return;
    nextAuthProbe=Date.now()+1000;
    // OAuth can open another tab. Inspect only URLs of this owned browser;
    // do not attach to, read, click or type into an authentication page.
    let targets;
    try {targets=await cdp.send('Target.getTargets');} catch {return;}
    if(targets?.targetInfos?.some(t=>t.type==='page' && isRejectedAuthPage(t.url))) {
      trace({code:'auth_browser_rejected'});
      throw new Error(AUTH_BROWSER_REJECTED);
    }
  };
  const deadline=Date.now()+Math.min(480000,Math.max(1000,timeoutMs));
  let sawReply=false;
  const check=()=>{if(signal?.aborted) throw new Error('Control request cancelled');if(Date.now()>deadline) throw new Error(sawReply ? 'Website answer format not recognized; timed out' : 'Website response timed out');};
  notify({kind:'status',message:'Đang mở trang AI. Bạn đăng nhập trong cửa sổ riêng nếu cần.'});
  while(true) {
    check();await checkAuth();const ready=await evaluate('ready');
    if(ready.authRequired)throw new Error('AUTH_REQUIRED');
    if(ready.ready) {if(ready.draft) throw new Error('Ô chat đang có nội dung. Hãy xử lý bản nháp rồi thử lại.');break;}
    await delay(250);
  }
  check();
  const prepared=await evaluate('prepare',message);
  if(!prepared.ready) throw new Error('Không thể nhập yêu cầu vào ô chat');
  notify({kind:'status',message:'Đã nhập yêu cầu vào trang AI; đang kiểm tra nút gửi.'});
  while(true) {check();await checkAuth();if((await evaluate('sendable',message)).ready) break;await delay(100);}
  check();notify({kind:'dispatched'}); // From here onwards, never automatically retry.
  if(!(await evaluate('send',message)).ready) throw new Error('Chưa xác định trang AI đã nhận yêu cầu');
  notify({kind:'status',message:'Đã bấm gửi trên web. Đang chờ câu trả lời khớp yêu cầu.'});
  let stable='',since=0,lastTrace='',readErrors=0,refusal='',refusalSince=0,problemText='',problemSince=0;
  while(true) {
    check();await checkAuth();
    let observed;
    try {observed=await evaluate('read',message);}
    catch(error) {
      if(error?.message===AUTH_BROWSER_REJECTED)throw error;
      readErrors++;
      if(readErrors===1){trace({code:'read_context_unavailable'});notify({kind:'status',message:'Phiên đọc trang đang đổi; đang thử đọc lại, không gửi lại yêu cầu.'});}
      await delay(250);continue;
    } // SPA/context changes: retry reads only, never Send.
    if(observed.conflict)throw new Error('Chat conversation changed after dispatch');
    if(!sawReply && observed.replies?.length) {
      sawReply=true;
      notify({kind:'status',message:'Đã thấy phản hồi trên web; đang đọc và kiểm tra kết quả.'});
    }
    // A completed refusal is a model answer, not malformed task JSON. Require
    // the exact current turn and exclude thoughts; never execute it as a plan.
    const refused=(observed.replies||[]).filter(reply=>reply.bound && (reply.complete||reply.finalContent)
      && isModelRefusal(reply.text)).at(-1)?.text.trim() || '';
    if(refused) {
      if(refused!==refusal){refusal=refused;refusalSince=Date.now();}
      else if(Date.now()-refusalSince>=2500) throw Object.assign(new Error('Model refused this request'),{code:'MODEL_REFUSAL'});
    } else {refusal='';refusalSince=0;}
    const problem=(observed.replies||[]).map(reply=>({text:reply.text,code:responseProblem(reply.text,requestId,
      {boundToTurn:reply.bound,complete:reply.complete && !observed.streaming,allowedPaths})})).filter(item=>item.code).at(-1);
    if(problem) {
      if(problemText!==problem.text){problemText=problem.text;problemSince=Date.now();}
      else if(Date.now()-problemSince>=2500)throw Object.assign(new Error('Model response needs review'),{code:problem.code});
    } else {problemText='';problemSince=0;}
    const answer=(observed.replies||[]).map(reply=>readControlResponse(reply.text,requestId,{boundToTurn:reply.bound,complete:reply.complete && !observed.streaming,allowedPaths})).filter(Boolean).at(-1);
    const signature=JSON.stringify([observed.origin,observed.assistantCount,observed.streaming,!!answer,(observed.replies||[]).filter(r=>r.bound).length]);
    if(signature!==lastTrace) {
      lastTrace=signature;
      trace({code:'observed',origin:observed.origin||'',assistantCount:observed.assistantCount||0,streaming:!!observed.streaming,parsed:!!answer,boundCount:(observed.replies||[]).filter(r=>r.bound).length,completeCount:(observed.replies||[]).filter(r=>r.complete).length,finalContentCount:(observed.replies||[]).filter(r=>r.finalContent).length});
    }
    if(answer && !observed.streaming && !problem) {
      if(answer!==stable) {stable=answer;since=Date.now();}
      else if(Date.now()-since>=2500) return {text:answer,model:`web:${provider}`};
    } else {stable='';since=0;}
    await delay(250);
  }
}

async function main() {
  const [profile,provider]=process.argv.slice(2);
  if(!path.isAbsolute(profile||'') || !PROVIDERS[provider]) throw new Error('Invalid browser configuration');
  if(process.argv[4]==='--login') {
    const selection=selectBrowser();
    try {
      const browserProfile=aiBrowserProfile(profile,selection?.kind||'chrome',provider);
      await runInteractiveLogin(browserProfile,{selection,url:PROVIDERS[provider].url,notify:event=>process.stdout.write(JSON.stringify(event)+'\n')});
    } catch(error) {
      const code=['PROFILE_BUSY','BROWSER_MISSING','LOGIN_CLOSED_UNEXPECTEDLY'].includes(error?.code)?error.code:'BROWSER_LAUNCH_FAILED';
      process.stdout.write(JSON.stringify({kind:'login_error',code})+'\n');
      process.exitCode=1;
    }
    return;
  }
  const commands=createInterface({input:process.stdin});
  const abort=new AbortController();
  let resolveInput;
  const input=new Promise(resolve=>{resolveInput=resolve;});
  commands.on('line',line=>{
    if(line==='stop') {abort.abort();return;}
    if(Buffer.byteLength(line)>300000) {abort.abort();return;}
    try {resolveInput(JSON.parse(line));} catch {abort.abort();}
  });
  commands.on('close',()=>abort.abort());
  process.on('SIGTERM',()=>abort.abort());
  const notify=event=>process.stdout.write(JSON.stringify(event)+'\n');
  let browser,dispatched=false,receipt,receiptFile,writes=Promise.resolve();
  const save=()=>{
    if(!receiptFile)return Promise.resolve();
    const snapshot=JSON.stringify({...receipt,updatedAt:new Date().toISOString()});
    writes=writes.then(async()=>{const temp=receiptFile+'.'+process.pid+'.tmp';await writeFile(temp,snapshot);await rename(temp,receiptFile);}).catch(()=>{});
    return writes;
  };
  try {
    const request=await input;
    composeControlRequest(request.prompt,request.requestId);
    const turns=path.join(profile,'turns');await mkdir(turns,{recursive:true});
    receiptFile=path.join(turns,request.requestId+'.json');
    receipt={version:1,requestId:request.requestId,provider,state:'starting',dispatched:false,diagnostics:[]};
    await save();
    const selection=selectBrowser();
    browser=await launchBrowser(await aiWorkBrowserProfile(profile,selection?.kind||'chrome',provider),{selection});
    await browser.cdp.send('Page.navigate',{url:PROVIDERS[provider].url},browser.sessionId);
    await showBrowser(browser);
    const result=await runControlTurn(browser,provider,request.prompt,{signal:abort.signal,requestId:request.requestId,
      trace:event=>{receipt.diagnostics.push(event);receipt.diagnostics=receipt.diagnostics.slice(-24);void save();},
      notify:event=>{if(event.kind==='dispatched'){dispatched=true;receipt.dispatched=true;receipt.state='dispatched';void save();}notify(event);}});
    receipt.state='completed';receipt.result=result;await save();
    notify({kind:'result',...result});
  } catch(error) {
    if(['AMBIGUOUS_MODEL_RESPONSE','PROVIDER_RESPONSE_ERROR'].includes(error?.code)) {
      if(receipt){receipt.state='failed';receipt.reason=error.code.toLowerCase();await save();}
      notify({kind:'error',unknown:dispatched,message:error.code==='AMBIGUOUS_MODEL_RESPONSE'
        ? 'AI trả nhiều kịch bản khác nhau cho cùng một lượt. Kedi dừng để kiểm tra; không tự chọn hoặc tạo lại.'
        : 'Dịch vụ AI trả thông báo lỗi thay vì kịch bản. Kedi dừng; không tự gửi lại.'});
      process.exitCode=1;return;
    }
    if(receipt && receipt.state!=='completed'){receipt.state='failed';receipt.reason=error?.code==='MODEL_REFUSAL'?'model_refusal':error?.message===AUTH_BROWSER_REJECTED?'auth_browser_rejected':error?.message==='Chat conversation changed after dispatch'?'conversation_changed':error?.message?.includes('timed out')?'timeout':abort.signal.aborted?'cancelled':'transport_error';await save();}
    if(error?.code==='MODEL_REFUSAL') {
      notify({kind:'error',unknown:dispatched,message:`${provider==='gemini'?'Gemini':provider==='claude'?'Claude':'ChatGPT'} đã từ chối yêu cầu này, nên chưa có kế hoạch để chạy. Bạn có thể sửa yêu cầu hoặc chọn AI khác. Kedi không tự gửi lại.`});
      process.exitCode=1;return;
    }
    if(error?.message==='AUTH_REQUIRED') {
      notify({kind:'error',unknown:false,message:'Cần đăng nhập trước. Bấm “Đăng nhập web · lưu phiên”, đăng nhập trong cửa sổ Chrome/Edge rồi đóng cửa sổ; lần sau Concat dùng lại phiên đã lưu.'});
      process.exitCode=1;return;
    }
    if(error?.message===AUTH_BROWSER_REJECTED) {
      notify({kind:'error',unknown:dispatched,message:'Google từ chối đăng nhập trong cửa sổ Concat đang điều khiển. Chế độ này chưa dùng được với tài khoản hiện tại. Hãy quay lại yêu cầu để chọn kết nối API đã có, hoặc dừng tác vụ và đăng nhập trong trình duyệt thông thường.'});
      process.exitCode=1;return;
    }
    const detail=error?.message==='Chat conversation changed after dispatch'
      ? 'Hội thoại đã có lượt chat khác sau khi gửi; hãy kiểm tra phản hồi. Không tự gửi lại.'
      : error?.message?.startsWith('Website answer format not recognized')
      ? 'Web đã trả lời nhưng chưa đọc được JSON hợp lệ cho tác vụ. Không tự gửi lại.'
      : 'Kết quả chat web chưa xác định; không tự gửi lại.';
    notify({kind:'error',unknown:dispatched,message:dispatched?detail:error?.code==='PROFILE_BUSY'?'Hồ sơ AI đang được mở. Đóng cửa sổ đăng nhập hoặc chat Concat cũ trước khi chạy tác vụ.':'Chưa gửi yêu cầu. Kiểm tra đăng nhập, bản nháp hoặc nút gửi trong cửa sổ AI.'});
    process.exitCode=1;
  } finally {
    commands.close();process.stdin.destroy();await browser?.close();
  }
}
if(process.argv[1] && path.resolve(process.argv[1])===fileURLToPath(import.meta.url)) main().catch(()=>{process.exitCode=1;});
