import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
const require=createRequire(new URL('../../promax-backend/package.json',import.meta.url));
const {GraphWorkflowDefinitionSchema}=require('@promax/contracts');
const {productionWorkflowDigest}=require('./dist/integrations/artcraft-runtime/production-workflow-binding.js');
const graph=JSON.parse(readFileSync(new URL('../docs/contracts/native-production-v1.workflow.json',import.meta.url),'utf8'));
test('native seven-stage contract is accepted unchanged by the actual BE schema',()=>{
  assert.deepEqual(GraphWorkflowDefinitionSchema.parse(graph),graph);
  assert.equal(graph.nodes.length,7);assert.equal(graph.edges.length,6);
  assert.equal(productionWorkflowDigest(graph),'ec6a85ff214a72ed5832e7d4efcb2c512854629dc55a622f7dd41974a1687ebf');
  assert.equal(graph.settings.parallelExecution,false);
  for(let i=0;i<graph.nodes.length;i++){
    const node=graph.nodes[i];assert.equal(node.retryable,false);assert.equal(node.maxAttempts,1);
    if(i){assert.ok(graph.edges.some(e=>e.sourceNodeId===graph.nodes[i-1].nodeId&&e.targetNodeId===node.nodeId));}
  }
});
test('native worker is behind Promax scheduling and does not export cloud credentials',()=>{
  const read=p=>readFileSync(new URL(p,import.meta.url),'utf8');
  const native=read('../src/crates/concat/src/studio_native_production.rs');
  assert.match(native,/CentralWorker::via_nexora/);assert.match(native,/cloud\s*\.preflight/);
  assert.doesNotMatch(native,/\.run\(\s*plan/);
  const nexora=read('../../donutbrowser/src-tauri/src/concat_worker_bridge.rs');
  assert.match(nexora,/bearer|authorization/);assert.match(nexora,/1024 \* 1024/);
  assert.doesNotMatch(nexora,/Json\(.*session\.access/);
});

test('Create owns Grok startup; configuration checks never open a tab and execution still requires preflight',()=>{
  const read=p=>readFileSync(new URL(p,import.meta.url),'utf8');
  const native=read('../src/crates/concat/src/studio_native_production.rs');
  const ui=read('../src/crates/concat/ui/workspace/studio-production.slint');
  const controller=read('../src/crates/concat/src/studio_production.rs');
  const dry=native.indexOf('if !start_after_prepare');
  const open=native.indexOf('ensure_production_grok');
  assert.ok(dry>=0 && open>dry);
  assert.match(native.slice(dry,open),/return;/);
  assert.ok(native.indexOf('production_browser_ready',open)>open);
  assert.match(native.slice(open),/cloud\s*\.preflight/);
  assert.match(native,/submit_native\(&app,\s*checked\)/);
  assert.match(controller,/get_native_workflow\(\)/);
  assert.match(controller,/get_confirm_start\(\)/);
  assert.match(controller,/error == RunError::UnknownOutcome[\s\S]*?set_uncertain\(true\)[\s\S]*?else if native_reservation/);
  assert.match(native,/runtime\.pipeline\.id\(\) == reservation[\s\S]*?runtime\.worker\.status\(\) != "outcome_unknown"/);
  assert.match(ui,/ProductionStudio.native-workflow \? ProductionStudio.page-index > 0 : ProductionStudio.ready/);
});
