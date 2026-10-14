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
