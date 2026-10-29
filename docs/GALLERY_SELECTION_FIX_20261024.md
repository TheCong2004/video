# Video chooser and saved-video gallery — 2026-10-24

## User-visible change

- Account results are a paged thumbnail grid (20 per page), not an empty conditional area or a list of raw URLs. Select individual videos, the first 10/100 new videos, or clear the selection. Existing local videos are hidden by default and cannot be queued twice.
- Changing the requested scan limit no longer clears already loaded rows or selections. The new limit takes effect on the next explicit scan; identical URL edits also preserve results.
- One “Lấy danh sách” action automatically opens/reuses Concat's isolated browser for a TikTok account. No separate second browser-fetch button or Concat sign-in is needed.
- Empty, loading, all-already-downloaded and verification/error states are explicit. The scan/download cancel action stays outside the scrolling body.
- “Danh sách video” shows actual cached video frames and duration in a 24-item-per-page grid. Selecting a tile updates one detail panel with play/import/reveal actions. Full file paths are optional detail, not repeated large cards.

## Implementation constraints

Thumbnail fetch, bounded image decoding and FFmpeg frame extraction happen off the UI thread. Images are at most 320×180, CDN responses at most 4 MB, and image decode limits are applied before allocation. No remote thumbnail redirects are followed; only HTTPS cover CDNs are allowed. Local extraction has an 8-second timeout and uses one decoder/extractor at a time. Current-page model rows are updated individually. Library pixels outside the current page are released; a disk cache avoids decoding again.

Actual TikTok covers in this session used AVIF (the first bytes contain `ftypavif`), not JPEG/WebP. A bounded AVIF-only FFmpeg fallback was added: inspect spatial extents before decoding, cap input size and dimensions, use one decoder thread, an allocation bound and timeout, and cache a PNG. Unknown formats do not enter that fallback. The first attempted main build was deliberately stopped before delivery after this missing format was identified.

The account collector reads only the exact requested public TikTok account in an app-owned browser profile. Its loopback debugging endpoint must match the browser identity recorded in that profile. No cookies or personal browser session are exported, and no CAPTCHA is bypassed.

Original video files, provenance, duplicate detection, cancellation and project-epoch import guards are preserved. UI QA fixtures are not inserted into the user's library.

## Verification recorded so far

- Core adapter suite: **213 passed, 0 failed, 6 explicitly ignored** (`cargo test -p concat-studio --lib -- --test-threads=1`).
- Focused account/library and production regression checks: **23 passed** (six Node test files).
- Isolated native Slint components compiled. Pointer-event checks reached select/download-cancel/import actions.
- Rendered real-frame chooser/library captures: `.runtime/download-ui-check/gallery-selection-real.png`, `gallery-library-real.png`. The repeated image in these layout fixtures is deliberately the same frame from an existing QA MP4, not a claimed live account scan.
- Actual TikTok account scan in this session encountered a slider verification challenge and returned no video links. This is **not** recorded as an end-to-end live account success. User verification is required; there is no forced bypass or fabricated gallery.
- Follow-up after the account feed became visible: existing account-scan diagnostic successfully enumerated **10 and 100 distinct public links**, each with a public cover URL. The first cover downloaded successfully (6,513 bytes). These are live adapter diagnostic receipts, not yet a receipt for the newly built main EXE. No media was downloaded in those scans.
- `tools/runtime-checks/video-preview.rs` compiled the **exact** `studio_video_preview.rs` module against the application's existing dependencies. It passed actual AVIF decoding, actual local MP4 frame/duration extraction, bounded pixel dimensions and repeat-cache pixel equality. The fixture MP4 SHA-256 stayed `40F085A4FDD8067BAB3D795BB8A3C813BD127AB1EF26B8A4C43EABCD63FD631A` before and after.
- Follow-up through the exact `remote` thumbnail function: **all 10 live public account covers decoded**, producing **10 distinct pixel hashes**. `tools/runtime-checks/browser-covers.mjs` supplies only public cover metadata from the explicitly owned account-browser endpoint; no session/cookie data or video downloads are involved. One QA command initially failed because its ad-hoc JavaScript expression was malformed; the checked-in constant expression replaced it and the rerun passed.
- Native components rechecked at a real **1080×720** viewport with the decoded AVIF cover: selection, in-progress cancellation, library import action, empty and all-existing states passed. PNGs are under `.runtime/download-ui-check/*-720.png`. The footer remains visible outside the scrolling gallery. These layout fixtures repeat one image deliberately and do not claim distinct account photos.

## Native artifact

- `scripts/windows-studio.ps1 -Build`: **exit 0**, 20m36s for the final build.
- Updated existing dev executable: `D:/capcutpolot/concat/src/target/debug/concat.exe`, **273,000,960 bytes**.
- SHA-256: `143AC68D2475CB75C8E8F231D8CF07D8C67CA0513CAB7035DBBEDE32848B881A`.
- The exact new EXE ran hidden with `--smoke`, `CONCAT_DOWNLOAD_SMOKE=1`, an explicit actual AVIF cover and existing MP4 fixture: **exit 0**. Fresh log: `.runtime/gallery-native-smoke/app.concat.editor/logs/concat-20261023-204932.log`.
- That log records PASS for actual CDN cover decoding, actual MP4 frame/duration/cache, the 24-card model and selected-video detail, actual bound select-10/100 callbacks, hide-existing, duplicate/busy guards and reset, then `smoke: ok`.
- Test AppData/TEMP were isolated from the user's catalog and projects. No normal Concat process was left running. The old Concat PID 6188 was stopped only after explicit user approval; Nexora was not stopped or rebuilt.
- A subsequent optional combined **new-EXE live account callback** launch was rejected by the host execution policy before process creation. It was **not retried via another mechanism** and is not recorded as passed. Live 10/100 enumeration and all ten actual cover decodes were independently verified above, while the exact EXE's bound callbacks/local decode checks passed. This distinction is intentional: no claim that the policy-blocked combined live check ran.
- This is the existing **dev EXE**, not a new NSIS installer. Launch with `scripts/windows-studio.ps1 -RunBuilt`; no Nexora reinstall is needed.

## Delivery self-review

Overall **4.0/5** against the reported blank chooser and nonvisual saved-video library.

| Axis | Score | Evidence / remaining improvement |
| --- | --- | --- |
| Accuracy | 4 | Core 213/0/6 and 23 regression checks, live 10/100 rows, ten distinct decoded public covers, exact native smoke exit 0. Combined exact-EXE live callback was policy-blocked; do not present it as passed. |
| Completeness | 4 | Both requested grids, selection preservation, explicit empty/error states, cached actual video previews and duplicate handling delivered. A normal interactive user-session walkthrough remains outside the automated receipts. |
| Clarity | 4 | One list-fetch action, explicit verification/all-existing states, one selected-video action panel; signed cover URLs and full file paths are not displayed as large repeated cards. Further user feedback can refine card density. |
| Actionability | 4 | Existing dev EXE replaced and its hash/log recorded; direct launch command supplied. This is not an installer rebuild. |
| Conciseness | 4 | Short user handoff; detailed evidence stays here. Native build was slow, so progress messages were necessarily repetitive. |

Top improvement: complete the optional combined live callback/normal interactive walkthrough when execution policy permits. No critical axis ≤2. Self-check: the user's concern is real visible video selection, not merely a passing parser; the UI image/pointer receipts and native callback tests address that concern while the remaining limitation is stated. Verdict: deliver with the verification boundary above, without claiming universal platform success.
