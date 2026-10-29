import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const ui=readFileSync(new URL('../src/crates/concat/ui/workspace/studio-production.slint',import.meta.url),'utf8');
const native=readFileSync(new URL('../src/crates/concat/src/studio_production.rs',import.meta.url),'utf8');
test('all seven production configuration steps remain directly reachable',()=>{
  for(let step of [1,5,6,7]) assert.match(ui,new RegExp(`current-phase == ${step}: StudioCard`));
  assert.match(ui,/current-phase >= 2: SceneWorkspace/);
  assert.match(ui,/current-phase == 3 && ProductionScenes.count == 0: StudioCard/);
  assert.match(ui,/current-phase == 4: StudioCard/);
  assert.match(ui,/current-phase = index \+ 1/);
  assert.match(ui,/current-phase -= 1/);
  assert.match(ui,/current-phase \+= 1/);
  assert.doesNotMatch(ui,/min-width: (320|360|680)px/);
});
test('restored controls invalidate the prepared production plan',()=>{
  for(const field of ['script','asset-urls','character-anchor','visual-style','image-prompt','expand-prompt','motion-prompt','voice-id']) {
    assert.match(ui,new RegExp(`ProductionStudio\\.${field} = text; ProductionStudio\\.invalidate\\(\\)`));
  }
  assert.match(ui,/current <=> ProductionStudio\.visual-engine-index;[^}]+ProductionStudio\.invalidate/);
  assert.match(native,/current == prepared/);
  assert.match(native,/fields\.contains_key\("production_settings"\)/);
  assert.match(native,/let mut config = settings\(app\)/);
  assert.match(native,/config\["pageSnapshot"\] = serde_json::to_value\(page\)/);
  assert.match(native,/fields.insert\("production_settings".into\(\), config\)/);
});
test('local anchor is previewed and never masquerades as remote source media',()=>{
  assert.match(native,/production-anchor/);
  assert.match(native,/limits\.max_alloc/);
  assert.match(native,/if !state\.get_anchor_label\(\)\.is_empty\(\) && workflow != concat_studio::native_pipeline::WORKFLOW\s*\{\s*return Err\(RunError::NotReady\)/);
  // Only the reserved native worker may resolve local references; generic
  // remote workflows still cannot receive a local path as an uploaded URL.
  assert.match(native,/studio_native_production::augment/);
  assert.match(native,/localAnchorPath/);
  assert.match(ui,/chưa tải lên dịch vụ/);
});
test('export is an explicit local configuration action, not a fake render',()=>{
  assert.match(ui,/ProductionStudio\.export-settings\(\)/);
  assert.match(native,/concat-production-config/);
  assert.match(native,/"execution": "not-submitted"/);
  assert.match(native,/production-export/);
  assert.match(ui,/!ProductionStudio\.uncertain/);
  assert.match(ui,/ProductionStudio\.native-workflow \? ProductionStudio\.page-index > 0 : ProductionStudio\.ready/);
  assert.match(ui,/StudioRunList \{ \}/);
});
