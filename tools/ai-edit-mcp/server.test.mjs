import test from 'node:test';
import assert from 'node:assert/strict';
import net from 'node:net';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import { callBridge, createServer, validateConnection } from './server.mjs';

test('MCP tool call reaches Concat and describes the active apply policy', async () => {
  const requests=[];
  const server=createServer(async request => { requests.push(request); return {ok:true,applied:false}; });
  const client=new Client({name:'test',version:'1'});
  const [a,b]=InMemoryTransport.createLinkedPair();
  await Promise.all([server.connect(a),client.connect(b)]);
  try {
    const list=await client.listTools();
    assert.deepEqual(list.tools.map(t=>t.name).sort(),['concat_campaign_control','concat_campaign_draft','concat_campaign_get','concat_campaign_list','concat_control_capabilities','concat_control_status','concat_control_submit','concat_get_context','concat_get_status','concat_propose_edit']);
    assert.equal(list.tools.some(t=>/approve|execute|shell/.test(t.name)),false);
    assert.match(list.tools.find(t=>t.name==='concat_propose_edit').description,/auto-apply is enabled/);
    await client.callTool({name:'concat_get_context',arguments:{}});
    await client.callTool({name:'concat_propose_edit',arguments:{scope:'1:2',plan:{version:1}}});
    assert.deepEqual(requests,[{action:'context'},{action:'propose',scope:'1:2',plan:{version:1}}]);
    const invalid=await client.callTool({name:'concat_propose_edit',arguments:{scope:'',plan:{}}});
    assert.equal(invalid.isError,true);
    assert.equal(requests.length,2);
  } finally { await client.close(); await server.close(); }
});

test('AI can observe a successful native apply from Concat response', async () => {
  const server=createServer(async request => {
    assert.equal(request.action,'propose');
    return {ok:true,applied:true,count:2};
  });
  const client=new Client({name:'test',version:'1'});
  const [a,b]=InMemoryTransport.createLinkedPair();
  await Promise.all([server.connect(a),client.connect(b)]);
  try {
    const result=await client.callTool({name:'concat_propose_edit',arguments:{scope:'1:2',plan:{version:1}}});
    assert.equal(result.isError,false);
    assert.equal(JSON.parse(result.content[0].text).applied,true);
  } finally { await client.close(); await server.close(); }
});

test('Control contract keeps permission grants outside model input and preserves refusal', async () => {
  const requests=[];
  const server=createServer(async request=>{requests.push(request);return {ok:false,error:'MCP Control is disabled'};});
  const client=new Client({name:'control-contract',version:'1'});
  const [a,b]=InMemoryTransport.createLinkedPair();
  await Promise.all([server.connect(a),client.connect(b)]);
  try {
    const list=await client.listTools();
    for(const tool of list.tools.filter(t=>t.name.startsWith('concat_control_'))) assert.equal(tool.outputSchema.type,'object');
    const submit=list.tools.find(t=>t.name==='concat_control_submit');
    assert.equal('network' in submit.inputSchema.properties,false);
    assert.equal('mode' in submit.inputSchema.properties,false);
    const result=await client.callTool({name:submit.name,arguments:{command:'Xem thư viện',mode:'full_auto',network:true}});
    assert.equal(result.isError,true);
    assert.equal(result.structuredContent.ok,false);
    assert.deepEqual(requests,[{action:'control',operation:'submit',input:{command:'Xem thư viện',script:'',model:'',audioPath:'',workflowKind:'auto'}}]);
    await client.callTool({name:submit.name,arguments:{command:'Read library',workflowKind:'shell'}});
    assert.equal(requests.length,1);
    await client.callTool({name:submit.name,arguments:{command:'x'.repeat(16001)}});
    assert.equal(requests.length,1);
    await client.callTool({name:'concat_control_status',arguments:{}});
    assert.equal(requests[1].operation,'status');
  } finally {await client.close();await server.close();}
});

test('local connection rejects public hosts and malformed credentials', () => {
  const value={version:1,host:'127.0.0.1',port:12345,token:'a'.repeat(64)};
  assert.equal(validateConnection(value),value);
  for(const patch of [{host:'0.0.0.0'},{host:'example.com'},{port:0},{token:'short'}]) {
    assert.throws(()=>validateConnection({...value,...patch}));
  }
});

test('real TCP handoff is bounded and returns structured bridge refusal unchanged', async () => {
  const server=net.createServer(socket=>socket.once('data',data=> {
    const value=JSON.parse(data.toString());
    assert.equal(value.token,'a'.repeat(64));
    socket.end(JSON.stringify({ok:false,error:'stale',applied:false})+'\n');
  }));
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  try {
    const result=await callBridge({version:1,host:'127.0.0.1',port:server.address().port,token:'a'.repeat(64)},{action:'status'});
    assert.deepEqual(result,{ok:false,error:'stale',applied:false});
  } finally { await new Promise(resolve=>server.close(resolve)); }
});

test('bridge errors are sanitized, not raw paths or credentials', async () => {
  const server=createServer(async()=>{throw new Error('private credential fixture');});
  const client=new Client({name:'test',version:'1'});
  const [a,b]=InMemoryTransport.createLinkedPair();
  await Promise.all([server.connect(a),client.connect(b)]);
  try {
    const result=await client.callTool({name:'concat_get_status',arguments:{}});
    assert.equal(result.isError,true);
    assert.doesNotMatch(JSON.stringify(result),/private credential/);
  } finally {await client.close();await server.close();}
});

test('legacy timeout replies cannot claim that a persisted Control write did not run', async () => {
  const server=net.createServer(socket=>socket.once('data',()=>socket.end(JSON.stringify({ok:false,error:'Bridge unavailable, invalid request or timeout; no edit was applied. Do not replay automatically.'})+'\n')));
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  try {
    const result=await callBridge({version:1,host:'127.0.0.1',port:server.address().port,token:'a'.repeat(64)},{action:'control',operation:'submit',input:{command:'render'}});
    assert.equal(result.ok,false);
    assert.match(result.error,/outcome unknown/);
    assert.doesNotMatch(result.error,/no edit was applied/i);
  } finally {await new Promise(resolve=>server.close(resolve));}
});
test('Campaign MCP exposes bounded draft and control actions, never plan approval', async () => {
  const requests=[];
  const server=createServer(async request=>{requests.push(request);return {ok:true,accepted:true};});
  const client=new Client({name:'campaign-test',version:'1'});
  const [a,b]=InMemoryTransport.createLinkedPair();
  await Promise.all([server.connect(a),client.connect(b)]);
  try {
    const listed=await client.listTools();
    assert.equal(listed.tools.some(tool=>/campaign.*(approve|publish|execute)|shell/i.test(tool.name)),false);
    const draft=await client.callTool({name:'concat_campaign_draft',arguments:{plan:{objective:'new draft only'}}});
    assert.notEqual(draft.isError,true);
    assert.deepEqual(requests[0],{action:'control',operation:'campaign.draft',input:{plan:{objective:'new draft only'}}});
    const bad=await client.callTool({name:'concat_campaign_control',arguments:{campaign_id:'../../secret',action:'resume'}});
    assert.equal(bad.isError,true);
    assert.equal(requests.length,1);
  } finally {await client.close();await server.close();}
});

