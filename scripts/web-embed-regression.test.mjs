import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = path => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');

test('NexPulse is a large launcher route, not an external browser shortcut', () => {
  const rail = read('src/crates/concat/ui/start.slint');
  assert.match(rail, /label: "NexPulse"; glyph:/);
  assert.match(rail, /root\.studio-picked\(StudioRoute\.web\)/);
  assert.match(rail, /if root\.studio-route == StudioRoute\.web: StudioWebPage/);
  assert.match(rail, /if root\.studio-route != StudioRoute\.web && root\.studio-route != StudioRoute\.hotclip: PaneScroll/);
  assert.match(rail, /StudioWeb\.hide-native\(\)/);
});

test('web is unprivileged and restricted to the supplied public HTTPS origin', () => {
  const native = read('src/crates/concat/src/studio_web.rs');
  assert.match(native, /https:\/\/deloy-internet-frontend\.vercel\.app\/launches/);
  assert.match(native, /build_as_child\(window\)/);
  assert.match(native, /with_navigation_handler/);
  assert.match(native, /with_permission_handler\(\|_\| wry::PermissionResponse::Deny\)/);
  assert.match(native, /with_download_started_handler\(\|_, _\| false\)/);
  assert.match(native, /text\.chars\(\)\.take\(256\)/);
  assert.doesNotMatch(native, /\.with_ipc_handler\(|\.with_initialization_script\(|opener::|Command::|disable-web-security|ignore-certificate-errors/);
});

test('native child follows viewport and hides for editor, sheets, route changes and minimize', () => {
  const native = read('src/crates/concat/src/studio_web.rs');
  const ui = read('src/crates/concat/ui/workspace/studio-web.slint');
  const app = read('src/crates/concat/ui/app.slint');
  assert.match(ui, /native-frame\.absolute-position\.x/);
  assert.match(ui, /native-frame\.height/);
  assert.match(native, /get_web_host_allowed\(\)/);
  assert.match(native, /is_minimized\(\)/);
  assert.match(native, /set_bounds\(bounds\)/);
  assert.match(native, /set_visible\(false\)/);
  assert.match(app, /web-host-allowed: root\.on-start && !root\.sheet-open && !root\.confirm\.open && !root\.relink\.open && root\.open-menu < 0/);
  assert.match(app, /changed web-host-allowed/);
  assert.match(app, /if root\.sheet-open \{ StudioWeb\.hide-native\(\); StudioHotclip\.hide-native\(\); \}/);
});
