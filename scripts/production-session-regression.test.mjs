import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import assert from 'node:assert/strict';
const read = p => readFileSync(new URL(`../${p}`, import.meta.url), 'utf8');
const source = read('src/crates/concat/src/studio_session.rs');
const ui = read('src/crates/concat/ui/workspace/studio-production.slint');

test('production uses Nexora native connectivity without a separate account flow', () => {
  assert.match(source, /WorkflowClient::via_nexora/);
  assert.match(source, /page\.profile\.id/);
  assert.doesNotMatch(source, /LoginAttempt|opener::|on_login|on_restore|complete_login/);
  const production = source.split('pub(crate) fn production_client()')[1].split('pub(crate) fn busy')[0];
  assert.doesNotMatch(production, /AuthManager/);
  assert.doesNotMatch(ui, /StudioSession\.(login|restore|logout|signed-in)|label: "Đăng nhập/);
  assert.doesNotMatch(source, /studio_local_ai::client|set_signed_in\(true/);
});

test('local connection is not a fabricated signed-in cloud user or test mode', () => {
  assert.match(source, /set_signed_in\(false\)/);
  assert.doesNotMatch(source, /test_tenant|test_user|TEST_MODE|PROMAX_AUTH_MODE/);
});

test('connection change advances the epoch so old requests cannot update another Page', () => {
  assert.match(source, /CONNECTION_GENERATION\.fetch_add\(1, Ordering::AcqRel\)/);
  const pages = read('src/crates/concat/src/studio_production_pages.rs');
  assert.match(pages, /studio_session::connection_changed\(\)/);
  assert.match(pages, /generation != crate::studio_session::session_generation\(\)/);
  assert.match(source, /cancel_pending_login/);
  assert.doesNotMatch(source, /println!|eprintln!|log::|tracing::/);
});

test('workflow refresh preserves explicit Page preference and rejects stale connections', () => {
  const bridge = read('src/crates/concat/src/studio_bridge.rs');
  assert.match(bridge, /connection_generation != crate::studio_session::session_generation\(\)/);
  assert.match(bridge, /page\.workflow_id\(\)/);
  assert.match(bridge, /index\.map_or\(-1/);
  assert.doesNotMatch(bridge, /set_workflow_index\(0\)/);
});
