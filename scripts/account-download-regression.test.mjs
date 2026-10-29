import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const read = p => readFileSync(new URL('../'+p, import.meta.url), 'utf8');
test('account selection has explicit limits, skip-existing controls and a paged thumbnail grid', () => {
  const ui = read('src/crates/concat/ui/dialogs/lux-import.slint');
  for (const label of ['10 video', '100 video', '500 video', 'Chọn 10 mới', 'Chọn 100 mới', 'Ẩn video đã có', 'Lấy danh sách']) assert.ok(ui.includes(label), label);
  assert.match(ui, /for item in StudioLux\.choices: VideoTile/);
  assert.match(ui, /disabled: item\.exists/);
  assert.match(ui, /StudioLux\.selected-count > 0/);
});
test('changing scan limit preserves the loaded gallery and selection instead of blanking it', () => {
  const src = read('src/crates/concat/src/studio_lux.rs');
  const handler = src.slice(src.indexOf('.on_limit_changed'), src.indexOf('.on_reset'));
  assert.doesNotMatch(handler, /clear_info/);
  assert.match(handler, /set_scan_limit_index/);
  const ui = read('src/crates/concat/ui/dialogs/lux-import.slint');
  assert.match(ui, /Chưa có danh sách để chọn/);
  assert.match(ui, /Các video đọc được đều đã có/);
  assert.match(src, /let browser = tiktok_account/);
});
test('saved video gallery shows thumbnails and one detail panel, not repeated path blobs', () => {
  const ui = read('src/crates/concat/ui/workspace/studio-bulk.slint');
  assert.match(ui, /for video in StudioBulk\.videos: VideoTile/);
  assert.match(ui, /thumbnail: video\.thumbnail/);
  assert.match(ui, /selected-video/);
  assert.match(ui, /if root\.show-path: Text/);
});
test('one selected video per attempt, durable provenance and no import after cancellation', () => {
  const src = read('src/crates/concat/src/studio_lux.rs');
  assert.match(src, /playlist: false/);
  assert.match(src, /video_library::record_online/);
  assert.match(src, /video_library::downloaded_sources/);
  assert.match(src, /!cancel\.load\(Ordering::Relaxed\)\s*&& !saved\.is_empty\(\)/);
  assert.match(src, /item\.exists = ok/);
  assert.match(src, /item\.selected = !ok/);
});
test('browser fallback never exports session secrets or disables the browser sandbox', () => {
  const src = read('src/crates/concat-studio/src/download_browser.rs');
  assert.match(src, /--remote-debugging-address=127\.0\.0\.1/);
  assert.match(src, /account\(source\)/);
  assert.match(src, /h == handle/);
  assert.doesNotMatch(src, /--no-sandbox|Network\.getAllCookies|Storage\.getCookies|cookies-from-browser/);
});
