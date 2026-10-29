# Production UX repair — 23 October 2026

## Scope and causes

- Native Create previously only revealed a separate confirmation below the
  production page. A single native click now invokes the existing start handler;
  validation, profile affinity, preflight and duplicate/unknown-outcome guards
  remain in place. Generic workflows retain confirmation beside the action.
- Status is in the same step-7 action card. It no longer requires finding a
  message at the bottom of a long scene page.
- The explicit native graph is Grok/9:16. Old generic provider/aspect values no
  longer silently prevent native preparation. These values are explicit in the
  native UI and effective plan; other graphs retain their editable controls.
- Scene panels no longer impose independent 640px scrollports. One page owns
  scrolling, and panels use intrinsic heights with compact spacing.
- Scene selection previously decoded every visible thumbnail again. A bounded
  UI-thread cache reuses Slint images, invalidated by file modification/size;
  at most 64 entries are retained. Missing files do not return stale images.
- Production background drift is disabled. Shared shell theme/settings controls
  no longer duplicate the Studio header/native title bar.
- Script templates, workflow catalog and tasks are always-visible header actions.

## Verification commands

```powershell
node --test scripts/native-production-contract.test.mjs scripts/production-steps-regression.test.mjs scripts/production-session-regression.test.mjs scripts/studio-launcher-layout.test.mjs scripts/shorts-ux-regression.test.mjs scripts/production-practical-ui.test.mjs
$env:CARGO_BUILD_JOBS='1'
$env:CARGO_INCREMENTAL='0'
cargo build --manifest-path tools/studio-preview/Cargo.toml --target-dir .runtime/nexpulse-build
.runtime/nexpulse-build/debug/concat-studio-preview.exe .runtime/ui-practical-20261023/create.png production-create 1440 1000
.runtime/nexpulse-build/debug/concat-studio-preview.exe .runtime/ui-practical-20261023/scenes.png production-scenes 1440 1800
.runtime/nexpulse-build/debug/concat-studio-preview.exe .runtime/ui-practical-20261023/render.png production-render 1440 1800
.\scripts\windows-studio.ps1 -Build
```

The native pointer test uses an isolated callback fixture, deliberately retains
old generic aspect/provider indices, verifies one-click delivery to the start
handler and rejects a duplicate while busy. It sends no network/provider job.
The preview uses the actual Slint components and software renderer, not HTML.

## Performance measurement contract

Operation: prepare the same 1024x576 fixture thumbnail eight times. Baseline:
clear cache before each preparation. Variant: reuse a warm cache. Correctness:
both produce 320x180 images. Budget: one variant, two rounds, local synthetic
fixture only, zero provider spend. This measures decode/preparation work, not
end-user GPU scroll FPS. Cache removal is the rollback.

The debug `CONCAT_STORYBOARD_SMOKE=1` / `concat.exe --smoke` path runs this
comparison and the existing split/edit/fractional-duration/undo/redo/QA/save
checks in a new temporary project, then removes only that fixture directory.

## Limits

No real Grok generation is submitted by these checks. A selectable Nexora Page
is not proof that the browser worker or Promax transport is ready. The prior
production `TRANSPORT_ORIGIN_NOT_CONFIGURED` blocker is separate from this UI
repair; an inline error must not be described as a completed video.

## Results

- Final main build: `windows-studio.ps1 -Build`, exit 0, 19m45s.
  `src/target/debug/concat.exe`: 271,440,384 bytes, written 20:18:44 local.
  Existing EXE was replaced in place; no extra installer/archive was produced.
- Final Node regression set above plus `native-build-resource-regression`:
  38 tests pass, zero failures.
- Actual new EXE: `CONCAT_STORYBOARD_SMOKE=1`, `--smoke`, exit 0.
  Log `concat-20261023-131917.log`: native split/edit/fractional-duration/
  duplicate/reorder/delete/undo/redo/QA/persistence pass; `smoke: ok`.
- Thumbnail measurements, eight preparations per group:
  round 0: uncached 42.3339 ms, warm cache 0.3457 ms;
  round 1: uncached 34.8818 ms, warm cache 0.3645 ms.
  Same synthetic image and 320x180 dimension gate in both paths. These are
  preparation microbenchmarks, not claims about real-world scroll FPS.
- Real Slint render inspected at 1440px and 960px widths, light and dark.
  Pointer fixture reaches native Create once and blocks a second busy click.
  There are no nested scene scrollports or gaps separating the selected editor
  from its scene cards. One white scene card now owns the grouped controls.
- Read-only production WebSocket upgrade repeated during this repair: HTTP 503,
  `TRANSPORT_ORIGIN_NOT_CONFIGURED`, requiring a Node/Nest `PROMAX_WS_ORIGIN`.
  No real provider-to-video-to-import E2E pass is claimed.
- Main build cleanup reported 0.00 GB removed: there were no additional eligible
  old artifacts in this pass. Projects, models and dependency libraries retained.
- Final preview rebuild: exit 0. `launcher-production 1080 1000` passes real
  sidebar click -> step-2 click -> all seven native layouts. `production-create
  1440 1000` passes again. The launcher fixture emitted one embedded-image format
  warning while cycling layouts; it did not fail the control assertions. This
  is not a claim of warning-free rendering for every possible media input.
- Explicit final `cleanup-build.ps1 -PruneOwnCache -KeepMainConfigurations 1
  -Apply` also found 0.00 GB eligible; no active compiler was running.
