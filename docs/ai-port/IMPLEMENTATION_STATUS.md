# Implementation status — 2026-10-15

## Follow-up session: current state (supersedes first-slice sequencing below)

### Disk cleanup follow-up (user authorized, 2026-10-15)

Disk-full blocker has been relieved: D now has ~35.4 GiB free. Removed redundant
Concat incremental configurations/session snapshots, expired own-library builds,
abandoned compiler objects, test EXE/PDB outputs and failed render fixtures.
Source, .deps, models, third-party Cargo libraries and current dev executable/
native library were preserved. Removed artifacts are regenerable, not trashed.
`scripts/cleanup-build.ps1` defaults to dry-run; `-Apply` prunes only validated
build/temp paths, rejects reparse points and respects active processes/Cargo
locks. `windows-studio.ps1` calls cleanup before and after builds/tests, including
failure paths. Guard checks passed for source exclusion, occupied Cargo lock,
repeat dry-run, and retained current executable/library. Full GUI/render checks
were not rerun during this cleanup request; their earlier blocked status below
is historical and the exact continuation still applies.

Actual target HEAD at start was the requested baseline
`067f1e268e59c34e507d808e40349ccc3cb76b81`, with a clean worktree. Source clones
remain read-only. An unrelated `concat-studio/src/products.rs` change appeared
during this session; it was preserved, not treated as our port or reverted.

Implemented expanded native operations and bounded multi-turn agent runtime;
details/test names/gaps are in the follow-up table in FEATURE_PARITY_MATRIX.
The runtime uses the existing authenticated Promax provider service and creates
only a revision/owner-bound proposal. It cannot approve or mutate the live GUI
Session. Native UI adds an opt-in agent mode (default off), max-turn control and
contact-sheet inspection, not a replacement editor/theme. Each provider turn
may consume quota; this turn performed zero real provider invocations.

The contact sheet uses Concat Monitor/export compositor, full project geometry
before downsampling, isolated ReaderPool, max16 times, per-frame/title memory
bounds and local cancellation. Cut/end/changed-span samples are explicit;
excess sample coverage is refused, not silently truncated. Strict inspection
refuses partially missing source layers while normal playback's existing
graceful fallback stays unchanged. No upstream renderer or history was copied.

### Commands actually run

- Baseline `-TestAI`: 6 agent +121 project +6 host Session tests passed.
- Baseline `-TestAdapters`: 77 passed,1 ignored live-service smoke.
- Baseline `-CheckOnly`: passed before this session's latest UI/contact changes.
- Latest `cargo test -p concat-agent --profile quick`: **14 passed**, including
  mixed native ops, two-way trim bounds, range/source mapping, 50-operation
  one-undo, 30th failure preserving project/revision/next ID, multi-turn tools,
  cancel/unknown outcome/missing usage refusal. TEMP/TMP and target on D.
- Latest `-TestAdapters`: **78 passed,1 ignored**, including actual HTTP fixture
  for authenticated tool-call transport/capability metadata.
- Promax `pnpm exec tsc -p tsconfig.build.json`,
  `pnpm exec tsc -p tests/tsconfig.json`,
  `node --test .test-build/unit/edit-plan.test.js`: **13 passed**, including
  extended schema and normal request close vs response disconnect/cleanup.
- Latest `-TestAI`: agent13/project121 passed at that point, **host link failed**
  LNK1318 Unexpected PDB error LIMIT(12), during disk exhaustion. Host contact
  and modified render tests are written, not represented as executed.
- Latest `-CheckOnly`: **failed writing full.rmeta, os error112 (disk full)**.
- Additional `cargo check -p concat-host --tests`: also failed os error112 in
  wgpu-types. Latest app/contact/source-completeness changes remain unverified.

Disk D had only 2,338,816 free bytes at the final diagnostic. No move to C, cache
deletion, source reset, release build or unapproved model/job request was used
to bypass this. Need several GB free on D before compiling/linking the GUI and
host tests. No running backend was silently reloaded by this session.

### Exact continuation

1. After freeing D space, run `-CheckOnly`, `-TestAI`, `-TestAdapters` again;
   TestAI now runs both host `session::tests` and `contact_sheet::tests` and stops
   if either fails. Fix any failures, then test native GUI inspection
   and approve/undo/save/export. Check missing-layer refusal and cancellation.
2. Complete OpenChatCut dependency reading/tool schemas/skills and durable run
   ledger/terminal recovery; current runtime is a bounded subset, not full port.
3. WeftCut named checkpoints, owner/write lease, actor activity/history lock,
   duplicate mutation and crash recovery on the existing Session/undo boundary.
4. Cartcut remaining import/style/motion/effects/transitions/text metrics and
   contact-sheet paging/overlays; current preview refuses >16 samples.
5. Word alignment/transcript-to-native edits, VAD/beats/scenes/reframe/search
   versioned cache using existing speech/media/vision/text crates.
6. Authenticated MCP bound to the actual GUI-owned Session, not headless copy.
7. Velorn graph/dependency/Comfy jobs, verified provenance/import/native placement
   with fixture integration tests; real services/weights are separate E2E gates.
8. Once native user login exists, perform at most the one already-authorized
   short real structured request, without uncertain retries. Missing login
   must NOT block independent implementation/testing above.

Full requested port is **not complete**. No real-model, desktop GUI, live MCP,
intelligence or generation E2E is claimed. Earlier text below is historical.

### Handoff self-check

Current partial delivery: 3.2/5, not acceptance of the requested full port.

| Axis | Score | Evidence / improvement |
|---|---|---|
| Accuracy | 4 | 14 agent,78 adapter,13 BE tests passed; newest contact/GUI changes need compile/runtime verification after disk space is available. |
| Completeness | 1 | Full lease/checkpoint/durable recovery, intelligence, live MCP, generation and source skills are not implemented; continue the exact sequence above, not mark parity complete. |
| Clarity | 4 | Current evidence supersedes preserved historical notes; remove historical ambiguity only once later acceptance replaces it. |
| Actionability | 3 | Source/tests and continuation locations are present, but the app cannot be freshly built with only ~2 MB free on D. Free several GB, then rerun all prescribed gates. |
| Conciseness | 4 | Final handoff is short; historical evidence retained here is longer for auditability. |

Highest-impact improvements: complete missing source groups, verify native
contact/GUI render once D has space, then run authenticated E2E with explicit
quota permission. Self-check: user would agree this is progress, not the full
functional port requested. Preserve this work; do not rewrite/reset it.

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
5. Continue independent safe checkpoints, full skills, intelligence, live-session
   MCP and generation work; real-provider login is not a prerequisite for coding.

Review identified and fixed a cleanup revision bypass for false/null retired
fields. Higher layers still must enforce human permission, durable idempotency,
auth/path scope and cost budget; an opaque draft alone is not authorization.
