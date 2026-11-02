// Explicit QA against an isolated OBS portable profile. Never streams/records.
// Run manually: node test/obs-smoke.mjs <own-portable-websocket-config.json> <output-dir>
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import WebSocket from 'ws';
const configPath=path.resolve(process.argv[2]||'');
assert(configPath.includes(path.join('.runtime','temple-ui','obs','config')), 'Only the task-owned portable OBS profile is allowed');
const config=JSON.parse(fs.readFileSync(configPath,'utf8').replace(/^\uFEFF/,''));
const out=path.resolve(process.argv[3]);fs.mkdirSync(out,{recursive:true});
const audioCheck=process.argv.includes('--audio');let audioPeak=0,audioMeterEvents=0;
const ws=new WebSocket(`ws://127.0.0.1:${config.server_port}`);
const sha=s=>crypto.createHash('sha256').update(s).digest('base64');
const pending=new Map();let sequence=0;
const ready=new Promise((resolve,reject)=>{
  const timeout=setTimeout(()=>reject(Error('OBS identify timeout')),10000);
  ws.on('error',reject);
  ws.on('message',bytes=>{
    const m=JSON.parse(bytes);
    if(m.op===0){const auth=m.d.authentication;ws.send(JSON.stringify({op:1,d:{rpcVersion:1,eventSubscriptions:audioCheck?65536:0,...(auth?{authentication:sha(sha(config.server_password+auth.salt)+auth.challenge)}:{})}}));}
    if(m.op===5&&m.d.eventType==='InputVolumeMeters')for(const input of m.d.eventData.inputs||[])if(input.inputName==='Concat Temple QA'){audioMeterEvents++;audioPeak=Math.max(audioPeak,...(input.inputLevelsMul||[]).flat());}
    if(m.op===2){clearTimeout(timeout);resolve();}
    if(m.op===7){const p=pending.get(m.d.requestId);if(p){pending.delete(m.d.requestId);clearTimeout(p.timeout);m.d.requestStatus.result?p.resolve(m.d.responseData||{}):p.reject(Error(`${m.d.requestType}: ${m.d.requestStatus.comment}`));}}
  });
});
function request(requestType,requestData={}){return new Promise((resolve,reject)=>{
  const requestId=String(++sequence),timeout=setTimeout(()=>{pending.delete(requestId);reject(Error(`${requestType} timed out`));},10000);
  pending.set(requestId,{resolve,reject,timeout});ws.send(JSON.stringify({op:6,d:{requestType,requestId,requestData}}));
});}
const delay=ms=>new Promise(r=>setTimeout(r,ms));
let ownCamera=false;
async function cameraState(expected){for(let i=0;i<50;i++){if((await request('GetVirtualCamStatus')).outputActive===expected)return;await delay(100);}throw Error('Virtual camera transition timed out');}
try {
  await ready;
  const version=await request('GetVersion');
  const stream=await request('GetStreamStatus'),record=await request('GetRecordStatus');
  assert(!stream.outputActive&&!record.outputActive, 'An active OBS output must never be altered');
  await request('SetVideoSettings',{baseWidth:1080,baseHeight:1920,outputWidth:1080,outputHeight:1920,fpsNumerator:30,fpsDenominator:1});
  const {currentProgramSceneName:sceneName}=await request('GetSceneList');
  const name='Concat Temple QA';
  const inputs=await request('GetInputList');
  if(!inputs.inputs.some(x=>x.inputName===name))await request('CreateInput',{sceneName,inputName:name,inputKind:'browser_source',inputSettings:{url:'http://127.0.0.1:3199/live/overlay.html',width:1080,height:1920,fps:30,reroute_audio:true,shutdown:false},sceneItemEnabled:true});
  if(audioCheck)await request('SetInputSettings',{inputName:name,inputSettings:{url:'http://127.0.0.1:3199/live/overlay.html?qa='+Date.now(),width:1080,height:1920,reroute_audio:true},overlay:true});
  await request('SetInputMute',{inputName:name,inputMuted:!audioCheck});
  if(audioCheck)await request('SetInputAudioMonitorType',{inputName:name,monitorType:'OBS_MONITORING_TYPE_NONE'});
  const special=await request('GetSpecialInputs');
  for(const inputName of Object.values(special).filter(Boolean))await request('SetInputMute',{inputName,inputMuted:true});
  await delay(5000);
  const settings=await request('GetInputSettings',{inputName:name});
  assert.equal(settings.inputSettings.width,1080);assert.equal(settings.inputSettings.height,1920);
  const capture=async filename=>{const image=await request('GetSourceScreenshot',{sourceName:name,imageFormat:'png'});fs.writeFileSync(path.join(out,filename),Buffer.from(image.imageData.split(',')[1],'base64'));};
  await capture('obs-browser-source.png');
  if(audioCheck){for(let i=0;i<100&&audioPeak<0.00001;i++)await delay(100);assert(audioPeak>0.00001,`Browser Source has no audio signal; events=${audioMeterEvents}`);}
  const origin='http://127.0.0.1:3199';
  const post=async command=>{const r=await fetch(origin+'/api/control',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(command)});assert.equal(r.status,200);};
  await post({cmd:'chat',name:'QA_OBS',text:'Cầu an'});await delay(3500);
  await capture('obs-browser-source-event.png');
  assert.notDeepEqual(fs.readFileSync(path.join(out,'obs-browser-source.png')),fs.readFileSync(path.join(out,'obs-browser-source-event.png')), 'Real browser source must update after a scene event');
  const camBefore=await request('GetVirtualCamStatus');
  let virtualCamera='already active; untouched';
  if(!audioCheck&&!camBefore.outputActive){await request('StartVirtualCam');ownCamera=true;await cameraState(true);await request('StopVirtualCam');await cameraState(false);ownCamera=false;virtualCamera='started and stopped successfully';}
  await post({cmd:'clear'});await post({cmd:'pin',title:'',price:''});
  await request('SetInputMute',{inputName:name,inputMuted:true});
  const result={obsVersion:version.obsVersion,browserSource:settings.inputSettings,video:await request('GetVideoSettings'),sourceUpdated:true,virtualCamera,...(audioCheck?{audioPeak,audioMeterEvents}:{}),streaming:(await request('GetStreamStatus')).outputActive,recording:(await request('GetRecordStatus')).outputActive};
  fs.writeFileSync(path.join(out,'obs-results.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result));
} finally {if(ownCamera){await request('StopVirtualCam').catch(()=>{});await cameraState(false).catch(()=>{});}ws.close();}
