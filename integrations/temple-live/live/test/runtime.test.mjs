import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import net from 'node:net';
import {spawn} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import WebSocket from 'ws';
const repo=fileURLToPath(new URL('../../../../',import.meta.url));
const output=path.join(repo,'.runtime/temple-ui');fs.mkdirSync(output,{recursive:true});
const delay=ms=>new Promise(r=>setTimeout(r,ms));
async function port(){const s=net.createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
async function wait(predicate,timeout=6000){const end=Date.now()+timeout;while(Date.now()<end){const value=await predicate();if(value)return value;await delay(30);}throw Error('runtime assertion timed out');}
async function socket(url){
  const ws=new WebSocket(url),messages=[];ws.on('message',b=>messages.push(JSON.parse(b)));
  await new Promise((resolve,reject)=>{ws.once('open',resolve);ws.once('error',reject);});
  return{ws,messages,send:m=>ws.send(JSON.stringify(m)),event:(type,from=0)=>wait(()=>messages.slice(from).find(m=>m.t===type))};
}
test('bundled server preserves HTTP, native WS, state, overlay assets and persistence',async t=>{
  const p=await port(),origin=`http://127.0.0.1:${p}`,data=fs.mkdtempSync(path.join(output,'runtime-test-'));
  fs.writeFileSync(path.join(data,'state.json'),JSON.stringify({settings:{autoChant:false,gifts:{readAll:false,minDiamonds:50000}}}));
  const logs=[];let child;
  const start=async()=>{
    child=spawn(process.execPath,[fileURLToPath(new URL('../server.mjs',import.meta.url))],{windowsHide:true,env:{...process.env,PORT:String(p),HOST:'127.0.0.1',CONCAT_TEMPLE_DATA_DIR:data}});
    child.stdout.on('data',b=>logs.push(String(b)));child.stderr.on('data',b=>logs.push(String(b)));
    await wait(()=>{
      if(child.exitCode!==null)throw Error('Test runtime exited before ready: '+logs.join('').slice(-1000));
      return fetch(origin+'/status').then(r=>r.ok).catch(()=>false);
    },15000);
  };
  const stop=async()=>{if(child&&child.exitCode===null){const end=new Promise(r=>child.once('exit',r));child.kill();await end;}};
  t.after(async()=>{await stop();fs.writeFileSync(path.join(data,'test.log'),logs.join(''));});
  await start();
  const control=await socket(origin.replace('http','ws')+'/ws?role=control');
  const overlay=await socket(origin.replace('http','ws')+'/ws?role=overlay');
  t.after(()=>{control.ws.terminate();overlay.ws.terminate();});
  const initial=await control.event('state');assert.equal(initial.tiktok.status,'idle');assert.equal(initial.viewers.length,0);
  assert.equal((await fetch(origin+'/live/overlay.html')).status,200);
  assert.match(await (await fetch(origin+'/live/overlay.html')).text(),/width="1080" height="1920"/);
  assert.equal((await fetch(origin+'/assets/hall.jpg')).status,200);
  assert.equal((await fetch(origin+'/live/studio.html')).status,200);
  assert.equal((await fetch(origin+'/live/control.html')).status,200);
  assert.equal((await fetch(origin+'/live/server.mjs')).status,404);
  const http=async cmd=>{const response=await fetch(origin+'/api/control',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(cmd)});assert.equal(response.status,200);return response.json();};
  const status=async()=>fetch(origin+'/status').then(r=>r.json());
  await http({cmd:'chat',name:'QA_HTTP',text:'Cầu an'});await wait(async()=>(await status()).viewers===1);
  let joins=overlay.messages.filter(m=>m.t==='join');assert.equal(joins.at(-1).v.demo,true);
  await http({cmd:'clear'});
  const fixture=process.env.TEMPLE_WIRE_FIXTURE||path.join(output,'native-wire.json');
  await t.test('actual Rust-generated native payloads', {skip:!fs.existsSync(fixture)},async()=>{
    const commands=JSON.parse(fs.readFileSync(fixture,'utf8'));
    for(const command of commands){
      if(command.cmd==='fill')await http({cmd:'clear'});
      const from=overlay.messages.length;
      control.send(command);
      if(command.cmd==='chat'){const m=await overlay.event('chat',from);assert.equal(m.name,'QA_Temple');}
      if(command.cmd==='like')await overlay.event('like',from);
      if(command.cmd==='gift'){const m=await overlay.event('gift',from);assert.equal(m.coins,command.diamonds);}
      if(command.cmd==='fill')await wait(async()=>(await status()).viewers===command.n);
      if(command.cmd==='mo')await overlay.event('mo1',from);
      if(command.cmd==='bell')await overlay.event('bell',from);
      if(command.cmd==='bots')await wait(async()=>(await status()).bots.on===command.on);
      if(command.cmd==='skin')await overlay.event('skin',from);
      if(command.cmd==='pin'){const m=await overlay.event('pin',from);assert.equal(m.pinned.title,command.title);}
      if(command.cmd==='stop')await overlay.event('stop',from);
      if(command.cmd==='clear')await wait(async()=>(await status()).viewers===0);
    }
  });
  await http({cmd:'settings',settings:{moBpm:64,voice:'vi-VN-HoaiMyNeural',music:false,seatMode:'36'}});
  await wait(async()=>(await status()).settings.moBpm===64);
  const first=await socket(origin.replace('http','ws')+'/ws?role=control');
  const saved=await first.event('state');assert.equal(saved.settings.voice,'vi-VN-HoaiMyNeural');assert.equal(saved.cap,36);first.ws.terminate();
  await http({cmd:'pin',title:'',price:''});await http({cmd:'clear'});
  control.ws.terminate();overlay.ws.terminate();
  await wait(()=>{try{return JSON.parse(fs.readFileSync(path.join(data,'state.json'),'utf8')).settings.moBpm===64;}catch{return false;}});
  await stop();await start();const restored=await status();assert.equal(restored.settings.voice,'vi-VN-HoaiMyNeural');assert.equal(restored.settings.moBpm,64);assert.equal(restored.tiktok.status,'idle');
});
