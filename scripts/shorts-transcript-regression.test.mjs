import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = path => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
const base = 'src/crates/concat/';

test('Transcript workbench stays available even when automatic picks are empty', () => {
  const ui = read(base + 'ui/workspace/auto-shorts.slint');
  const area = ui.slice(ui.indexOf('title: "Cắt theo lời thoại"'), ui.indexOf('if AutoShorts.result-count > 0: StudioHeading'));
  for (const action of ['transcript-search', 'transcript-paginate', 'transcript-select', 'transcript-save', 'transcript-history', 'transcript-create', 'transcript-export']) {
    assert.ok(area.includes(`AutoShorts.${action}(`), action);
  }
  assert.match(ui, /if AutoShorts\.analyzed: StudioCard/);
  assert.doesNotMatch(area, /AutoShorts\.result-count > 0/);
  assert.match(area, /không đổi tiếng nói/);
  assert.match(area, /lưu bản SRT/i);
});

test('New callbacks are bound, source changes discard the old review and corrected evidence loses approval', () => {
  const native = read(base + 'src/auto_shorts.rs');
  const review = read(base + 'src/auto_shorts_transcript.rs');
  assert.match(native, /transcript::bind\(app\)/);
  assert.equal((native.match(/state\.transcript_review = transcript::Workbench::default\(\)/g) || []).length, 2);
  assert.match(native, /Workbench::new\(review\)/);
  for (const action of ['search', 'paginate', 'select', 'save', 'history', 'create', 'export']) assert.ok(review.includes(`on_transcript_${action}(`));
  assert.match(review, /state\.selected\.clear\(\)/);
  assert.match(review, /h\.needs_review = true/);
  assert.match(review, /create_new\(true\)/);
  assert.match(review, /fingerprint\.unchanged\(\)/);
  assert.match(review, /const PAGE: usize = 20/);
  assert.doesNotMatch(review, /ai_structured|commit_draft|selected\.insert/);
});

test('Repeated-take scan is explicit, off-thread, bounded, invalidated by correction and never deletes clips', () => {
  const ui = read(base + 'ui/workspace/auto-shorts.slint');
  const native = read(base + 'src/auto_shorts_transcript.rs');
  const core = read('src/crates/concat-agent/src/transcript.rs');
  assert.match(ui, /AutoShorts\.find-repetitions\(\)/);
  assert.match(ui, /không kết luận bản nào tốt hơn và không tự xóa/);
  assert.match(native, /on_find_repetitions/);
  assert.match(native, /Review::new\(cues, duration\)\?\.repetitions\(\)/);
  assert.match(native, /transcript_review\.repetitions\.clear\(\)/);
  assert.match(native, /transcript_review\.repetitions_scanned = false/);
  assert.match(core, /hits\.len\(\) == 2000/);
  assert.doesNotMatch(native, /Command::Remove|Command::Trim|commit_draft/);
  assert.match(ui, /height: AutoShorts\.transcript-rows\.length > 0 \? 300px/);
});
