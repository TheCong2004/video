# Studio backend migration into Concat

2026-10-13. Status: implementation in progress, not a complete ArtCraft replacement.

## User-approved direction

Concat is the main desktop application. Move useful Studio UI **and application
services**, including OpenTalking/LIVE. Do not copy whole ArtCraft workspaces or
add menu-only substitutes. Remove old implementations only after replacements
are verified and persistent data can be preserved.

Proposed scope, communicated to the user: AI production, workflows, scripts,
batch import, jobs, channels/publication, activity, provider configuration, and
OpenTalking/LIVE. Legacy CapCut-specific draft output and editor functions already
provided by Concat are excluded from the new implementation. No old code or user
assets have been deleted as part of this slice.

## Ownership boundaries

- `concat-studio`: Rust application services, currently local prompt storage,
  bounded health checks and authenticated read-only workflow definitions.
  No Slint/Tauri/FFmpeg/Python/model dependency; tests run independently.
- `concat-host`: existing editor integration, compatibility exports of Studio
  services. Do not import the ArtCraft Tauri app or its workspace-hack crate.
- Native window: Slint views and asynchronous Rust bindings. No file access on
  the UI thread; a closed window discards worker responses.
- Promax: shared authenticated server APIs. Moving desktop ownership does not
  mean copying cloud identity, database credentials or provider keys into projects.
- Browser workers and model engines: optional capabilities, not a requirement
  for opening Concat or editing a script. Runtime packaging is still pending.

## Concrete source findings

ArtCraft's `floword_commands.rs` exposes Tauri operations using TaskDatabase,
AppHandle, cookies, pipeline state, browser workers and SQLite queries. It is not
a standalone HTTP backend that can be copied into Concat unchanged.

Prompt source: `artcraft/crates/schema/database/sqlite_tasks/src/queries/prompt_templates/`
and list/upsert/delete handlers in `core/commands/pipeline/floword_commands.rs`.
Content fields retained: name, image_prompt, expand_prompt, video_prompt;
timestamps remain Unix seconds. The new store adds optimistic revisions.

## Implemented in this slice

- Independent `concat-studio` crate and independent test command.
- Existing health/workflow adapter code moved into that crate, with compatibility
  exports keeping the native window API stable.
- Real local prompt CRUD, persisted at `<Concat config>/studio/scripts.json`.
  It works without ArtCraft/Promax/Node/Python running.
- OS file lock across read-modify-replace, bounded JSON, synced temporary write,
  atomic replace, input validation and stale-save/delete rejection.
- Native script route with editor, load/new/save/delete actions, explicit delete
  confirmation, loading/error feedback and unsaved-draft protection.
- Native UI strings have Vietnamese/English translations; other locales use
  explicit English fallback. Store validation errors are still English.
- Scroll content constrained to available viewport width instead of long text
  dictating the layout width. Visual verification remains pending.

This is a local prompt library, **not** an online marketplace, AI generation,
or migration of the existing ArtCraft SQLite rows.

## Required acceptance criteria

1. Local script CRUD: starting with a fresh isolated config directory, create,
   reopen, edit and delete a prompt; records persist, stale revisions and corrupt
   files cannot be overwritten. Verify unit fixtures and manual native actions.
2. Engine independence: Studio tests and script CRUD run without Tauri, FFmpeg,
   Python, browser workers or model downloads. Verify crate dependencies and
   `cargo test -p concat-studio --profile quick`.
3. Native UI: script-route callbacks invoke the real store off-thread; deleting
   requires confirmation; no other video/project/ArtCraft file is deleted.
   Verify structural tests, native check and interactive smoke (pending).
4. Data migration: importing existing ArtCraft data must be an explicit action,
   with read-only source access, preflight counts, backup/rollback, duplicate-ID
   handling and schema/version checks. **Not implemented; do not read a guessed
   live database or silently show old data as migrated.**
5. Production/jobs: submit one explicit job, correlate job/project, expose real
   stage events/cancel/retry and import only a verified artifact into Concat.
   No fake progress, duplicate resubmission or paid provider calls in tests.
   **Pending.**
6. Auth/providers: native sign-in/refresh and OS-backed secret persistence must
   precede tenant-mutating cloud actions. Never copy browser cookie databases or
   store provider secrets in a timeline document. **Pending.**
7. OpenTalking/LIVE: preserve identity and motion from the selected source video;
   keep idle playback independent of reply synthesis; mute source audio; report
   actual engine/model readiness and CPU delays. No promise of CPU real-time
   MuseTalk or AI-generated full-body gestures. **Pending.**
8. Publishing: preserve page/profile ownership, explicit approval and real
   server states. Verify against fixtures before user-authorized account tests.
   **Pending.**
9. Removal gate: delete redundant ArtCraft modules only after replacement tests,
   user data backup/migration and runnable native validation. Never remove models,
   media, credentials, history or browser profiles as generic cleanup. **Pending.**

## Next implementation sequence

1. Native authentication/session ownership and provider capability configuration.
2. Existing script import with source schema preflight (no destructive migration).
3. Production job model, queue and actual provider/worker adapters; native forms,
   real events, retry/cancel and output-to-Media integration.
4. Batch validation/commit, workflow editor/run controls, job centre/history.
5. Channel/profile configuration and approval-based publication.
6. OpenTalking character/voice/video history and lightweight continuous LIVE
   playback plus comment/reply queue; integrate only installed usable engines.
7. Compare old/new flows, package only required runtime dependencies, remove
   verified duplicate implementations with a rollback path.

## Verification commands

```powershell
cd D:\capcutpolot\concat\src
cargo test -p concat-studio --profile quick

cd ..
node scripts/studio-launcher-layout.test.mjs
.\scripts\windows-studio-launcher.tests.ps1
.\scripts\windows-studio.ps1 -CheckOnly
# Rebuild and run the current native UI (not the old -RunBuilt executable):
.\scripts\windows-studio.ps1
```

No production migration, external generation, account publication or existing
ArtCraft data deletion is part of these tests.

## Evidence for this implementation slice

- Independent `cargo test -p concat-studio --profile quick`: **18 passed**, no
  filtered tests; incremental test rebuild 4.04s on this machine (not a desktop
  rebuild benchmark). Fixtures use temporary local folders and loopback servers.
- `cargo tree -p concat-studio --depth 1`: serde, serde_json, ureq, url only.
- Native `windows-studio.ps1 -CheckOnly`: **passed** after crate extraction and
  localized UI bindings, 3m04s. This checks the generated Slint/Rust tree; it does
  not relink the executable or prove interactive UX.
- Structural layout/callback tests: **6 passed**. Launcher environment test:
  **passed**. Locale inventory: 25 locales, zero missing/stale keys; existing
  English fallback warnings remain. Diff whitespace check passed.
- Interactive create/edit/delete, viewport widths and multiline editing remain
  unverified. No current native executable rebuild/runtime smoke is claimed.

### Self-review of progress against the full request

Summary: 3.2/5; useful tested first backend slice, not the requested full migration.

| Axis | Score | Evidence / improvement |
|---|---|---|
| Accuracy | 4 | Native check and 18 fixtures pass; add interactive persistence and layout smoke |
| Completeness | 1 | Production/jobs/auth/publication/OpenTalking and legacy data import remain unported; implement and verify these before calling migration complete |
| Clarity | 4 | Source/ownership and pending criteria are explicit; improve remaining English store errors |
| Actionability | 3 | Local script source and repeatable checks exist, but user still needs a dev rebuild and the original ArtCraft app for other functions |
| Conciseness | 4 | One independent crate, no new direct third-party dependency; migration documentation is necessarily larger than this code slice |

Highest-impact next work: native auth/provider ownership; production/job/verified
artifact pipeline; OpenTalking/LIVE motion/reply integration. Do not remove old
modules before those replacements and data migration pass their gates.

Self-check: the user would not consider this the full requested replacement.
Verdict: report partial progress only; do not describe the application as migrated.
