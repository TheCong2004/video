import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read=path=>readFileSync(new URL(`../${path}`,import.meta.url),'utf8');
test('Hotclip has exactly one native editor destination, not a duplicate launcher',()=>{
  const start=read('src/crates/concat/ui/start.slint');
  assert.doesNotMatch(start,/text: "HOTCLIP"|StudioRoute\.hotclip: HotclipPane/);
  assert.doesNotMatch(start,/StudioHotclip\.active = root\.hotclip-selected/);
  const media=read('src/crates/concat/ui/workspace/media-pane.slint');
  assert.match(media,/text: "Hotclip"/);
  assert.match(media,/root\.tab = 8/);
  assert.match(media,/HotclipPane\s*\{[\s\S]*?visible: root\.tab == 8/);
  assert.match(media,/visible: root\.tab != 7 && root\.tab != 8/);
  assert.match(read('src/crates/concat/ui/app.slint'),/StudioHotclip\.hide-native\(\)/);
});
test('legacy Electron host is explicit opt-in, not normal startup',()=>{
  const lib=read('src/crates/concat/src/lib.rs');
  assert.match(lib,/studio_hotclip::bind\(&app, std::env::var_os\("CONCAT_HOTCLIP_SMOKE"\)\.is_some\(\)\)/);
  assert.match(lib,/let result = app\.run\(\);\s*studio_hotclip::shutdown\(\)/);
  const core=read('src/crates/concat-studio/src/hotclip.rs');
  assert.match(core,/node_modules\/electron\/dist\/electron\.exe/);
  assert.match(core,/TcpListener::bind\("127\.0\.0\.1:0"\)/);
  assert.match(core,/stream\.set_nonblocking\(false\)/);
  assert.match(core,/command\.env_clear\(\)/);
  assert.match(core,/JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE/);
  assert.match(core,/value\["nonce"\] != nonce/);
  assert.match(core,/value\["pid"\]/);
  assert.doesNotMatch(core,/taskkill|cmd\.exe|powershell\.exe|dev:web|npm install/i);
});

test('native Hotclip binds real analysis, transcript, timeline and export actions',()=>{
  const pane=read('src/crates/concat/ui/workspace/hotclip-pane.slint');
  assert.doesNotMatch(pane,/StudioHotclip|WebView|iframe|Electron/);
  for(const action of ['choose-source','analyze','cancel','transcript-search','transcript-select','transcript-save','transcript-history','transcript-create','find-repetitions','preview','stage','export-selected','toggle-watch']) {
    assert.ok(pane.includes(`AutoShorts.${action}(`),`missing real action ${action}`);
  }
  assert.match(pane,/Theme\./);
  assert.match(pane,/PaneScroll/);
});
test('native Hotclip uses shared palette and guarded transcript undo/redo',()=>{
  const pane=read('src/crates/concat/ui/workspace/hotclip-pane.slint');
  assert.doesNotMatch(pane, /#[0-9a-f]{3,8}\b/i);
  for (const [state,direction] of [['undo','false'],['redo','true']]) {
    assert.match(pane,new RegExp(`enabled: !AutoShorts\\.busy && AutoShorts\\.transcript-${state};\\s*clicked => \\{ AutoShorts\\.transcript-history\\(${direction}\\);`));
  }
  assert.match(pane,/text: AutoShorts\.source-label;\s*color: Theme\.fg/);
  assert.doesNotMatch(pane,/row\.score\s*:\s*"90"/);
});
test('only owned child HWND is attached and clipped to host bounds',()=>{
  const host=read('src/crates/concat/src/studio_hotclip.rs');
  assert.match(host,/GetWindowThreadProcessId/);
  assert.match(host,/actual_pid!=host\.pid/);
  assert.match(host,/SetParent\(hwnd,parent/);
  assert.match(host,/SWP_NOACTIVATE\|SWP_NOZORDER/);
  assert.match(host,/get_web_host_allowed\(\).*minimized/);
  assert.match(host,/set_minimized\(true\)/);
});

test('native visibility also reaches the real Electron widget lifecycle',()=>{
  const core=read('src/crates/concat-studio/src/hotclip.rs');
  const host=read('src/crates/concat/src/studio_hotclip.rs');
  const electron=readFileSync(new URL('../../hotclip/src/main/index.ts',import.meta.url),'utf8');
  assert.match(core,/pub fn set_visible\(&self, visible: bool\)/);
  assert.match(core,/Control::Visible\(show\) => visibility = Some\(show\)/);
  assert.match(host,/if host\.attached.*process\.set_visible\(show\)/);
  assert.match(electron,/command === "show"[\s\S]*?presentConcatWindow\(mainWindow\);[\s\S]*?webContents\.invalidate\(\)/);
  assert.match(electron,/command === "hide".*mainWindow\.hide\(\)/);
});

test('native layout repairs actual child drift even when desired bounds stay cached',()=>{
  const host=read('src/crates/concat/src/studio_hotclip.rs');
  assert.match(host,/GetWindowRect/);
  assert.match(host,/inner_position\(\)/);
  assert.match(host,/actual[^\n]*!=Some\(bounds\)/);
  assert.match(host,/HOTCLIP_GEOMETRY_SMOKE=PASS/);
});
