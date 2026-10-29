import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const read=p=>readFileSync(new URL(`../${p}`,import.meta.url),'utf8');
test('LIVE breakpoints and grid widths use an allocated viewport, never their measured parent width',()=>{
  const ui=read('src/crates/concat/ui/workspace/studio-live.slint');
  assert.match(ui,/wide: root\.viewport-width >= 980px/);
  assert.match(ui,/single: root\.viewport-width < 650px/);
  assert.doesNotMatch(ui.replace(/\/\/[^\n]*/g,''),/root\.width/);
  assert.match(ui,/width: root\.single \? Math\.max\(0px, root\.viewport-width\)/);
  assert.match(ui,/StudioStatus \{ width: 144px; height: 32px; label: root\.connection-label/);
});
test('launcher and preview pass explicit width and clamp transient minimize dimensions',()=>{
  const launcher=read('src/crates/concat/ui/start.slint');
  const pane=read('src/crates/concat/ui/workspace/studio-pane.slint');
  assert.match(launcher,/viewport-width: Math\.max\(0px, root\.width - root\.rail-w - 12px\)/);
  assert.match(pane,/viewport-width: Math\.max\(0px, root\.viewport-width - 32px\)/);
  assert.match(read('src/crates/concat/ui/workspace/pane.slint'),/width: Math\.max\(0px, parent\.width - 12px\)/);
  const smoke=read('tools/studio-preview/src/main.rs');
  assert.match(smoke,/\(64,64\),\(1,1\),\(0,0\),\(1080,720\)/);
  assert.match(smoke,/LAUNCHER_RESIZE=PASS/);
  assert.match(read('scripts/ui-responsive-check.ps1'),/launcher-resize 1080 720/);
});
