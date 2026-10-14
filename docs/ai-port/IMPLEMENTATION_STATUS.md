# Implementation status — 2026-10-14

## Not complete

The four cloned projects have been inspected and dependency closures mapped.
This is the first vertical slice in progress, not a completed four-repo port.
Native AI Editing is wired and compiles, but GUI acceptance is still pending.
There is no full agent tool/MCP/generation runtime yet.

## Implemented in this working tree

- `concat-project/commands/mod.rs`: native Batch stages both Project and IdMint;
  failed/nested batches no longer consume live IDs.
- `concat-project/editor.rs`: monotonic revision including undo/redo/view edits,
  opaque per-editor authority, exact mint fork, bounded PreparedDraft,
  sequential/native-batch parity check and consuming revision-checked commit.
- Read-only PlanningSnapshot retains live binding while inference/compilation
  can run on a worker. Plain mutable scratch forks have separate identities.
- `concat-host/session.rs`: additive planning/prepare/commit APIs and revision
  on EditorView. Six host Session tests now pass, including disk/native render.
- New `concat-agent`: strict version-1 JSON/seconds schema, 64 KiB response,
  max256 operations, active timeline/revision binding, clip/track checks,
  sequential aliases, native no-op refusal and move-collision refusal.
  Initial operations: split, remove/ripple, move, shortening trim, text title.
- Promax additive authenticated `/api/v1/artcraft/ai/edit-plan`: strict JSON
  output capability gate, exact selected model, plan/refusal alternatives,
  bounded allowlisted context, actual optional usage, typed refusal/truncation,
  catalogue/completion cancellation, no automatic retry. Other chat/test routes
  remain unchanged. Native request has a dedicated 80-second bound.
- Native dockable AI Editing pane/controller: worker inference/compile, actual
  clip/duration diff, isolated composited before/draft images, explicit approve,
  reject/cancel and epoch/revision guards. Session approval goes through native
  Studio bookkeeping. Account changes invalidate old proposals.

## Real verification evidence

Baseline before new draft APIs:

- `scripts/windows-studio.ps1 -CheckOnly`: passed, 3m20s.
- `cargo test -p concat-project --profile quick`: 114 passed.
- `cargo test -p concat-studio --profile quick`: 75 passed, 1 ignored live smoke.

TDD:

- Nested failed-batch test RED: next created ID was c4 instead of control c3.
  After staging mint, GREEN:115 project tests.
- Draft API tests initially RED: prepare_batch/commit_draft missing.
  GREEN:119 project tests; cleanup stale regression brought count to120.
- New agent tests initially RED: plan module absent. GREEN:3 agent tests,
  covering split -> created alias -> remove -> Vietnamese title -> approval ->
  one undo/redo -> document reopen; invalid/stale/oversized/unknown input and
  partial failures leave live state unchanged.
- Latest project suite:121 passed; agent suite:6 passed.
- `scripts/windows-studio.ps1 -TestAI`:121 project +6 agent +6 host passed.
  Host real-fixture test encodes footage, checks title absence/presence at
  0.05/0.3/0.8 seconds, exports actual MP4, saves/reopens, and undo restores
  the original project. Initial cleanup failed on Windows due retained encoder
  and warm decoder handles; explicit fixture resource release fixed it.
- `-TestAdapters`:77 passed,1 ignored live smoke. Structured native receipt
  fixtures included. Backend worker reports11 new mock tests plus12 existing
  AI tests and build/test compilation passed.
- Native CheckOnly after Session changes passed (5m14s). First pane check
  found missing explicit Slint global export (E0432); export fixed and latest
  CheckOnly passed in4m56s. Do not claim GUI E2E or provider readiness from these tests.

All commands ran with target outputs on D. Build/test TEMP/TMP now also point
to `.runtime/native-temp` on D. Source worktrees were rechecked clean; no cache
deleted, paid generation/model download/login/publishing performed.

User explicitly authorized one short real AI request and starting local Promax.
Local Promax was started (PID8892, loopback43100); health200 verified, then only
that verified owned process was reloaded to current backend code (PID29468). There is
no native DPAPI session in APPDATA or debug portable and no explicit pilot token.
Thus no real inference request has been sent; user login is required. No cookie
extraction, invented credential or implicit identity bootstrap is permitted.

## Next required work (same first slice)

1. Execute GUI helper/acceptance tests (pane compilation now passes).
2. After user native login, use exactly one authorized short structured request
   with the configured model; do not retry an unknown outcome.
3. Native GUI draft/preview/approval/undo/save/export acceptance remains required.
5. Only after that first slice, proceed with safe checkpoints, full skills,
   intelligence, authenticated live-session MCP and generation workflows.

Review identified and fixed a cleanup revision bypass for false/null retired
fields. Higher layers still must enforce human permission, durable idempotency,
auth/path scope and cost budget; an opaque draft alone is not authorization.
