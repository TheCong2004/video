import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const read=p=>readFileSync(new URL(`../${p}`,import.meta.url),'utf8');

test('Nexora profiles, not a second local Page catalogue, determine the visible rows',()=>{
  const code=read('src/crates/concat/src/studio_production_pages.rs');
  assert.match(code,/client\.profiles\(\)/);
  assert.match(code,/catalogue\(profiles, presets\)/);
  assert.match(code,/p\.profile\.name/);
  assert.doesNotMatch(code,/on_start_run|on_check_plan|ProductionClient|workers\/.*dispatch/);
});

test('linked presets do not fabricate a profile, output path or server workflow',()=>{
  const code=read('src/crates/concat-studio/src/production_pages.rs');
  assert.match(code,/browser_profile_id/);
  assert.match(code,/linked\.remove\(&profile\.id\)/);
  assert.doesNotMatch(code,/ContentPageStore::.*save|ContentPageDraft/);
  const ui=read('src/crates/concat/src/studio_production_pages.rs');
  assert.match(ui,/set_workflow_ids\(Default::default\(\)\)/);
  assert.match(ui,/set_workflow_index\(found.map_or\(-1/);
});

test('native UI connects and refreshes without registration or test-mode controls',()=>{
  const ui=read('src/crates/concat/ui/workspace/studio-production.slint');
  assert.match(ui,/StudioBrowser\.start\(\)/);
  assert.match(ui,/ProductionStudio\.load-pages\(\)/);
  // Page refresh owns workflow refresh; the removed duplicate UI control must not return.
  assert.match(read('src/crates/concat/src/studio_production_pages.rs'),/invoke_load_workflows\(\)/);
  assert.doesNotMatch(ui,/StudioSession\.(login|restore|signed-in)/);
  assert.match(ui,/ProductionStudio\.page-summary/);
});
