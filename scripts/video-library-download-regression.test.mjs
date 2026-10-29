import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const read = p => readFileSync(new URL('../'+p, import.meta.url), 'utf8');
test('Danh sách video is a video inventory, not the CSV form', () => {
  const ui = read('src/crates/concat/ui/workspace/studio-bulk.slint');
  const panel = ui.slice(ui.indexOf('export component StudioBulkPanel'), ui.indexOf('export component StudioCsvPanel'));
  assert.match(panel, /StudioBulk\.videos/);
  assert.match(panel, /video-action\("reveal"/);
  assert.match(panel, /video-action\("import"/);
  assert.doesNotMatch(panel, /csv-content|Dữ liệu CSV/);
});
test('completed native and storyboard videos are indexed without relocating originals', () => {
  for (const name of ['studio_native_production.rs', 'studio_scenes.rs'])
    assert.match(read('src/crates/concat/src/'+name), /studio_video_library::record/);
  assert.match(read('src/crates/concat/src/studio_lux.rs'), /video_library::record_online/);
  assert.doesNotMatch(read('src/crates/concat-studio/src/video_library.rs').split('#[cfg(test)]')[0], /remove_dir_all|fs::copy/);
});
test('URL dialog exposes cancellation during extraction and download', () => {
  const ui = read('src/crates/concat/ui/dialogs/lux-import.slint');
  assert.match(ui, /StudioLux\.downloading \|\| StudioLux\.busy \? "Hủy"/);
  assert.match(ui, /StudioLux\.cancel-download\(\)/);
  const native = read('src/crates/concat/src/studio_lux.rs');
  assert.match(native, /on_cancel_download/);
  assert.match(native, /project_epoch\(\) == epoch/);
  assert.doesNotMatch(native, /bảo vệ bản quyền âm thanh/);
});
test('download output is per-attempt and the packaged app contains both engines', () => {
  const adapter = read('src/crates/concat-studio/src/lux.rs');
  assert.match(adapter, /download-\{:032x\}/);
  assert.doesNotMatch(adapter, /initial_files|Pre-cleanup/);
  assert.match(adapter, /JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE/);
  assert.match(read('scripts/windows-package.ps1'), /'lux.exe', 'yt-dlp.exe'/);
});
