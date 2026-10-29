# Account video selection — 24 October 2026

## Final handoff — verified native EXE

- `scripts/windows-studio.ps1 -Build` completed with **exit 0**, dev build 19m16s.
- Updated artifact: `D:/capcutpolot/concat/src/target/debug/concat.exe`,
  **271,045,632 bytes**, local timestamp **2026-10-24 02:38:33**.
- SHA256: `4BB4A1DA4A64B6C6F9569476AB4BDA5377CB27CDEB7CE52A44DDE79D2F1F3ADD`.
- The exact new executable ran hidden with `--smoke`, `CONCAT_DOWNLOAD_SMOKE=1`
  and `CONCAT_STORYBOARD_SMOKE=1`: **exit 0**. Actual bound download callbacks
  passed select 10/100, hide-existing, blocked duplicate selection, busy guard,
  clear/reset and switching profile → individual video URL. Existing storyboard
  smoke also passed. No network/download/provider calls or user-project mutation.
- Fresh native log: `C:/Users/thecong/AppData/Local/app.concat.editor/logs/concat-20261023-193933.log`
  (filename is UTC; local time is 24 October 02:39). Contains both smoke PASS lines
  and `smoke: ok`.
- This updates the existing **dev EXE**, not a separate NSIS installer or Nexora.
  Launch it using `scripts/windows-studio.ps1 -RunBuilt`. The smoke closed itself;
  no normal user session was left running.

## Scope and current verification

The existing URL download dialog now enumerates a channel/profile before downloading.
Users select individual videos or the first 10/100 **not already present**. Enumeration
is bounded to 10/100/500 entries. The virtual list hides existing files by default;
showing them labels and disables their selection rather than permitting duplicates.

The queue uses a separate downloader attempt per selected video, continues after an
individual failure, retains completed videos on cancellation, and keeps failed or
unprocessed entries selectable. Completed output is media-probed, registered with
source URL in the existing video library, then optionally imported into the unchanged
open project. Cancellation/project switching prevents automatic import but does not
remove completed files from the library. No login to Concat or Promax is introduced.

## Actual network evidence

The direct HTTP request for `https://www.tiktok.com/@oclon2026` returned HTTP 200 but
only a 1,462-byte SlardarWAF verification page (no hydration metadata). This is not
evidence that the account is private.

An explicit **Lấy qua trình duyệt** action now reads rendered profile links in an
isolated Concat-owned Edge/Chrome profile. It does not export cookies, read personal
browser stores, bypass challenges, disable the sandbox, or control Nexora/Grok tabs.
It reuses an exact-account tab in its own profile. If verification is needed, the user
completes it in the visible browser. Only HTTPS video links belonging to the requested
TikTok handle are accepted; scroll/wait/cancel are bounded. The browser remains open
and can be closed by the user. Its profile is reused, not recreated on each scan.

Verified with the real Rust adapter/example:

- `account-scan.exe browser https://www.tiktok.com/@oclon2026 D:/capcutpolot/concat/.runtime/account-browser-check 25`
  returned **10 distinct public video links**, exit 0.
- The same command with `90 100` returned **100 distinct public video links**, exit 0.
- `account-scan.exe direct https://www.youtube.com/@YouTube/videos unused 30 10`
  returned **10 distinct video URLs**, exit 0.
- An actual download of account video `7699825832717470977` succeeded: MP4,
  **1,858,499 bytes**, in the explicitly scoped QA output
  `.runtime/account-download-verification/download-3bba57a669c63b6b392939a8ce8117fa/`.
  Metadata probing returned one video and ten formats. FFprobe confirmed HEVC video
  at 1080×1440 plus AAC audio, duration 11.910998 seconds. No 100-video media batch was
  downloaded, and no fixture was imported into the user's project/library. The
  diagnostic browser was closed through its own loopback DevTools endpoint after
  checking; Nexora/personal browsers were untouched.

The example is `src/crates/concat-studio/examples/account-scan.rs`. Its optional
timeout and limit arguments make future manual network verification reproducible.
The account-browser fixture is separate from the normal app's
`studio/download-browser` profile.

## Deduplication and compatibility

Video-library entries retain their original path/source and have a backwards-compatible
optional `source_url`. Source keys normalize YouTube watch/short/shorts links and
TikTok numeric video IDs (independent of account renames/tracking parameters).
Only entries whose local file still exists suppress download. Legacy online downloads
with the old `[platform-id]` filename convention are recognized conservatively.
Files downloaded elsewhere, renamed legacy files without provenance, and cross-platform
reuploads cannot be proven duplicates; this is not a universal content-hash detector.

## Tests before native handoff

- `cargo test -p concat-studio --lib -- --test-threads=1`:
  **212 passed, 6 ignored, 0 failed**. Tests cover identity equivalence, file deletion
  allowing re-download, legacy provenance, exact 100-new selection excluding 20 old
  entries, flat metadata item URLs, browser account/URL scoping, cancellation, and
  existing unrelated adapter functionality.
- An earlier unrestricted parallel test run hit a pre-existing socket-fixture timeout
  in `browser::tests::production_starts_stopped_profile_and_reuses_its_created_grok_tab`.
  Serial rerun passed; no unrelated browser source was changed to hide the failure.
- Node account/download/production contract/UI regressions: **16 passed**.
- The real changed Slint dialog compiled in `tools/download-ui-preview`.
  Real pointer click checks passed for selection, busy cancellation, single-video
  cancellation, and error rendering. Preview emits existing embedded-image decode
  warnings, so it is not described as warning-free.
- The new native `CONCAT_DOWNLOAD_SMOKE=1 --smoke` hook exercises the actual bound
  selection callbacks (select 10/100, hidden existing entries, blocked duplicate
  selection, frozen busy state, clearing and URL switching), without network or
  user-project writes. Final executable smoke/build result is recorded below when run.

## Build and final executable

Historical in-progress checkpoint (superseded by the final handoff above): no claim
that the user's EXE was replaced was made before build/smoke finished. One initial
PowerShell redirection attempt converted Cargo's stderr status line into a terminating
`NativeCommandError`; the build was restarted through the existing script without
that redirection. The first compiling attempt then found two `E0599` errors in the
new native smoke assertions (`Model::row_count` needed the `slint::Model` trait import).
The import was fixed. Only this task's already-failing rustc PID 34568, verified against
its parent Cargo PID 32540, creation time and crate command line, was stopped to avoid
waiting on the giant generated UI after known errors. The corrected build was started
serially; no unrelated process was killed.

## Click-path / safety audit

- URL, playlist and limit edits invalidate old metadata/selection; busy operations
  reject edits. Results are generation-checked before publishing.
- Selecting 10/100 uses a tested helper that excludes existing files and never
  selects more than are available. Hidden-row indexes remain original indexes.
- Download snapshots selected individual URLs; never submits the entire profile
  despite the channel checkbox. Sources are checked against persisted history again.
- Cancel affects only the current download tree and stops subsequent queue items;
  completed files were indexed before the next item began.
- Browser collection only follows the explicit account in an app-owned browser
  profile. It does not transfer a browser session to the downloader or bypass private
  video access. A platform may still reject an individual download; that row reports
  the error and remains selected for retry.
- Listing order can include pinned videos; it is not claimed to be chronological.
  A bounded or stalled browser scan is reported as the number read, not as the whole
  account. Only TikTok has the rendered-browser fallback; other platforms use yt-dlp.

## Final self-evaluation

Accuracy 4/5: real 100-link TikTok and 10-link YouTube results and one successful
account-video download; no full 100-media batch is claimed. Completeness 4/5: requested
selection/dedup/cancellation are implemented, but private/gated videos still require
platform permission and cross-platform content duplicates are outside provenance
matching. Clarity 4/5: actions and source counts are explicit; generic TikTok image-alt
titles can be less informative than thumbnails. Actionability 4/5: updated native
EXE and actual bound-callback smoke passed; launch via the existing helper, with
no installer update implied. Conciseness 4/5:
detailed evidence lives here instead of a long final chat message.

Overall **4.0/5**; no critical axis at 2 or below. Future verification improvements,
ranked by impact: an explicitly chosen larger real-media queue run (not downloading
100 user videos just to benchmark), then additional platform-specific account
fixtures. Neither is presented as a completed test. Would the user agree? The
provided account was actually enumerated to 100 unique links, one of its videos was
downloaded, and the updated EXE's real selection callbacks passed; this matches the
delivered scope without claiming every platform/private account works.

Verification-loop checkpoint: isolated Slint build and focused Rust/Node tests pass;
Rust types/lint are covered by compiler and scoped `rustfmt --check` (no full Clippy
or coverage percentage measured). Scoped security grep found no credential hooks,
session-export calls or sandbox-disabling flags in the new browser/controller/index
code. Existing worktree changes are preserved; the changed pre-existing exports and
test expectation were reviewed without resetting, staging, committing or pushing.

No mandatory work remains for this scoped implementation. Full 100-media throughput,
all-social-network coverage, universal content-hash deduplication and private-video
access are not claimed. The implementation reused the current download dialog/theme
and library rather than adding another download screen. Skill-guided click-path
verification checked final states and cancellation; security review kept the rendered
browser isolated and credentials out of export paths.
