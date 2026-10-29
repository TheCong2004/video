import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const read=p=>readFileSync(new URL('../src/crates/concat/'+p,import.meta.url),'utf8');
test('native graph owns its fixed engine/aspect rather than rejecting invisible stale controls',()=>{
  const native=read('src/studio_native_production.rs');
  const binding=read('src/studio_production.rs');
  assert.doesNotMatch(native,/get_aspect_index\(\) != 0|get_visual_engine_index\(\) != 1/);
  assert.match(binding,/config\["visualEngine"\] = serde_json::json!\("grok_web"\)/);
  assert.match(binding,/config\["editingPreset"\]\["aspect"\] = serde_json::json!\("9:16"\)/);
});
test('Create starts immediately and status stays in the same action card',()=>{
  const ui=read('ui/workspace/studio-production.slint');
  const action=ui.indexOf('label: ProductionStudio.busy ? "Đang chuẩn bị…"');
  const status=ui.indexOf('StudioStatus { label: ProductionStudio.status',action);
  const next=ui.indexOf('label: "← Bước trước"',action);
  assert.ok(action>=0 && status>action && status<next);
  assert.match(ui.slice(action,status),/if ProductionStudio.native-workflow \{ ProductionStudio.start-run\(\); \}/);
});
test('scene workspace uses intrinsic stacking and one owning page scroller',()=>{
  const scenes=read('ui/workspace/studio-scenes.slint');
  assert.match(scenes,/SceneWorkspace inherits StudioCard/);
  assert.doesNotMatch(scenes,/PaneScroll|panel-height|preferred-width: 2400px/);
  assert.match(read('src/studio_scenes.rs'),/static THUMBNAILS/);
  assert.match(read('src/studio_scenes.rs'),/cache.len\(\) >= 64/);
});
test('production tools are on the header and duplicate shell settings is removed',()=>{
  const pane=read('ui/workspace/studio-pane.slint');
  assert.doesNotMatch(pane,/library-open|Thu gọn thư viện/);
  const header=pane.slice(pane.indexOf('if root.route != StudioRoute.auto-shorts: FlexboxLayout'),pane.indexOf('if root.route == StudioRoute.production: StudioCard'));
  for(const route of ['scripts','workflows','jobs']) assert.ok(header.includes(`root.navigate(StudioRoute.${route})`));
  const start=read('ui/start.slint');
  assert.doesNotMatch(start,/glyph: Glyph.settings; label: "Cài đặt ứng dụng"/);
  assert.match(start,/active: root.studio-route != StudioRoute.production/);
});
test('one connection row, no repeated browser panel or project/theme shortcuts',()=>{
  const ui=read('ui/workspace/studio-production.slint');
  assert.equal((ui.match(/ProductionStudio.load-pages\(\)/g)||[]).length,2); // guarded init + single refresh
  assert.doesNotMatch(ui,/Tải lại Page từ Nexora|Tải lại quy trình|StudioBrowserPanel \{/);
  assert.match(ui,/if !ProductionStudio.pages-loaded && !ProductionStudio.busy/);
  const header=read('ui/workspace/studio-pane.slint').split('if root.route != StudioRoute.auto-shorts: FlexboxLayout')[1].split('if root.route == StudioRoute.production: StudioCard')[0];
  assert.doesNotMatch(header,/Giao diện tối|Mở dự án|Danh sách dự án/);
  assert.match(header,/← Xưởng phim AI/);
});
test('reference input remains reachable with scenes and Open shows installed Nexora',()=>{
  const ui=read('ui/workspace/studio-production.slint');
  assert.match(ui,/if ProductionStudio.current-phase == 4: StudioCard/);
  assert.ok(ui.indexOf('title: "4. Nhân vật & ảnh tham chiếu') < ui.indexOf('current-phase >= 2: SceneWorkspace'));
  assert.match(ui,/StudioBrowser.status/);
  assert.match(read('src/studio_browser.rs'),/settings\(\)\?\.show\(\)/);
  const core=readFileSync(new URL('../src/crates/concat-studio/src/nexora.rs',import.meta.url),'utf8');
  assert.match(core,/pub fn show[\s\S]*?Ok\(\(\)\) => \{\s*self.spawn_manager\(\)\?/);
  assert.match(read('ui/start.slint'),/reset-key: root.studio-scroll-epoch/);
  assert.match(read('src/studio_native_production.rs'),/preset.default_aspect_ratio = Some\("9:16".into\(\)\)/);
});
