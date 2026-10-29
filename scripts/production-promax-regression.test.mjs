import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const read=p=>readFileSync(new URL(`../${p}`,import.meta.url),'utf8');

test('production execution is never overridden by a direct Nexora runner',()=>{
  const code=read('src/crates/concat/src/studio_production.rs');
  assert.doesNotMatch(code,/studio_production_local::bind\(/);
  assert.match(code,/studio_session::production_client\(\)/);
});

test('central production delegates authentication to Nexora, not local AI or Concat login',()=>{
  const code=read('src/crates/concat/src/studio_session.rs');
  assert.match(code,/WorkflowClient::via_nexora/);
  assert.doesNotMatch(code,/LoginAttempt/);
  assert.doesNotMatch(code,/studio_local_ai::client\(/);
  assert.match(code,/\.client\(\)/);
});

test('central submit preserves server scheduler authority and preflight',()=>{
  const code=read('src/crates/concat-studio/src/workflows.rs');
  assert.match(code,/schedulerAuthority[^\n]*promax/);
  assert.match(code,/let preflight = self\.preflight/);
  assert.match(code,/requires_worker && !preflight\.worker_available/);
});
