import test from 'node:test';
import assert from 'node:assert/strict';
import {composeControlRequest,parseControlResponse,readControlResponse,isRejectedAuthPage,templateWindowsPaths,responseProblem} from './control-web.mjs';

test('reads a complete replacement envelope after a truncated JSON string',()=>{
  const incomplete='{"requestId":"current","result":{"image_prompt":"An unfinished scene, ';
  const final=JSON.stringify({requestId:'current',result:{idea:'Complete replacement',motion_prompt:'Slow pan'}});
  assert.equal(JSON.parse(readControlResponse(incomplete+final,'current',{boundToTurn:true})).idea,'Complete replacement');
  assert.equal(readControlResponse(incomplete+final,'wrong',{boundToTurn:true}),null);
  assert.equal(readControlResponse(incomplete,'current',{boundToTurn:true}),null);
  const conflict=final+'\n'+JSON.stringify({requestId:'current',result:{idea:'Other complete script'}});
  assert.equal(readControlResponse(incomplete+conflict,'current',{boundToTurn:true}),null);
});

test('conflicting plans and provider error text require review instead of retries',()=>{
  const raw=JSON.stringify({requestId:'turn',result:{idea:'First'}})+'\n'+JSON.stringify({requestId:'turn',result:{idea:'Second'}});
  assert.equal(readControlResponse(raw,'turn',{boundToTurn:true,complete:true}),null);
  assert.equal(responseProblem(raw,'turn',{boundToTurn:true,complete:true}),'AMBIGUOUS_MODEL_RESPONSE');
  assert.equal(responseProblem(raw,'turn',{boundToTurn:true}),null);
  assert.equal(responseProblem(raw,'different',{boundToTurn:true,complete:true}),null);
  assert.equal(responseProblem('Sorry, something went wrong. Please try your request again.','turn',{boundToTurn:true,complete:true}),'PROVIDER_RESPONSE_ERROR');
});

test('repairs only exact template Windows paths in a bound current response',()=>{
  const file=String.raw`C:\Users\thecong\Downloads\poster tuyển dụng.png`;
  const allowedPaths=templateWindowsPaths('Template: '+JSON.stringify({providers:{assets:[file]}}));
  assert.deepEqual(allowedPaths,[file]);
  const raw='{"requestId":"path-turn","result":{"providers":{"assets":["'+file+'"]},"caption":"Line one\\nLine two"}}';
  assert.equal(parseControlResponse(raw,'path-turn'),null);
  const answer=JSON.parse(readControlResponse(raw,'path-turn',{boundToTurn:true,allowedPaths}));
  assert.equal(answer.providers.assets[0],file);
  assert.equal(answer.caption,'Line one\nLine two');
  assert.equal(readControlResponse(raw,'path-turn',{allowedPaths}),null);
  assert.equal(readControlResponse(raw,'wrong',{boundToTurn:true,allowedPaths}),null);
  assert.equal(readControlResponse(raw.replace('poster tuyển dụng','unknown'),'path-turn',{boundToTurn:true,allowedPaths}),null);
  assert.equal(readControlResponse(raw.slice(0,-1),'path-turn',{boundToTurn:true,allowedPaths}),null);
  const proper=JSON.stringify({requestId:'path-turn',result:{providers:{assets:[file]}}});
  assert.equal(JSON.parse(readControlResponse(proper,'path-turn',{boundToTurn:true,allowedPaths})).providers.assets[0],file);
  const ambiguous=String.raw`D:\new\test.png`;
  const ambiguousRaw='{"requestId":"path-turn","result":{"anchor":"'+ambiguous+'"}}';
  assert.notEqual(JSON.parse(ambiguousRaw).result.anchor,ambiguous);
  assert.equal(JSON.parse(readControlResponse(ambiguousRaw,'path-turn',{boundToTurn:true,allowedPaths:[ambiguous]})).anchor,ambiguous);
});

test('Control web correlates complete string and JSON results, never stale prose',()=>{
  assert.match(composeControlRequest('Write a script','turn-1'),/Correlation ID: turn-1/);
  assert.equal(parseControlResponse('{"requestId":"turn-1","result":"Actual response"}','turn-1'),'Actual response');
  assert.deepEqual(JSON.parse(parseControlResponse('```json\n{"requestId":"turn-1","result":{"outcome":"needs_input","question":"Which product?"}}\n```','turn-1')),{outcome:'needs_input',question:'Which product?'});
  for(const raw of ['{"requestId":"old","result":"old answer"}','prefix {"requestId":"turn-1","result":"no"}','{"requestId":"turn-1","result":"x","approved":true}','{"requestId":"turn-1","result":[]}']) assert.equal(parseControlResponse(raw,'turn-1'),null);
});
test('Control web bounds requests and responses',()=>{
  for(const id of ['', '../escape','x'.repeat(129)]) assert.throws(()=>composeControlRequest('hello',id));
  assert.throws(()=>composeControlRequest('x'.repeat(131073),'id'));
  assert.equal(parseControlResponse(JSON.stringify({requestId:'id',result:'x'.repeat(65537)}),'id'),null);
});

test('reads envelopes after prose, labels and multiple code blocks',()=>{
  const envelope={requestId:'turn-1',result:{outcome:'plan',steps:[]}};
  const raw='Analysis example:\n```json\n{"example":true}\n```\nResult:\n```json\n'+JSON.stringify(envelope)+'\n```\nCopy code';
  assert.deepEqual(JSON.parse(readControlResponse(raw,'turn-1')),envelope.result);
  assert.equal(readControlResponse(raw,'different'),null);
  assert.equal(readControlResponse(raw+'\n'+JSON.stringify({...envelope,result:'conflicting'}),'turn-1'),null);
});

test('inner task JSON requires an exact DOM-bound current user turn; thinking prose is ignored',()=>{
  const raw='```json\n{"outcome":"needs_input","question":"Which product?"}\n```';
  assert.equal(readControlResponse(raw,'turn-1'),null);
  assert.equal(JSON.parse(readControlResponse(raw,'turn-1',{boundToTurn:true})).question,'Which product?');
  assert.equal(readControlResponse('Actual narration','turn-1'),null);
  assert.equal(readControlResponse('Still thinking...','turn-1',{boundToTurn:true}),null);
  assert.equal(readControlResponse('{"requestId":"old","result":"stale"}','turn-1',{boundToTurn:true}),null);
  assert.equal(readControlResponse('{"requestId":"turn-1","result":"x","approved":true}','turn-1',{boundToTurn:true}),null);
  assert.equal(readControlResponse('{"incomplete":','turn-1',{boundToTurn:true}),null);
  assert.equal(readControlResponse('x'.repeat(262145),'turn-1',{boundToTurn:true}),null);
});

test('completed plain clarification needs both exact turn binding and a final-message marker',()=>{
  assert.equal(readControlResponse('Which story should the film tell?','turn-1',{boundToTurn:true}),null);
  assert.equal(readControlResponse('Which story should the film tell?','turn-1',{complete:true}),null);
  assert.equal(readControlResponse('Which story should the film tell?','turn-1',{boundToTurn:true,complete:true}),'Which story should the film tell?');
  assert.equal(readControlResponse('{"requestId":"old","result":"stale"}','turn-1',{boundToTurn:true,complete:true}),null);
});

test('recognizes rejected Google OAuth URL without treating login as rejection',()=>{
  assert.equal(isRejectedAuthPage('https://accounts.google.com/v3/signin/rejected?app_domain=example'),true);
  assert.equal(isRejectedAuthPage('https://accounts.google.com/signin/denied'),true);
  for(const url of ['https://accounts.google.com/v3/signin/identifier','https://accounts.google.com.evil.test/v3/signin/rejected','https://chatgpt.com/','not a URL'])assert.equal(isRejectedAuthPage(url),false);
});
