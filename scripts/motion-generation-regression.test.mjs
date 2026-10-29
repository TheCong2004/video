import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = path => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
test('generation is an explicit production action with duration and rights confirmation', () => {
  const ui = read('src/crates/concat/ui/workspace/studio-opentalking.slint');
  assert.match(ui, /Số giây mỗi clip cử chỉ/);
  assert.match(ui, /StudioOpenTalking\.gesture-seconds = value/);
  assert.match(ui, /StudioOpenTalking\.generate-gestures\(\)/);
  assert.match(ui, /gesture-consent && StudioOpenTalking\.gesture-image-name/);
  assert.doesNotMatch(read('src/crates/concat/ui/workspace/studio-live.slint'), /generate-gestures\(\)/);
});
test('whole pack is verified, saved and assigned; failed pack never replaces old profile', () => {
  const native = read('src/crates/concat/src/studio_motion_generation.rs');
  assert.match(native, /client\.readiness\(\)/);
  assert.match(native, /client\.render\(&input\.image, &audio\.bytes, &audio\.media_type, role, seconds, true\)/);
  assert.match(native, /install_generated\(files\)/);
  assert.match(native, /info\.audio\.is_some\(\)/);
  assert.match(native, /if !self\.committed/);
  assert.match(native, /get_session_active\(\)/);
  assert.doesNotMatch(native, /artcraft|artcaft|Command::new/i);
});

test('creation hides operator connection fields while settings retain the same secure inputs', () => {
  const ui = read('src/crates/concat/ui/workspace/studio-opentalking.slint');
  const settings = read('src/crates/concat/ui/workspace/studio-live-service.slint');
  assert.doesNotMatch(ui, /value: StudioOpenTalking\.gesture-(?:url|token)/);
  assert.doesNotMatch(ui, /EchoMimic|HTTPS|SSH tunnel/);
  assert.match(settings, /value: StudioOpenTalking\.gesture-url/);
  assert.match(settings, /secret: true; value: StudioOpenTalking\.gesture-token/);
  const shell = read('src/crates/concat/ui/workspace/studio-pane.slint');
  assert.match(shell, /if root\.route == StudioRoute\.settings: StudioMotionServicePanel/);
  const native = read('src/crates/concat/src/studio_motion_generation.rs');
  assert.match(native, /Tính năng tạo cử chỉ chưa được thiết lập/);
  assert.doesNotMatch(native, /Nhập URL|kiểm tra server EchoMimic/);
});
test('silent video creation uses requested duration and actual artifact duration', () => {
  const native = read('src/crates/concat/src/studio_opentalking.rs');
  assert.match(native, /retime_silent\(file_path, seconds\)/);
  assert.match(native, /get_duration_seconds\(\)/);
  assert.match(native, /duration: format!\("\{duration:\.1\}s"\)/);
  assert.doesNotMatch(native, /duration: "0:30"/);
});
test('upstream frame budget is driven by seconds, output is silent and exact', () => {
  const engine = read('integrations/motion/engine.py');
  assert.match(engine, /"--video_length", str\(frames\)/);
  assert.match(engine, /"-an", "-t", str\(seconds\)/);
  assert.match(engine, /HF_HUB_OFFLINE="1"/);
  assert.doesNotMatch(engine, /shell=True|pip install|git clone/);
});
