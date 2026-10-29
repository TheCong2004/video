// Actual browser DOM/CDP with intercepted local fixtures, never a live model call.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,rm} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {launchBrowser} from './browser-cdp.mjs';
import {PROVIDERS} from './web-chat.mjs';
import {runControlTurn} from './control-web.mjs';

async function fixture(provider,body,{format='envelope',emitUser=false,foreignTurn=false,turnAttributes=false,staleBusy=false,sendAfterInput=false,quillClipboard=false,authRequired=false}={}) {
  const root=fileURLToPath(new URL('../../.tmp/',import.meta.url));
  await mkdir(root,{recursive:true});
  const scratch=await mkdtemp(path.join(root,'control-web-test-'));
  let browser;
  try {
    browser=await launchBrowser(scratch,{headless:true});
    const {cdp,sessionId}=browser;
    const markup={
      chatgpt:['<textarea id="mobile-composer-prompt"></textarea>','data-testid="send-button"','data-message-role="assistant"'],
      gemini:[`<rich-textarea>${quillClipboard?'<div class="ql-clipboard" contenteditable="true">clipboard sentinel</div>':''}<div class="ql-editor" role="textbox" contenteditable="true"></div></rich-textarea>`,sendAfterInput?'aria-label="Send message"':'class="send-button"','data-fixture="gemini"'],
      claude:['<div class="ProseMirror" contenteditable="true"></div>','aria-label="Send message"','class="font-claude-response"'],
    }[provider];
    const html=`<!doctype html><body>${authRequired?'<button>Log in</button>':''}${markup[0]}<button ${markup[1]}>Send</button><main></main><script>
      window.sent=[];window.suppress=false;
      if(${staleBusy}) {const old=document.createElement('div');old.setAttribute('data-turn','assistant');old.setAttribute('aria-busy','true');old.textContent='Old unfinished response';document.querySelector('main').append(old);}
      const send=document.querySelector('button');
      send.onclick=()=>{
        const input=document.querySelector('textarea,[contenteditable]:not(.ql-clipboard)');
        const text='value' in input ? input.value : input.innerText;window.sent.push(text);
        if('value' in input)input.value='';else input.textContent='';
        const id=text.match(/Correlation ID: ([^\\n]+)/)[1];
        if(${emitUser}) {
          const user=document.createElement('${provider==='gemini'?'user-query':'div'}');
          user.setAttribute('${turnAttributes?'data-turn':provider==='claude'?'data-testid':'data-message-author-role'}','${provider==='claude'?'user-message':'user'}');
          user.textContent=text;document.querySelector('main').append(user);
          if(${foreignTurn}) {const other=user.cloneNode();other.textContent='A different manual turn';document.querySelector('main').append(other);}
        }
        if(window.suppress)return;
        const reply=document.createElement('${provider==='gemini'?'model-response':'div'}');
        reply.outerHTML;
        ${provider==='gemini'?'':`reply.setAttribute('${turnAttributes?'data-turn':provider==='chatgpt'?'data-message-role':'class'}','${provider==='chatgpt'?'assistant':'font-claude-response'}');`}
        const result={outcome:'plan',version:1,summary:'Read actual library',requirements:[{description:'List videos',tools:['library.list']}],steps:[{id:'library',tool:'library.list',dependencies:[],input:{}}]};
        const envelope={requestId:id,result};
        if(${JSON.stringify(format)}==='multiple-code') {
          reply.textContent='Analysis example and final answer: ';
          for(const value of [{example:true},envelope]) {const pre=document.createElement('pre'),code=document.createElement('code');code.textContent=JSON.stringify(value);pre.append(code);reply.append(pre);}
        } else reply.textContent=JSON.stringify(${JSON.stringify(format)}==='inner-json' ? result : envelope);
        if(${JSON.stringify(format)}==='truncated-replacement') {
          reply.textContent=${JSON.stringify('{"requestId":"')}+id+${JSON.stringify('","result":{"image_prompt":"An unfinished scene, ')}+JSON.stringify(envelope);
        }
        if(${JSON.stringify(format)}==='windows-path') {
          const file=${JSON.stringify(String.raw`C:\Users\thecong\Downloads\poster tuyển dụng.png`)};
          reply.textContent='{"requestId":"'+id+'","result":{"providers":{"assets":["'+file+'"]},"video_count":9}}';
        }
        document.querySelector('main').append(reply);
        if(${JSON.stringify(format)}==='conflicting-plans' || ${JSON.stringify(format)}==='provider-error') {
          reply.textContent=${JSON.stringify(format)}==='provider-error' ? 'Sorry, something went wrong. Please try your request again.'
            : JSON.stringify(envelope)+String.fromCharCode(10)+JSON.stringify({requestId:id,result:{...result,summary:'Different plan'}});
          const copy=document.createElement('button');copy.setAttribute('data-testid','copy-turn-action-button');reply.append(copy);
        }
        if(${JSON.stringify(format)}==='gemini-refusal' || ${JSON.stringify(format)}==='gemini-hidden-busy') {
          reply.textContent='';
          const thoughts=document.createElement('model-thoughts');thoughts.setAttribute('aria-busy','true');thoughts.textContent='Thinking';reply.append(thoughts);
          const final=document.createElement('message-content'),markdown=document.createElement('div');markdown.className='markdown-main-panel';
          markdown.textContent=${JSON.stringify(format)}==='gemini-refusal'
            ? 'Tôi không thể trợ giúp về điều đó, vì tôi chỉ là một mô hình ngôn ngữ.' : JSON.stringify(envelope);
          final.append(markdown);reply.append(final);
          const hiddenStop=document.createElement('div');hiddenStop.setAttribute('aria-hidden','true');hiddenStop.innerHTML='<button aria-label="Stop response">Stop</button>';document.body.append(hiddenStop);
        }
        if(${JSON.stringify(format)}==='plain-final') {
          reply.textContent='Which story should the film tell?';
          const copy=document.createElement('button');copy.setAttribute('data-testid','copy-turn-action-button');copy.textContent='Copy';reply.append(copy);
        }
        if(${JSON.stringify(format)}==='streaming') {
          const stop=document.createElement('button');stop.setAttribute('data-testid','stop-button');stop.textContent='Stop';document.body.append(stop);
          setTimeout(()=>{result.summary='Final completed plan';reply.textContent=JSON.stringify({requestId:id,result});stop.remove();},1800);
        }
        if(${JSON.stringify(format)}==='gemini-thoughts' || ${JSON.stringify(format)}==='gemini-thoughts-only') {
          reply.textContent='';
          const thoughts=document.createElement('model-thoughts');
          thoughts.className='thoughts-container';
          const draft=document.createElement('div');draft.className='markdown';
          const pre=document.createElement('pre'),code=document.createElement('code');
          code.textContent=JSON.stringify({outcome:'unsupported',reason:'Draft, not the final answer'});
          pre.append(code);draft.append(pre);thoughts.append(draft);reply.append(thoughts);
          if(${JSON.stringify(format)}==='gemini-thoughts')setTimeout(()=>{
            const final=document.createElement('message-content'),markdown=document.createElement('div');
            markdown.className='markdown markdown-main-panel';
            markdown.textContent=JSON.stringify({requestId:id,result:{outcome:'needs_input',question:'Bạn muốn giới thiệu sản phẩm nào?'}});
            final.append(markdown);reply.append(final);
          },3500);
        }
      };
      if(${sendAfterInput}) {
        send.remove();let scheduled=false;
        document.querySelector('textarea,[contenteditable]:not(.ql-clipboard)').addEventListener('input',()=>{
          if(scheduled)return;scheduled=true;
          setTimeout(()=>document.querySelector('main').before(send),180);
        });
      }
    </script>`;
    cdp.on('Fetch.requestPaused',event=>{
      void cdp.send('Fetch.fulfillRequest',{requestId:event.requestId,responseCode:200,responseHeaders:[{name:'Content-Type',value:'text/html; charset=utf-8'}],body:Buffer.from(html).toString('base64')},sessionId).catch(()=>{});
    });
    await cdp.send('Fetch.enable',{patterns:[{urlPattern:'*',requestStage:'Request'}]},sessionId);
    await cdp.send('Page.navigate',{url:PROVIDERS[provider].url},sessionId);
    await body(browser);
  } finally {await browser?.close();await rm(scratch,{recursive:true,force:true,maxRetries:30,retryDelay:100});}
}
async function evaluate(browser,expression) {
  const reply=await browser.cdp.send('Runtime.evaluate',{expression,returnByValue:true},browser.sessionId);
  if(reply.exceptionDetails)throw new Error('Fixture expression failed');
  return reply.result.value;
}
test('Gemini complete replacement after a truncated prompt is recovered without another Send',async()=>fixture('gemini',async browser=>{
  const answer=await runControlTurn(browser,'gemini','Write one result',{requestId:'replacement',timeoutMs:12000});
  assert.equal(JSON.parse(answer.text).steps[0].tool,'library.list');
  assert.equal(await evaluate(browser,'window.sent.length'),1);
},{format:'truncated-replacement',emitUser:true}));
for(const [format,code] of [['conflicting-plans','AMBIGUOUS_MODEL_RESPONSE'],['provider-error','PROVIDER_RESPONSE_ERROR']]) {
  test(`Gemini ${format} is held for review after exactly one Send`,async()=>fixture('gemini',async browser=>{
    await assert.rejects(runControlTurn(browser,'gemini','Write one script',{requestId:'hold-response',timeoutMs:12000}),error=>error.code===code);
    assert.equal(await evaluate(browser,'window.sent.length'),1);
  },{format,emitUser:true}));
}
test('Gemini JSON with an unescaped template Windows path is read without another Send',async()=>fixture('gemini',async browser=>{
  const file=String.raw`C:\Users\thecong\Downloads\poster tuyển dụng.png`;
  const prompt='Return a plan using this template: '+JSON.stringify({providers:{assets:[file]},video_count:9});
  const answer=await runControlTurn(browser,'gemini',prompt,{requestId:'windows-path',timeoutMs:15000});
  assert.equal(JSON.parse(answer.text).providers.assets[0],file);
  assert.equal(JSON.parse(answer.text).video_count,9);
  assert.equal(await evaluate(browser,'window.sent.length'),1);
},{format:'windows-path',emitUser:true}));
test('Gemini refusal in the current final message stops without a JSON timeout or resend',async()=>fixture('gemini',async browser=>{
  await assert.rejects(runControlTurn(browser,'gemini','Write a campaign plan',{requestId:'refusal',timeoutMs:12000}),error=>error.code==='MODEL_REFUSAL');
  assert.equal(await evaluate(browser,'window.sent.length'),1);
},{format:'gemini-refusal',emitUser:true}));
test('Gemini ignores hidden stop controls and stale thoughts busy state after final JSON',async()=>fixture('gemini',async browser=>{
  const answer=await runControlTurn(browser,'gemini','Write a campaign plan',{requestId:'final-json',timeoutMs:12000});
  assert.equal(JSON.parse(answer.text).steps[0].tool,'library.list');
  assert.equal(await evaluate(browser,'window.sent.length'),1);
},{format:'gemini-hidden-busy',emitUser:true}));
test('An unrelated Gemini refusal never binds to the submitted request',async()=>fixture('gemini',async browser=>{
  await assert.rejects(runControlTurn(browser,'gemini','Write a campaign plan',{requestId:'unbound-refusal',timeoutMs:3200}),/timed out/);
  assert.equal(await evaluate(browser,'window.sent.length'),1);
},{format:'gemini-refusal'}));

test('Gemini waits past a stable draft JSON and reads only the final message-content',async()=>fixture('gemini',async browser=>{
  const answer=await runControlTurn(browser,'gemini','Write a product introduction',{requestId:'final-not-thoughts',timeoutMs:16000});
  assert.deepEqual(JSON.parse(answer.text),{outcome:'needs_input',question:'Bạn muốn giới thiệu sản phẩm nào?'});
  assert.equal(await evaluate(browser,'window.sent.length'),1);
},{format:'gemini-thoughts',emitUser:true}));

test('Gemini never treats a thoughts-only JSON draft as a completed reply',async()=>fixture('gemini',async browser=>{
  await assert.rejects(runControlTurn(browser,'gemini','Write a product introduction',{requestId:'thoughts-only',timeoutMs:3200}),/timed out/);
  assert.equal(await evaluate(browser,'window.sent.length'),1);
},{format:'gemini-thoughts-only',emitUser:true}));
for(const provider of ['chatgpt','gemini','claude']) {
  test(`Control web ${provider}: types, clicks exactly once and reads correlated plan`,async()=>fixture(provider,async browser=>{
    const events=[];
    const answer=await runControlTurn(browser,provider,'List saved videos',{requestId:'test-turn',timeoutMs:15000,notify:e=>events.push(e)});
    assert.equal(answer.model,`web:${provider}`);
    assert.equal(JSON.parse(answer.text).steps[0].tool,'library.list');
    assert.equal(await evaluate(browser,'window.sent.length'),1);
    assert.equal(events.filter(e=>e.kind==='dispatched').length,1);
  }));
}
test('Control web preserves a manual draft without sending',async()=>fixture('chatgpt',async browser=>{
  // Wait for the document through the same readiness loop before adding a draft.
  for(let i=0;i<40;i++) {
    if(await evaluate(browser,'!!document.querySelector("textarea")'))break;
    await new Promise(resolve=>setTimeout(resolve,50));
  }
  await evaluate(browser,'document.querySelector("textarea").value="manual draft"');
  await assert.rejects(runControlTurn(browser,'chatgpt','Another task',{timeoutMs:5000}),/bản nháp/);
  assert.equal(await evaluate(browser,'window.sent.length'),0);
  assert.equal(await evaluate(browser,'document.querySelector("textarea").value'),'manual draft');
}));
test('Control web cancellation after click never sends twice',async()=>fixture('chatgpt',async browser=>{
  const abort=new AbortController();
  await assert.rejects(runControlTurn(browser,'chatgpt','A task',{
    timeoutMs:10000,signal:abort.signal,
    notify:event=>{if(event.kind==='status' && event.message.startsWith('Đã bấm gửi'))abort.abort();},
  }),/cancelled/);
  assert.equal(await evaluate(browser,'window.sent.length'),1);
}));

test('Control reads final envelope when the first code block is an example',async()=>fixture('chatgpt',async browser=>{
  const answer=await runControlTurn(browser,'chatgpt','List videos',{requestId:'multiblock',timeoutMs:15000});
  assert.equal(JSON.parse(answer.text).steps[0].tool,'library.list');
  assert.equal(await evaluate(browser,'window.sent.length'),1);
},{format:'multiple-code'}));

for(const provider of ['chatgpt','claude','gemini']) {
  test(`Control ${provider} reads task JSON without envelope only after its exact user turn`,async()=>fixture(provider,async browser=>{
    const answer=await runControlTurn(browser,provider,'List videos',{requestId:'inner-json',timeoutMs:15000});
    assert.equal(JSON.parse(answer.text).steps[0].tool,'library.list');
    assert.equal(await evaluate(browser,'window.sent.length'),1);
  },{format:'inner-json',emitUser:true}));
}

test('Control ignores bare JSON without current user-turn proof, never resends',async()=>fixture('chatgpt',async browser=>{
  await assert.rejects(runControlTurn(browser,'chatgpt','List videos',{requestId:'unbound',timeoutMs:2500}),/timed out/);
  assert.equal(await evaluate(browser,'window.sent.length'),1);
},{format:'inner-json'}));

test('Control rejects a manual turn intervening after dispatch',async()=>fixture('chatgpt',async browser=>{
  await assert.rejects(runControlTurn(browser,'chatgpt','List videos',{requestId:'foreign',timeoutMs:5000}),/changed/);
  assert.equal(await evaluate(browser,'window.sent.length'),1);
},{format:'inner-json',emitUser:true,foreignTurn:true}));

test('Control retries a transient response read failure without another Send',async()=>fixture('chatgpt',async browser=>{
  const original=browser.cdp.send.bind(browser.cdp);
  let failNextRead=false,failed=false;
  browser.cdp.send=async(method,params,...rest)=>{
    if(failNextRead && !failed && method==='Runtime.evaluate'){failed=true;throw new Error('Execution context destroyed');}
    return original(method,params,...rest);
  };
  const answer=await runControlTurn(browser,'chatgpt','List videos',{requestId:'spa-turn',timeoutMs:15000,notify:e=>{
    if(e.kind==='status' && e.message.startsWith('Đã bấm gửi'))failNextRead=true;
  }});
  assert.equal(failed,true);
  assert.equal(JSON.parse(answer.text).steps[0].tool,'library.list');
  assert.equal(await evaluate(browser,'window.sent.length'),1);
}));

test('Control waits for streaming to finish before accepting a complete JSON prefix',async()=>fixture('chatgpt',async browser=>{
  const answer=await runControlTurn(browser,'chatgpt','List videos',{requestId:'stream-turn',timeoutMs:15000});
  assert.equal(JSON.parse(answer.text).summary,'Final completed plan');
  assert.equal(await evaluate(browser,'window.sent.length'),1);
},{format:'streaming'}));

test('Control reads the data-turn DOM variant used by the TobyFlow fallback',async()=>fixture('chatgpt',async browser=>{
  const observations=[];
  const answer=await runControlTurn(browser,'chatgpt','List videos',{requestId:'data-turn-test',timeoutMs:15000,trace:e=>observations.push(e)});
  assert.equal(JSON.parse(answer.text).steps[0].tool,'library.list');
  assert.ok(observations.some(e=>e.parsed && e.boundCount===1));
  assert.equal(await evaluate(browser,'window.sent.length'),1);
},{format:'inner-json',emitUser:true,turnAttributes:true}));

test('An old busy assistant bubble does not hide the completed current response',async()=>fixture('chatgpt',async browser=>{
  const answer=await runControlTurn(browser,'chatgpt','List videos',{requestId:'stale-busy-test',timeoutMs:15000});
  assert.equal(JSON.parse(answer.text).steps[0].tool,'library.list');
  assert.equal(await evaluate(browser,'window.sent.length'),1);
},{emitUser:true,turnAttributes:true,staleBusy:true}));

test('Control receives a completed plain clarification instead of waiting for an envelope forever',async()=>fixture('chatgpt',async browser=>{
  const answer=await runControlTurn(browser,'chatgpt','List videos',{requestId:'plain-final-test',timeoutMs:15000});
  assert.equal(answer.text,'Which story should the film tell?');
  assert.equal(await evaluate(browser,'window.sent.length'),1);
},{format:'plain-final',emitUser:true,turnAttributes:true}));

test('rejected Google login in the controlled tab stops before typing or Send',async()=>fixture('chatgpt',async browser=>{
  await browser.cdp.send('Page.navigate',{url:'https://accounts.google.com/v3/signin/rejected?app_domain=fixture'},browser.sessionId);
  for(let i=0;i<40;i++){if(await evaluate(browser,'Array.isArray(window.sent)'))break;await new Promise(r=>setTimeout(r,50));}
  const events=[];
  await assert.rejects(runControlTurn(browser,'chatgpt','Do not send',{timeoutMs:5000,notify:e=>events.push(e)}),/GOOGLE_AUTH_BROWSER_REJECTED/);
  assert.equal(events.some(e=>e.kind==='dispatched'),false);
  assert.equal(await evaluate(browser,'window.sent.length'),0);
}));

test('web MCP refuses to send until the user signs in to the saved browser profile',async()=>fixture('chatgpt',async browser=>{
  const events=[];
  await assert.rejects(runControlTurn(browser,'chatgpt','Do not send',{timeoutMs:5000,notify:e=>events.push(e)}),/AUTH_REQUIRED/);
  assert.equal(events.some(e=>e.kind==='dispatched'),false);
  assert.equal(await evaluate(browser,'window.sent.length'),0);
},{authRequired:true}));

test('rejected OAuth popup is detected from owned target metadata without attaching to it',async()=>fixture('chatgpt',async browser=>{
  for(let i=0;i<40;i++){if(await evaluate(browser,'Array.isArray(window.sent)'))break;await new Promise(r=>setTimeout(r,50));}
  const original=browser.cdp.send.bind(browser.cdp);
  browser.cdp.send=async(method,...args)=> method==='Target.getTargets'
    ? {targetInfos:[{type:'page',url:'https://accounts.google.com/v3/signin/rejected?do_not_log=this'}]}
    : original(method,...args);
  await assert.rejects(runControlTurn(browser,'chatgpt','Do not send',{timeoutMs:5000}),/GOOGLE_AUTH_BROWSER_REJECTED/);
  assert.equal(await evaluate(browser,'window.sent.length'),0);
}));

test('auth rejection after dispatch remains dispatched and never sends again',async()=>fixture('chatgpt',async browser=>{
  const original=browser.cdp.send.bind(browser.cdp);let blocked=false;
  browser.cdp.send=async(method,...args)=> method==='Target.getTargets' && blocked
    ? {targetInfos:[{type:'page',url:'https://accounts.google.com/v3/signin/rejected'}]}
    : original(method,...args);
  const events=[];
  await assert.rejects(runControlTurn(browser,'chatgpt','Send once',{timeoutMs:8000,notify:e=>{events.push(e);if(e.kind==='dispatched')blocked=true;}}),/GOOGLE_AUTH_BROWSER_REJECTED/);
  assert.equal(events.filter(e=>e.kind==='dispatched').length,1);
  assert.equal(await evaluate(browser,'window.sent.length'),1);
}));

for(const provider of ['gemini','chatgpt','claude']) {
  test(`${provider}: compose before waiting for a Send button that only appears after input`,async()=>fixture(provider,async browser=>{
    const answer=await runControlTurn(browser,provider,'List videos',{requestId:'deferred-send',timeoutMs:15000});
    assert.equal(JSON.parse(answer.text).steps[0].tool,'library.list');
    assert.equal(await evaluate(browser,'window.sent.length'),1);
  },{sendAfterInput:true}));
}

test('Gemini chooses the real editor instead of the auxiliary Quill clipboard',async()=>fixture('gemini',async browser=>{
  const answer=await runControlTurn(browser,'gemini','List videos',{requestId:'quill-editor',timeoutMs:15000});
  assert.equal(JSON.parse(answer.text).steps[0].tool,'library.list');
  assert.equal(await evaluate(browser,'document.querySelector(".ql-clipboard").textContent'),'clipboard sentinel');
  assert.equal(await evaluate(browser,'window.sent.length'),1);
},{sendAfterInput:true,quillClipboard:true}));

test('Gemini preserves a manual draft even when no Send button has been created',async()=>fixture('gemini',async browser=>{
  for(let i=0;i<40;i++){if(await evaluate(browser,'!!document.querySelector(".ql-editor")'))break;await new Promise(r=>setTimeout(r,50));}
  await evaluate(browser,'document.querySelector(".ql-editor").textContent="manual Gemini draft"');
  await assert.rejects(runControlTurn(browser,'gemini','List videos',{timeoutMs:5000}),/bản nháp/);
  assert.equal(await evaluate(browser,'document.querySelector(".ql-editor").textContent'),'manual Gemini draft');
  assert.equal(await evaluate(browser,'window.sent.length'),0);
},{sendAfterInput:true}));
