// SPDX-License-Identifier: AGPL-3.0-or-later
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = name => readFileSync(new URL(`../${name}`, import.meta.url), 'utf8');

test('history routes real completed jobs to LIVE, not the external media player', () => {
  const ui = read('src/crates/concat/ui/workspace/studio-opentalking.slint');
  assert.match(ui, /use-live-video\(job\.id\); root\.navigate-to-live\(\)/);
  const native = read('src/crates/concat/src/studio_live_video.rs');
  assert.match(native, /job\.status == "completed"/);
  assert.match(native, /resolve_live_video\(&id\)/);
  assert.doesNotMatch(native, /ensure_job_video|create_job_video/);
});

test('preview and clean output both prioritize the decoded video frame', () => {
  for (const file of ['studio-live.slint', 'live-output-window.slint']) {
    const ui = read(`src/crates/concat/ui/workspace/${file}`);
    assert.match(ui, /if StudioLive\.video-frame\.width > 0: Image/);
    assert.match(ui, /source: StudioLive\.video-frame/);
    assert.match(ui, file === 'studio-live.slint'
      ? /StudioLive\.video-frame\.width == 0 && root\.has-avatar && root\.selected-preview\.width > 0/
      : /StudioLive\.video-frame\.width <= 0 && StudioOpenTalking\.avatar-preview/);
  }
  const native = read('src/crates/concat/src/studio_live.rs');
  assert.match(native, /output\.set_video_frame\(source\.get_video_frame\(\)\)/);
  assert.match(native, /output\.set_displayed_subtitle\(source\.get_displayed_subtitle\(\)\)/);
});

test('motion is silent, bounded, cancellation-aware, and not advertised as realtime lipsync', () => {
  const worker = read('src/crates/concat-media/src/live_video.rs');
  assert.match(worker, /latest: Arc<Mutex<Option<Update>>>/);
  assert.match(worker, /u\.generation == self\.generation/);
  assert.match(worker, /seek\(Rational::ZERO\)/);
  assert.doesNotMatch(worker, /AudioDecoder|speak_file|Command::new/);
  const ui = read('src/crates/concat/ui/workspace/studio-live.slint');
  assert.match(ui, /Không khớp môi theo lời AI/);
});
