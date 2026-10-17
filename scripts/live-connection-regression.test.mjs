import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = path => readFileSync(new URL('../' + path, import.meta.url), 'utf8');

test('inspection waits for a correlated response instead of treating stdin write as a room result', () => {
  const native = read('src/crates/concat/src/studio_live.rs');
  assert.match(native, /let request = runtime\.inspect\(&user\)\?/);
  assert.match(native, /Some\(request\.as_str\(\)\)/);
  assert.match(native, /LiveFrameKind::Result \| LiveFrameKind::CommandError/);
  assert.match(native, /Instant::now\(\) < deadline/);
});

test('event monitoring survives an ArtCraft automatic reconnect', () => {
  const native = read('src/crates/concat/src/studio_live.rs');
  assert.match(native, /let terminal = matches!\(status.state.as_str\(\), "stopped" \| "failed"\)/);
  assert.doesNotMatch(native, /let terminal\s*=\s*matches!\(status.state.as_str\(\), "disconnected"/);
  assert.match(native, /connector_error\(&code\)/);
  assert.match(native, /last_status.as_ref\(\) != Some\(&status\)/);
  assert.doesNotMatch(native, /if terminal \|\| weak\.upgrade\(\)\.is_none\(\)/);
  assert.match(native, /if terminal \|\| weak\.upgrade_in_event_loop\(\|_\| \{\}\)\.is_err\(\)/);
  const ui = read('src/crates/concat/ui/workspace/studio-live.slint');
  assert.match(ui, /session-active:.*state == "degraded".*state == "disconnected"/);
  assert.match(ui, /if !StudioLive.session-active: StudioButton/);
  const supervisor = read('src/crates/concat-studio/src/live_runtime.rs');
  assert.match(supervisor, /"connecting" \| "connected" \| "degraded" \| "disconnected" =>/);
});

test('development discovery includes owned staged resources and packaged discovery remains self-contained', () => {
  const native = read('src/crates/concat-studio/src/live_runtime.rs');
  assert.match(native, /if cfg!\(debug_assertions\) \{\s*resource_roots.push\(base.join\(".deps"\)\)/);
  assert.match(native, /code.len\(\) <= 80/);
  assert.match(native, /b.is_ascii_uppercase\(\)/);
});
