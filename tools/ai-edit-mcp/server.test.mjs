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
    assert.deepEqual(list.tools.map(t=>t.name).sort(),['concat_get_context','concat_get_status','concat_propose_edit']);
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
