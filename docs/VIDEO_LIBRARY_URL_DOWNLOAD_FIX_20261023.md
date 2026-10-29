# Video inventory and online download repair — 23 October 2026

## Final handoff — 24 October 2026, Asia/Bangkok

The new development executable is built and its native startup smoke passed.

- File: `src/target/debug/concat.exe`, 269,584,384 bytes.
- Modified: **24 October 2026, 00:57:25** local time.
- SHA-256: `668d5d552aa7d3bc868496de1b5d2fe77f30b21e18018df1e424fcd992928152`.
- Serial build: `scripts/windows-studio.ps1 -Build`; Cargo finished in 18m12s.
  Durable logs: `.runtime/concat-build-20261024-003921.{stdout,stderr}.log`.
  The detached wrapper did not expose its process exit code, so the actual repo
  build command was rerun directly: cache-fresh verification finished in 1.28s,
  tool exit 0 and explicit `VERIFIED_BUILD_EXIT=0`. It did not recompile the app.
- New EXE `--smoke` with `CONCAT_STORYBOARD_SMOKE=1`: **exit 0**.
  Log: `C:/Users/thecong/AppData/Local/app.concat.editor/logs/concat-20261023-175842.log`
  (filename uses UTC). It contains storyboard PASS, native Create validation PASS
  and `smoke: ok`; no provider/browser generation was submitted.
- Fresh verification: **7 Lux tests + 2 library tests passed**; **27 Node regression
  tests passed**. The three focused native UI modes were rerun successfully.
- Rust and Slint types passed in the full build. Scoped `rustfmt --check` passed
  for the four new/rewritten Rust download/library modules. Full Clippy and
  repository-wide test coverage were not run; coverage is not claimed.
- Scoped auth/cookie-hook scan found no browser-cookie extraction, Authorization,
  API-key or access-token hooks in those four modules. This is not a full security
  audit of the existing dirty workspace.
- Existing missing-documentation warnings and the unused `account_changed`
  warning remain; the build is not represented as warning-free.
- The repo build helper removed about **0.26 GB** of regenerable artifacts before
  this build; it did not remove videos or projects. No second app installer or
  duplicate full build directory was made. No unrelated work was reset/staged/pushed.

Launch the verified build from any working directory:

```powershell
& 'D:\capcutpolot\concat\scripts\windows-studio.ps1' -RunBuilt
```

The completed changes here are the local video inventory and URL importer. They
do not resolve or claim end-to-end Grok provider/Promax transport readiness.

## Requested behavior

The existing sidebar **Danh sách video** is the inventory of completed generated
and downloaded local videos, not a CSV import screen. It keeps references to the
original files rather than making duplicate copies. The old CSV validator remains
an internal component but is not the video-library route.

## Implementation

- Completed native production, final storyboard renders and validated URL
  downloads register in `data/studio/video-library/*.json`.
- Registration uses a canonical-path key and independently published records;
  simultaneous completions cannot overwrite an entire shared JSON list.
- Known production roots (including Page output folders) and the active project's
  storyboard output can be rediscovered with bounded traversal. No drive-wide scan.
- The UI offers play, reveal and import into an open project, newest first;
  rendering is limited to 30 rows per page. Missing files are not shown.
- Online downloads use a unique owned directory per attempt under
  `data/videos/downloads`. Repeat downloads no longer depend on detecting a new
  filename in a shared temporary folder.
- Failed/cancelled attempts roll back only their freshly-created directory, after
  stopping the downloader process tree. Complete successful outputs are retained.
- Cancel works during metadata extraction and downloading/merging. Windows jobs
  own only the corresponding child tree. Metadata queries have a total time limit.
- URL/playlist changes invalidate the old stream selection. Busy operations cannot
  be launched twice. A finished download is not imported into a different project
  if the user changed or closed projects in the meantime.
- Files are probed for a video stream before cataloging/import. The import still
  uses Concat's existing media importer.
- YouTube/TikTok use the bundled yt-dlp helper when available; other existing
  platforms retain Lux. User configuration files and browser cookies are not read.
- TikTok profile URLs require the explicitly enabled playlist/channel option;
  otherwise the dialog explains how to use a specific video link. A feed cannot
  be represented as one video's stream selection.
- Short actionable errors replace raw Go stacks. HTTP 403 is not described as
  proof of a copyright restriction.
- Windows packaging stages both engines and the helper's license notices.

## Evidence collected

- 7 Lux adapter unit tests passed, including cancellation, validation, metadata
  parsing and bounded error presentation.
- 2 persistent library unit tests passed: registration survives reread, deduplicates,
  preserves the original, rejects missing files and retains concurrent records.
- 27 focused Node regression tests passed. Two old assertions were updated to
  reflect the previously requested single Page/workflow-refresh row rather than
  resurrecting its removed duplicate control.
- The actual Rust download adapter downloaded the public YouTube sample
  `jNQXAC9IVRw` twice into separate directories. FFprobe reported video and audio,
  19.021 seconds. This exercises actual file discovery, not simulated completion.
- The same adapter downloaded TikTok video `7047596209028074758`; FFprobe reported
  video 576×1024 plus audio, 21.268 seconds, 1,790,611 bytes.
- Real adapter cancellation returned `DOWNLOAD_CANCELLED` in approximately
  1.56 seconds; its newly-created attempt directory was removed.
- Both downloaded samples passed first-frame decode with the staged FFmpeg runtime.
- The focused native headless tool compiled in 1m34s and passed the library import
  button and download cancellation button checks through real pointer events;
  the concise error dialog was rendered and visually inspected. Screenshots:
  `.runtime/download-ui-check/{library,downloading,error}.png`. The library fixture
  emitted four existing embedded-image decode warnings; text layout and actions
  still passed. This is not a claim of warning-free full-app rendering.
- The supplied TikTok profile `@oclon2026` did not expose an extractable public
  list in the direct feed check. The extractor reported it could not obtain a
  secondary user ID and warned about privacy/embedding access. This does **not**
  prove that the account is private. Individual-video success is not a claim that
  this account's entire feed is downloadable.
- No provider generation job was created, no browser cookies were accessed, and
  no platform account/login requirement was added to Concat.

## Historical failed checkpoint — superseded by final handoff above

The full Concat executable has NOT been updated. Windows Application event 1000
at 23:43:38 reports rustc 1.93.1 PID 22420 crashed with exception `0xc0000409`.
System event 2004 at 23:43:35 reports low virtual memory: the Concat compiler used
20,041,363,456 bytes, the concurrent Nexora compiler used 4,400,975,872 bytes, and
an IDE used 1,908,592,640 bytes. The existing EXE still has timestamp 21:46:56.
Do not treat this as a successful binary handoff or launch the old EXE as proof
of these changes. Coordinate with the other Nexora build and free memory before
retrying the full build and startup check. No other agent's compiler was stopped.
Broad all-route UI compilation was stopped to reduce memory pressure; the smaller
focused native UI tool above replaced it and passed.

This paragraph describes the failed **23 October** attempt only; the successful
24 October build at the top supersedes its pending-binary status.

## Helper provenance

yt-dlp official release `2026.10.21`, Windows x64 executable, verified SHA-256:
`33121d192d706e62f684a06f80918e3f3a8ba52ac3a65213b6951c39c7d9f555`.
Sources: <https://github.com/yt-dlp/yt-dlp/releases/tag/2026.10.21> and
<https://github.com/yt-dlp/yt-dlp#readme>.

This verification does not claim every supported social network has been tested,
nor that remote platform access limits have been bypassed.

The three downloaded QA samples total about 2.74 MB under `.runtime`. An attempt
to remove only their explicit test directories was blocked by the host's command
policy; it was not retried through another mechanism. These are not user videos
or duplicate application builds. They remain available for reproducing the checks.

## Completion self-evaluation

Overall: **4.0/5**. Deliver the verified local build with the explicit platform limit.

| Axis | Score | Evidence / remaining improvement |
|---|---:|---|
| Accuracy | 4 | Real downloaded files, first-frame decode, fresh EXE/hash and exit-0 startup are recorded; the supplied TikTok account feed is still unverified. Obtain a specific accessible account/video before claiming that feed works. |
| Completeness | 4 | Library registration, repeat downloads, cancellation, safe import and UI checks passed; only the two requested platforms were exercised live, not every social site. Extend live cases when concrete failing links are supplied. |
| Clarity | 4 | The final handoff separates successful artifact checks from historical failure; the full audit remains longer than the user-facing summary. Keep that summary outcome-first. |
| Actionability | 4 | The new EXE is present and the exact launch command is supplied; OS video-player association and remote feed access remain environmental. Surface their errors rather than claiming success. |
| Conciseness | 4 | Final user reply can be short; detailed reproducibility evidence stays in this document rather than the chat handoff. |

No axis is 2 or below. Highest-value follow-up is verification of the user's
specific TikTok feed when it exposes accessible video links; it is not a reason
to conceal the successful individual-video checks. Self-check: the user can
reasonably agree that this local build is now delivered, but should not be asked
to accept that their entire TikTok account feed has been proven downloadable.
