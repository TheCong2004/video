import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read=path=>readFileSync(new URL(`../${path}`,import.meta.url),'utf8');
const root='src/crates/concat/';

test('Overview is removed, LIVE is the default and advanced tools remain reachable',()=>{
 const start=read(root+'ui/start.slint');
 const pane=read(root+'ui/workspace/studio-pane.slint');
 assert.doesNotMatch(start+pane,/StudioRoute\.overview|label: "Tổng quan"/);
 assert.match(start,/studio-route: StudioRoute\.live/);
 for(const route of ['scripts','workflows','jobs']) assert.ok(pane.includes(`root.navigate(StudioRoute.${route})`));
 assert.doesNotMatch(pane,/library-open/);
 assert.match(pane,/root\.route == StudioRoute\.production: StudioButton \{ label: "Tác vụ"/);
});

test('Too-short source is blocked both on the UI and before opening an AI client',()=>{
 const ui=read(root+'ui/workspace/auto-shorts.slint');
 const native=read(root+'src/auto_shorts.rs');
 assert.match(ui,/source-duration < AutoShorts\.min-duration/);
 assert.match(ui,/!root\.duration-blocked/);
 assert.match(ui,/Chọn video dài hơn hoặc giảm độ dài bên dưới/);
 const worker=native.slice(native.indexOf('// Re-probe even watched/changed files'));
 const check=worker.indexOf('source_problem(inspected.duration, cfg.min_duration)');
 const client=worker.indexOf('crate::studio_local_ai::client()');
 assert.ok(check>=0 && client>check,'fresh duration validation must precede the AI client');
 assert.match(native,/source_problem\(source\.duration, cfg\.min_duration\)/);
});

test('Full diagnostic warnings remain available without being appended to the main result',()=>{
 const native=read(root+'src/auto_shorts.rs');
 const ui=read(root+'ui/workspace/auto-shorts.slint');
 assert.match(native,/set_diagnostics\(analysis\.warnings\.join\("\\n"\)\.into\(\)\)/);
 assert.match(native,/set_analysis_note\(/);
 assert.match(ui,/if root\.diagnostics-open && AutoShorts\.diagnostics != ""/);
 assert.match(ui,/if AutoShorts\.analysis-note != ""/);
 assert.doesNotMatch(native,/Chưa chọn, dựng hoặc xuất tự động\. \{\}/);
});

test('Selection, review, export, cancel and optional AI boundaries remain explicit',()=>{
 const ui=read(root+'ui/workspace/auto-shorts.slint');
 for(const action of ['choose-source','choose-transcript','analyze','retry','cancel','toggle-selected','preview','stage','export-selected','choose-output','open-output','choose-watch','toggle-watch','refresh-results']) assert.ok(ui.includes(`AutoShorts.${action}(`),action);
 assert.match(ui,/dùng model và hạn mức đã cấu hình/);
 assert.match(ui,/Xuất video không tự đăng bài/);
 assert.match(ui,/AutoShorts\.selected-count > 0 && AutoShorts\.output-label != ""/);
});
