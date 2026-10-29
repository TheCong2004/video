# AI Control progress

## Nexora / Grok batch adapters (2026-10-29)

Added real catalogue discovery to planning, the existing Nexora/Grok runner as a
typed executor, and 1–100-job expansion into the same durable workflow store.
The secondary window reuses existing Nexora startup/catalogue callbacks. AI
script generation, Factory speech/subtitles/editing and artifact validation are
scheduled per video. See `AI_NEXORA_BATCH.md`: publishing, text-only Grok image
generation, multi-shot film assembly and all-app UI coverage remain incomplete.

## Product-script response correction (2026-10-29)

Confirmed that the latest Gemini script request arrived but returned a malformed
unsupported object. Preserve strict validation, explain the schema error in
Vietnamese, prefer Gemini's final answer container over thoughts markdown, and
tell the planner to ask for absent product facts without demanding media assets
for text-only requests. Existing failed workflows are preserved, with no automatic
model resend or fabricated plan. See the corresponding test-results entry.

## Blocked planner Resume correction (2026-10-29)

Resume now depends on executable work or an eligible safe retry, instead of
merely an unfinished workflow. Failed non-replayable planning requests display
a Vietnamese instruction to return to the request. The callback independently
refuses an ineffective Resume, including on legacy stored failed planners.
This fixes the misleading action; creating animated character footage still
requires an actual generation executor and is not implemented by this change.

Updated 2026-10-28. Work in progress; not all phases or acceptance scenarios are complete.

## Latest Gemini composer fix (2026-10-29)

Confirmed on Gemini's live guest page that Send is absent until text is entered.
The adapter now prepares an enabled composer first, then waits for Send. It
excludes the auxiliary Quill clipboard and preserves existing manual drafts.
The five targeted installed-browser regression fixtures and all 30 intake
regression tests passed. No model prompt
was submitted during the live guest inspection, and no native rebuild is needed.
See `AI_TEST_RESULTS.md` and the Gemini regression logs for evidence and limits.

The user confirmed Google sign-in works in ordinary Chrome. This does not attach
that browser to Control: Google OAuth rejection in the controlled profile remains
a separate transport limitation, requiring a supported normal-browser bridge or
the existing configured API path; no fingerprint/cookie workaround was added.

## Latest: visible actions and durable web reply intake (2026-10-29)

Published after verification: `src/target/debug/concat-ai-control-4232ce1d618b.exe`.
Native build, waited version/GPU smoke and normal-launcher selection succeeded.

`studio_ui_actions.rs` adds the bounded `ui.action` executor. It locates the actual
accessible control in the main Slint tree, scrolls if necessary, highlights its
current bounds, dispatches pointer press/release, and uses the actual field's
SetValue action with value readback. Supported actions are currently `open_factory`,
`open_film`, and `film_idea`. They do not generate a film or claim animation support.
The main-window input hook and separate guard prevent manual input during these
UI operations; Stop/takeover remains available. Browser planning/login is not
covered by the main-app guard. Existing business and account owners are unchanged.

The reference requested as TonyFlow is TobyFlow in `chromex`. Review of its text
intake identified the `data-turn` assistant fallback, whole-message extraction,
turn tracking and stable-text completion patterns. The new adapter supports
`data-turn`, ignores old busy bubbles, reads message text/code blocks, and persists
correlated completed replies before sending them to the native process. A complete
matching receipt can recover a transport failure without sending again. It stores
bounded diagnostic metadata, not cookies/auth headers. It does not copy the
reference's private endpoint hooks or introduce a dependency on its servers.

A plain clarification is accepted only from the exact submitted turn with a final
message marker; native planning then presents it as input needed, never as commands.
Completed JSON plans still pass the existing strict compiler and permission checks.

Validation: full web/MCP regression 48/48; final intake-specific suite 21/21;
receipt unit 1/1; owned-process protocol 3/3; main UI component integration passed
at both 1280×900 and the user's 900×712 logical layout. The integration platform
creates no desktop windows and verified actual navigation callbacks, Film idea
readback, guard show/release, refusal to write without a guard, and rejection of an unregistered action. Computer Use
explicitly denied the separate desktop test app, so physical Windows input blocking
and authenticated production-provider behavior are not claimed as verified.

## Latest: request → plan navigation and web reply intake (verified 2026-10-29)

AI Control now shows one page at a time. Submit opens the execution plan;
Back restores the request form without discarding its draft or stopping the
workflow. History opens the selected plan. The existing composer/task blocks are
reused in the small view crate; the main UI and business owners were preserved.

Web intake reads all rendered reply content, tolerates labels/prose/multiple code
blocks around a correlated envelope, and accepts inner task JSON without that
envelope only when the DOM proves it follows the complete submitted user turn.
Whitespace differences between composer and rendered user bubble are normalized.
Wrong IDs, intervening manual turns, unbound task JSON, ambiguous output and
thinking prose are rejected. Transient SPA read errors retry reads, never Send.
All native plan/edit validation remains active. Older unknown dispatches are not
automatically replayed by installing this fix.

Current published executable: `src/target/debug/concat-ai-control-ab989df940aa.exe`.
Full Node/MCP/browser regression: 46/46; launcher contracts: 14/14; native build,
version and GPU smoke passed. Actual isolated GUI verified submit → plan → Back,
with the original request preserved. Browser checks use installed browsers with
local intercepted fixtures, not authenticated production ChatGPT/Claude accounts.

## Latest: two native windows (23:12 local)

The main app and AI Control now open simultaneously, tiled beside each other.
`concat-control-ui` is a small independent view crate; `studio_control_window.rs`
forwards commands to the existing main-window runtime, workflow store and project
owner. It does not create another runtime/account owner. Editor application reveals
the existing main editor before applying its revision-checked EditPlan.
Closing AI Control only hides it; the main menu reopens it with the draft retained.
Closing the main app hides the auxiliary window and permits normal shutdown.
`CONCAT_AI_CONTROL_EMBEDDED=1` retains the previous embedded presentation for rollback.

Current verified build: `src/target/debug/concat-ai-control-c0d60aebae37.exe`.
The standard launcher selects it through the SHA256 receipt. Build, version/GPU
smoke, 14 launcher contracts and the geometry unit test passed. Actual desktop
review at 1920×1200 / 150% verified both windows, hide/reopen draft preservation,
real workflow creation with an ungranted-network blocker, and cancellation saved
to the same store. No provider request or authenticated web operation was made.
Earlier build-in-progress statements below are historical observations.

Added a durable workflow core, native AI Control page, shared native/MCP owner and seven concrete service adapters. The existing three MCP edit tools are preserved, with three additive Control tools. Existing Film and Factory routes remain present. Codex has an isolated app-server health probe; model execution is explicitly disabled.

The registry currently executes media inspection, library metadata listing, script generation through the existing connection, strict edit-plan generation, Factory render, artifact validation and native edit-plan application. Unsupported commands return an explicit blocker. Model output never supplies arbitrary execution or shell commands.

Verification includes core/protocol/MCP/provider regressions, real FFmpeg and persisted Factory/API integrations, native UI/adapter compilation and a successful full native build. The actual isolated Codex handshake succeeds and reports no authenticated runtime account and no enabled execution. See `AI_TEST_RESULTS.md` for exact evidence and limits.

The native executable now builds and passes version/smoke checks. In its actual GUI, an operator-attached image and real narration completed inspect → render → validate, producing a checked 6-second 720×1280 audible video. Closing/reopening the application and loading the saved workflow restored its receipts without rerendering. The existing Film page and its two saved projects remain present. Review output: `dist/kedi-test/review-ai-control-20261028/film.mp4`.

Added bounded input/output contracts, review of resolved inputs before approval, explicit per-step JSON input correction/checkpoint invalidation and numeric duration enforcement. A requested 3-minute render refuses a short narration instead of silently padding or returning a few seconds. Existing Factory callers retain their previous duration behavior. `CONCAT_AI_CONTROL_DISABLED=1` disables the new runtime for rollback without removing manual modules.

The first full build exposed a missing `AiControl` root Slint export. It is fixed, and the harness now imports the actual public root rather than only the page. AI generation uses the existing structured client (larger bounded context, 4096 output tokens and explicit truncation checks), rather than the short 256-token chat route. Multi-action timeline requests involving music/captions/export explicitly report missing adapters instead of applying a partial edit and claiming the whole request is done. Bridge timeout responses now preserve outcome uncertainty.

The current source exports `control_plan.rs` and registers an eighth capability, `ai.plan`, using the existing authenticated structured client. Default submissions ask the model to build a bounded DAG from the real registry; explicit render-assets and read-only library templates bypass planning. A plan is compiled and transactionally persisted with pending child steps before any child executes. The six compiler tests pass. This new wiring is **not present in the verified 15:34 executable** and has not yet passed an authenticated model-turn or full desktop acceptance run. Requirement descriptions are model declarations, not proof that every natural-language outcome was covered.

Current source also adds validated-result open/reveal callbacks and restores command/script/model/template/audio form state from workflow history. These are pending full native GUI verification. Project creation and live timeline export are still missing Control executors; selecting a new project explicitly reports that blocker. Existing-project selection reuses the native picker and refuses switching while an editor project is open.

The native Slint build is split into `concat-ui` so backend changes can reuse compiled UI code. The workspace now contains 20 crates. The first split attempt compiled UI but its root check used a studio dependency built before `control_plan` was exported, so it failed; that attempt did not produce a new verified executable. The current full rebuild is `.runtime/ai-control-workspace-native-build.log`. Keep the verified executable until its checks pass.

Current source now opens a dedicated full-window AI workspace from the AI Control route, sharing the same workflow/project/account owners. Steps are numbered blocks with dependency labels, translated states and actual result text/counts where available. Raw JSON, MCP controls and Codex diagnostics are advanced details. Video deletion confirmation is a centered window-level modal rather than part of the scrolling library.

The new owned browser transport supports planning, narration and scoped edit-plan responses through ChatGPT/Gemini/Claude pages. It types into the known composer, clicks Send once, checks a correlated response and preserves manual drafts. Native project changes still use the existing validated EditPlan/undo path. It does not expose arbitrary clicks on arbitrary sites. Website login remains manual; API transport is retained. Sources cannot change after an AI dispatch; pre-dispatch changes invalidate inputs/approvals through the durable core.

The MCP/browser suite passes 36/36 using actual browser processes with intercepted local fixtures. Browser-process protocol tests pass 3/3; core 9 and presentation 2 remain passing, and the current plan suite passes 7/7 including provider rebinding before dispatch and approval invalidation. The actual current service and native adapter sources compile independently. These checks do not prove authenticated production website compatibility or a new desktop GUI acceptance run.

The user prioritized ChatGPT/Claude for writing and planning. Both share the browser transport; Gemini remains available. The current user-owned application is still using the verified `src/target/debug/concat.exe`. It has not been closed or replaced. A separate preview executable build is queued after UI codegen, with fresh backend dependencies: `.runtime/ai-control-workspace-preview-build.log`. This avoids replacing the running EXE. A native GUI test against the normal workflow store requires closing the existing application first; only one runtime may own that store.

Outstanding: full capability coverage, generic planning/provider routing, live project lifecycle/export, external MCP client, Codex runtime execution, protected manual-edit continuation, richer recovery UI and the master prompt's complete A–D acceptance runs.

## CozyCat presentation revision

The user-selected UI reference was inspected at pinned commit
`abf3a55cdf915fcb6f0238f3c142b45e5bcc3a54`. Actual upstream code contains accordion
and animation variants, not an AI/web automation engine. MIT notice is retained.
Native presentation is now adapted through `control-chrome.slint`: warm surfaces,
compact actions, content-sized foldouts, two-column/stacked layout and contextual
workflow blocks. Font and automation owners remain the existing Kedi services.
See `AI_UI_REFERENCE.md` for source and validation details.

Read-only visual review passed at 1100/520 px, including mouse/Enter accordion
interaction and scrolling. The prior workspace preview built successfully at
20:04 and passed version/smoke with actual Intel Iris Xe / DX12. Its UI predates
this Cozy revision. The new full build is `.runtime/ai-control-cozy-native-build.log`;
the user's running original app remains open and unchanged.
