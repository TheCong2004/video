# Native LIVE motion clips

## Use

- OpenTalking → Lịch sử → a completed video → **Dùng cho LIVE**.
- Or LIVE → **Chọn video từ máy** for an existing local MP4/MOV/MKV/WebM/AVI.
- **Bật clip chuyển động MC mẫu** uses the packaged MC motion clip, clearly
  labelled as a sample rather than a generated result or a different avatar.
- Open **Concat LIVE Output**, then capture that window in TikTok LIVE Studio.
- **Dừng video** releases the decoder and restores the selected portrait.

This is a silent looping motion clip. Original clip audio is never decoded or
played, so it does not overlap the existing AI/TTS output. Eye, head and body
motion come from the source footage. A static source video remains static.
This feature does **not** synthesize full-body movement from a photograph and
does **not** lipsync a reused video to new AI replies. Wav2Lip/real-time avatar
integration is separate and is not claimed complete by this change.

## Implementation

- Existing linked concat-media Decoder, not a browser or external video player.
- Dedicated worker, output paced to 30fps, aspect-preserving fit within 720×1280,
  at most two decoder threads. Actual speed depends on source and hardware.
- One latest-frame mailbox: no growing frame queue or preloading whole clips.
- EOF seeks to the start; no freeze on the final picture. Seam quality still
  depends on source footage; this is not motion interpolation or optical flow.
- UI consumes frames on a timer and mirrors the same image and subtitle/product
  state into the clean output's own Slint globals. No second decoder.
- Selection generations discard stale frames/downloads after stop/replacement.
- Job resolution accepts bounded safe IDs, uses the existing file or downloads
  the actual engine result; it never silently generates a demo for missing jobs.
- Selecting a different portrait on LIVE stops the previous motion clip.

## Verification

`cargo test -p concat-media --lib live_video::tests` exercises real H.264
encode/decode: changing pixels, looping after EOF, replacement, stop, missing
files. `node --test scripts/live-motion-regression.test.mjs` checks wiring.

Optional native application smoke: `CONCAT_LIVE_MOTION_SMOKE=<local moving video>`
with `Concat.exe --smoke`. Requires 12 frame changes, identical image readback
from preview and clean output, and clearing both when stopped. No TikTok
connection, AI call or posting is made by that opt-in path. A smoke test is not
a mouse-driven UI test or a proof of actual TikTok broadcast quality.
