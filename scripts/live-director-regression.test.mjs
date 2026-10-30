// Source-wiring checks supplement (not replace) Rust HTTP and native A/V tests.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = path => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
const director = read('src/crates/concat/src/studio_live_director.rs');
const live = read('src/crates/concat/src/studio_live.rs');
const lip = read('src/crates/concat/src/studio_lipsync.rs');
const voice = read('src/crates/concat-studio/src/live_voice.rs');
const ui = read('src/crates/concat/ui/workspace/studio-live.slint');

test('voice samples stay on LIVE while technical and preparation controls move out', () => {
  const controls = ui.slice(ui.indexOf('title: "Điều khiển LIVE"'), ui.indexOf('title: "Xem trước"'));
  for (const label of ['Giọng mẫu', 'Lấy giọng từ video nhân vật', 'Tải giọng lên']) {
    assert.ok(controls.includes(label), label);
  }
  assert.match(controls, /StudioLive\.enroll-video-voice\(\)/);
  assert.match(controls, /StudioLive\.enroll-server-voice\(\)/);
  assert.doesNotMatch(controls, /voice-server-url|voice-server-token|voice-consent|Kết nối giọng mẫu/);
  assert.match(controls, /StudioLive\.use-sample-voice\(true\)/);
  assert.match(read('src/crates/concat/ui/workspace/studio-live-service.slint'), /export component StudioVoiceServicePanel/);
  assert.doesNotMatch(ui, /if StudioLive\.show-settings: StudioCard/);
  assert.doesNotMatch(ui, /title: "Thiết lập LIVE"|text: "AI trả lời"|text: "Chẩn đoán kết nối"/);
  assert.doesNotMatch(controls, /choose-director-clip\(index\)/);
  const studio = ui.slice(ui.indexOf('export component StudioLivePage'));
  assert.match(studio, /StudioLive\.studio-tab==1/);
  assert.match(studio, /StudioLive\.choose-director-clip\(index\)/);
  const interaction = ui.slice(ui.indexOf('title: "Tương tác"'));
  assert.match(interaction, /toggle-auto-product-reply\(\)/);
  assert.match(interaction, /toggle-auto-product-rotation\(\)/);
  assert.match(ui, /StudioLive\.ensure-lipsync\(\)/);
  const mc = read('src/crates/concat/ui/workspace/studio-opentalking.slint');
  assert.match(mc, /StudioCharacterMotionSetup/);
  assert.match(mc, /root\.choose-motion-clip\(index\)/);
  assert.match(director, /on_enroll_video_voice/);
  assert.match(director, /selected_motion\(\)/);
  assert.match(director, /info\.audio\.is_none\(\)/);
  assert.match(director, /Video nhân vật không còn tiếng · chọn video gốc có giọng/);
});

test('manual and automatic replies use existing engines through one director', () => {
  assert.match(live, /studio_live_director::bind\(app\)/);
  assert.match(live, /studio_live_director::synthesize\(text, voice\)/);
  assert.match(live, /studio_live_director::synthesize\(&reply, &voice\)/);
  assert.match(director, /TURN\.try_lock\(\)/);
  assert.match(director, /None => crate::studio_opentalking::synthesize_speech_robust/);
  assert.match(director, /None if SAMPLE_MODE\.load/);
  assert.match(lip, /studio_live_director::source\(text, &source\)/);
});

test('automatic lip sync verifies configured service without uploading on page entry', () => {
  const ensure = lip.slice(lip.indexOf('on_ensure_lipsync'), lip.indexOf('on_connect_lipsync'));
  assert.match(ensure, /!live\.get_lipsync_enabled\(\)/);
  assert.match(ensure, /!live\.get_lipsync_busy\(\)/);
  assert.match(ensure, /!live\.get_lipsync_token\(\)\.trim\(\)\.is_empty\(\)/);
  assert.match(ensure, /live\.invoke_connect_lipsync\(\)/);
  assert.doesNotMatch(ensure, /\.render\(|set_lipsync_enabled\(true\)/);
  assert.doesNotMatch(ui, /Tắt khớp môi GPU|Kết nối & bật khớp môi GPU/);
  const settings = read('src/crates/concat/ui/workspace/studio-pane.slint');
  assert.match(settings, /route == StudioRoute\.live: StudioLivePage/);
  assert.match(ui, /StudioLive\.studio-tab==6/);
  assert.match(settings, /motion-clips: StudioLive\.director-clips/);
});

test('explicit sample upload connects automatically without a separate consent gate', () => {
  assert.doesNotMatch(director, /get_voice_consent|set_voice_consent/);
  assert.match(director, /VoiceClient::connect/);
  assert.match(director, /client\.enrollment_name/);
  assert.match(director, /AudioDecoder::open/);
  assert.match(director, /duration: Some\(8\.0\)/);
  assert.match(voice, /!consent/);
  assert.match(voice, /\/v1\/voices\/enroll/);
  assert.match(voice, /\/v1\/audio\/speech/);
  assert.doesNotMatch(director, /Command::new|artcraft|artcaft/i);
});

test('offline fixture uses the same A/V completion without owning user files', () => {
  const fixture = lip.slice(lip.indexOf('pub(crate) fn play_fixture'), lip.indexOf('fn play_complete'));
  assert.match(fixture, /play_complete\(/);
  assert.doesNotMatch(fixture, /Cleanup|remove_file|\.render\(/);
  assert.match(lip, /speak_synced_file_cancellable\(&path/);
  assert.match(lip, /!current\(epoch\).*\|\|/s);
  assert.match(lip, /clock\.completed\(\)/);
  assert.match(lip, /selection_current\(selection\)/);
});
