# Native Studio migration progress

This document tracks the migration of useful ArtCraft Studio capabilities into
Concat's native Rust/Slint application. The goal is to replace the old UI and
its required services with working native equivalents, not to copy menu labels
or show mock screens. ArtCraft source code, local databases, models, generated
media and user data must remain intact until each replacement is verified.
Build artifacts stay on D:; never build this project on C:.

## Current status

Migration is incomplete. The Concat workspace has native routes and service
adapters for several Studio areas, but they have not all been compiled,
integrated against the running backends, or exercised end-to-end. Do not remove
the old ArtCraft implementation or call the migration complete.

## Native work present in the current source tree

- Native Studio shell, navigation, theme and localization, with Promax and
  ArtCraft connection settings kept explicit and services not started merely
  by opening the app.
- Promax-backed AI keys/models and text-generation controls. The native LIVE
  reply path uses the selected model, turns off by default, deduplicates chat
  event IDs, permits one request at a time, applies a cooldown and bounds its
  output. The LIVE page exposes model and OpenTalking voice selectors and lets
  users refresh both catalogues in place. This path still needs authenticated
  service verification; speech is audio-only (no lip-sync).
  The LIVE comment feed now keeps the latest bounded comments and has a
  per-comment Ask AI action that generates, sends and speaks a reply.
- Promax workflow catalogue and run controls, including explicit confirmation
  for mutations and response/receipt validation.
- Script-market and production routes with native controls and typed backend
  calls; publication side effects must still be verified against the real
  service before claiming parity.
- The native Channels route now has a real local page list/editor with
  create/update/archive/restore, preserving ArtCraft's page configuration
  fields. Storage uses atomic writes, an inter-process lock and revision checks.
  The ArtCraft SQLite catalog is untouched; import, CSV validation wiring,
  account linking and publishing are still pending.
- Bulk-import CSV parsing and row-level validation. The native content-page
  store is not yet connected to it; batch enqueue API and worker remain unmigrated.
- Native OpenTalking catalogue/job controls for configured avatars, voices,
  backgrounds and runtime-reported video drivers; create, stop, delete, refresh,
  download and import actions use bounded local HTTP and validate receipts.
  A still image is not represented as a source of full-body motion.
- Native TikTok LIVE controls and a supervised connector protocol/runtime.
  Debug builds now discover the sibling ArtCraft connector only when its built
  entrypoint and dependencies exist; packaged builds still require packaged
  resources or explicit configuration. Real TikTok connection, chat ingestion
  and message delivery remain unverified.

## Remaining work and limitations

- Stage and package the LIVE connector and its runtime resources in Concat;
  verify explicit shutdown, reconnect and process ownership.
- Complete LIVE reply orchestration. AI text replies and the OpenTalking TTS
  transport compile; the reply voice is selected from the runtime catalogue.
  Audio responses are MIME/size bounded, decoded to capped 48 kHz stereo and
  mixed as one-shots through Concat's supervised output without moving the
  timeline. A missing output device is reported in LIVE status. Actual TTS,
  TikTok/OBS capture and live delivery remain unverified, and lip sync,
  moderation, product scripting and OBS/TikTok control are not migrated.
- The source LIVE surface also includes product/playlist management, a motion
  clip picker, preview/output window, event rules and OBS settings. These do
  not yet have native equivalents, so the native LIVE page remains visibly
  smaller than ArtCraft. Manual per-comment AI reply is now present, but its
  authenticated LIVE/TTS behavior still needs a real session test.
- Wire the new page catalog into CSV validation; implement legacy SQLite import
  as an explicit preview/backup operation, and only then connect batch queueing
  to a verified job API and worker contract.
- Audit every source Studio feature against its backend contract, migrate the
  still-missing useful operations, then test native UI behavior and real
  backend flows before retiring any old implementation.

## Verification history and current gate

In this continuation, the full `concat-studio` fixture suite passed **66/66**,
including create/reopen/update/archive, stale-write rejection and corrupt-store
preservation for the new page store. All **14** native launcher structural
checks pass, including the Channels route binding and LIVE selector/reply controls.
The full `cargo check -p concat` passed on D: after fixing a moved cache handle
in the Channels binding and Arc ownership in manual LIVE reply. The ArtCraft connector's local runtime process
smoke passed malformed/oversized input and stdin-close handling without opening
a TikTok session. Rust formatting and locale parsing pass. All Cargo artifacts
stay on D:. Legacy SQLite import,
actual TikTok/OBS capture, TTS service request, manual LIVE run, full app launch,
connector distribution packaging and lip-sync remain unverified.

The observed free space on D: increased to about 26 GB during this turn, and
the existing `target/quick` cache remained in place. No target/cache files were
deleted. No C: build was run. ArtCraft code and user media were not removed.

## 2026-10-14: connect the user's updated OpenTalking UI

The running local backend on port 30000 returned valid `{ok:true,data:...}`
catalogues. The native list adapter mistakenly tried to unwrap `data` a second
time after its transport had already unwrapped it. Corrected that boundary and
verified the native Rust adapter against the real service: 20 avatars, 1 voice,
1 background, 2 video drivers and 0 jobs, including the selected avatar's image.
Four OpenTalking tests passed, covering catalogue envelopes, the create-job
request/receipt contract, local validation and the live read-only smoke.

The current UI now loads catalogues when opened, displays engine state using
backend readiness, loads the selected character's actual preview, preserves
avatar/voice/driver selections during refresh and polls active jobs every three
seconds. Voice preview now hands returned audio to Concat playback instead of
discarding it while claiming it played. Requests use the speech/job backend's
timeouts and distinguish speech-provider failure from connection failure.

The local Edge provider remains unable to synthesize: the underlying Microsoft
WebSocket returns HTTP 403 (verified separately from the native client). The
runtime masks this as an FFmpeg stream decode failure and returns HTTP 502.
Catalogue connectivity is verified; successful voice generation and a real
video render are not yet verified. The final `cargo check -p concat` passed
with preview loading, job polling and speech-error handling. The complete
Studio service suite passed 73 tests (one live smoke intentionally ignored in
that suite); the explicitly enabled real-backend smoke also passed. User UI styling and data were preserved;
all build output stays under D:\\capcutpolot\\concat\\src\\target.

## 2026-10-14: responsive OpenTalking create workspace

Preserved the user's native visual design. The creation workspace uses a
width-aware grid: two balanced columns at content widths >= 840 logical pixels,
one column below that. The five navigation tabs use five, three or two columns
according to available width. The preview height is bounded to 240–480 logical
pixels and scales with both column width and actual launcher viewport height.
The image uses `contain`, and the form card is capped at its content height so
the source image cannot stretch the inputs apart. The launcher passes viewport
height through StudioPage; no backend changes or restart are required.

Verification: native `windows-studio.ps1 -CheckOnly` passed (16m43s), the new
focused structural regression passed, and `git diff --check` passed. The full
existing structural suite reports 11 passes and four stale assertions against
the user's previously changed UI; it is not claimed as green. A separate
offscreen preview build was stopped because it rebuilt another dependency set;
no new render images were produced. Its temporary source edits were reverted.
The running user app was not stopped. All build artifacts remain on D:.

Self-evaluation: accuracy 4/5 (compiler/test evidence, no runtime resize capture),
completeness 4/5 (create-workspace fix; other pages not visually audited), clarity
4/5 (explicit breakpoints; long compile wait), actionability 4/5 (dev restart
command available; current executable is unchanged), conciseness 4/5 (small
layout-only changes, but verification took longer than intended). Overall 4.0/5.
Highest-value follow-up is checking the rebuilt app at different window sizes
and Windows display scales. The user can verify the visible result after their
next dev launch; this is not a claim that the full migration is complete.

## 2026-10-14: Promax-owned Youwee metadata boundary

User chose Concat media processing/rendering only, excluding CapCut project
export and legacy automation. Promax now owns authenticated Youwee search and
source-resolution routes and a checksum-verified yt-dlp runtime on D:. The native
production form has an explicit source URL check using that API off-thread;
its result never claims a download or render. Two native adapter fixtures cover
the authenticated route/response and unsafe URL rejection. Studio tests: 75 pass,
one ignored live smoke. Native check: passed in 9m50s. Real backend adapter search
and metadata resolution were verified separately from HTTP auth/envelope tests.

Bulk commit previously fabricated a successful queue submission; it now clearly
reports that no jobs were created and points to single-job production. Downloads,
subtitles and the Concat render worker remain pending. Old ArtCraft source/data
and running user processes were preserved. See the exact scope and evaluation
in `D:/capcutpolot/promax-backend/docs/CONCAT_MEDIA_MIGRATION.md`.
