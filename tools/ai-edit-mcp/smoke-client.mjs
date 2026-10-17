// Hermetic native smoke client; no model/provider request.
import assert from 'node:assert/strict';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import { fileURLToPath } from 'node:url';
const client = new Client({name:'concat-native-smoke',version:'1'});
const transport = new StdioClientTransport({command:process.execPath,args:[fileURLToPath(new URL('./server.mjs',import.meta.url)),process.argv[2]],stderr:'pipe'});
try {
  await client.connect(transport);
  const tools=await client.listTools();
  assert.equal(tools.tools.length,3);
  const get=async(name,args={})=>{
    const r=await client.callTool({name,arguments:args});
    assert.notEqual(r.isError,true,JSON.stringify(r));
    return JSON.parse(r.content[0].text);
  };
  const {context}=await get('concat_get_context');
  const stale=await client.callTool({name:'concat_propose_edit',arguments:{scope:'stale',plan:{}}});
  assert.equal(stale.isError,true);
  const result=await get('concat_propose_edit',{scope:context.scope,plan:{
    version:1,units:'seconds',timelineId:context.timelineId,baseRevision:context.baseRevision,
    operations:[{op:'split',clipId:context.clipId,time:4,as:'tail'},{op:'remove',clipId:'@tail',ripple:false}],
  }});
  assert.equal(result.applied,false);
  const status=await get('concat_get_status');
  assert.equal(status.draftReady,true); assert.equal(status.applied,false);
  process.stdout.write('MCP_STDIO_DPAPI_NATIVE_DRAFT_PASSED\n');
} finally {await client.close();}
