# AI Control verification

## Nexora / Grok batch adapters (2026-10-29)

See `AI_NEXORA_BATCH.md` for the executable contracts and limitations. Live
read-only health/catalogue checks returned Nexora protocol v1 READY and two
profiles; no profile/account/job was operated. Batch/compiler/display regression:
16 passed, 3 unchanged optional Node protocol tests ignored
(`.runtime/ai-control-nexora-batch-regression.log`). This includes 100 independent
jobs expanded to 400 durable steps, scope/reference resolution, actual JSON
store save/load, duplicate/oversize/incomplete input rejection and count checking.
The 400 execution results are protocol fixtures, not generated media.
Core workflow regression: 11/11 (`ai-control-batch-workflow-regression.log`).
Existing Grok transport/recovery regression: 20/20 controlled-HTTP checks
(`ai-control-grok-adapter-regression.log`). No live Grok generation, account
login, publishing or desktop click test was performed.
Final native build succeeded; version and Intel Iris Xe DX12 smoke exited 0.
Launcher contract: 14/14. Published verified executable
`src/target/debug/concat-ai-control-b1e2fbb15152.exe`, SHA256
`b1e2fbb151523da436c0a7a8b0e087fe0967ba1ff92b34cf228cc4f7a1f431bc`.
The normal launcher now selects it. The previous verified build and user data
remain available; this task did not operate production social accounts.

## Product-script plan response correction (2026-10-29)

The user's failed script request has a completed Gemini turn receipt and a failed
native planner. Read-only inspection found an `unsupported` object without the
mandatory `missing` field. This is a schema failure, not proof that `ai.script`
is unavailable. No stored turn or workflow was modified, resubmitted or repaired
by inventing a capability/step.

Gemini extraction now prefers final `message-content`/`markdown-main-panel`
content and excludes thoughts panels, including their code blocks. A regression
fixture presents stable draft JSON for 3.5 seconds before a real final question;
the reader must wait for the final question and click Send only once. A second
fixture with thoughts only must time out without accepting that JSON. Both passed
(`.runtime/ai-control-gemini-final-message-tests.log`). These are local browser
fixtures, not a live authenticated model round trip; the saved receipt alone
does not prove which Gemini DOM element produced the malformed object.

The planner prompt explicitly distinguishes writing a script from rendering a
video: no assets/audio/editor are required for script-only requests; absent
product facts should produce a Vietnamese `needs_input` question. Strict schema
validation remains enforced. Malformed responses and the legacy English error
are displayed as a Vietnamese formatting error with a next action.
Native plan/display tests: 12 passed, 3 unchanged optional protocol tests ignored
(`.runtime/ai-control-script-plan-tests.log`).
Full web regression: 32/32 passed (`ai-control-gemini-final-regression.log`).
Native build completed in 4m03s; version and Intel Iris Xe DX12 smoke exited 0.
Published `src/target/debug/concat-ai-control-a55f8f3d7beb.exe` through the
hash-bound launcher. Save work and restart Kedi, then submit a new request;
the old failed workflow/receipt remains unchanged and is not automatically retried.

## Blocked planner Resume correction (2026-10-29)

Resume previously used `!cancelled && !finished`, which enabled the button for
a failed, already-dispatched non-replayable planner. The Resume callback could
not retry that step and simply reached the same blocked graph again.
`Workflow::can_continue` now checks runnable dependencies and actual safe retry
eligibility. Both publication and the Resume callback use it. Active execution
retains Stop. Missing permission before dispatch remains resumable; unknown
writes and pending children of failed steps do not become runnable.

Unsupported responses retain the provider reason in the durable error with an
explicit unsupported-capability marker; the normal display explains the blocker
in Vietnamese. The exact legacy message reported in the screenshot is also
translated without editing saved workflows. No generative-animation capability
was added or claimed by this fix.

Verification logs: `.runtime/ai-control-blocked-resume-tests.log` and
`.runtime/ai-control-blocked-resume-native-build.log`.
Workflow tests: 11/11 passed (`ai-control-blocked-resume-workflow-tests.log`),
including blocked dependencies, exhausted retries, unknown writes, missing
permission before dispatch, cancellation and safe read retries. Display/plan
checks: 11 passed, 3 unchanged optional Node protocol tests ignored. Native build
finished successfully; version and DX12 smoke exited 0. Published verified
`src/target/debug/concat-ai-control-c09871541b87.exe` through the existing
hash-bound launcher. No authenticated model call or desktop input test was run.

Updated 2026-10-28. Results below distinguish unit, protocol, compile-only and actual application checks.

## Gemini empty-composer deadlock (2026-10-29)

Live public Gemini guest-page inspection in an isolated browser context confirmed
an enabled `rich-textarea .ql-editor[role="textbox"]` while no Send button existed.
After inserting an unsent diagnostic draft, `button[aria-label="Send message"]`
appeared and became enabled. No account sign-in or model request was submitted;
only that owned test tab was closed afterwards.

The adapter had required both composer and Send before preparing the prompt,
creating a circular wait on this UI. Readiness/preparation now require only an
enabled composer. Send is discovered and checked after input, with one click only.
Gemini's auxiliary `.ql-clipboard` is excluded from input candidates.

`.runtime/ai-control-gemini-composer-tests.log`: 5/5 installed-browser fixture
checks passed: delayed Send creation on Gemini/ChatGPT/Claude, ignoring Quill's
clipboard and preserving a manual Gemini draft even without a Send button.
Full intake regression passed 30/30 tests with zero failures:
`.runtime/ai-control-gemini-regression-tests.log`.
Provider HTTP is locally intercepted in these regression tests. A live model
round trip was not tested.

This is a JS adapter change; no EXE rebuild is needed. Stop the currently running
old Node request and create a new request to load it. Google OAuth rejection is
separate: the user confirmed normal Chrome signs in successfully, while the
controlled-browser rejection remains unsupported by the current transport.

## Google sign-in rejection handling (2026-10-29)

The screenshot reports Google OAuth `signin/rejected`, not a model response.
The browser adapter now recognizes this state from URL metadata in the controlled
tab or another page of its owned browser. It stops with a specific error instead
of polling for a composer/model response until an unknown-result timeout. Before
Send it remains a known, undispatched login failure; after Send it preserves
dispatch uncertainty and never retries. It does not read auth forms/credentials,
click sign-in controls, spoof browser identity or copy another browser's cookies.

`.runtime/ai-control-auth-unit-tests.log`: 6 passed. Focused installed-browser
fixtures in `.runtime/ai-control-auth-browser-tests.log`: 3 passed, covering the
current tab, OAuth popup metadata and rejection after dispatch. All provider/auth
HTTP is intercepted with local fixture HTML; no real account sign-in was tested.
Broader intake regression is `.runtime/ai-control-auth-regression-tests.log`.

This changes the external JS adapter only; the native executable does not need
rebuilding. Stop an existing request to end its older Node process. This patch
does not make Google permit login in the CDP browser. An authenticated normal-
browser extension transport is not currently implemented for Control; a normal
browser login alone does not connect that browser to the current adapter. The
existing configured API connection is a separate supported execution path.

Primary references: [Google supported-browser sign-in](https://support.google.com/accounts/answer/7675428?hl=en)
and [OpenAI login-method guidance](https://help.openai.com/en/articles/7426629-why-cant-i-log-in-to-chatgpt).

## Latest visible UI / reply intake checks (2026-10-29)

- Final native artifact: `src/target/debug/concat-ai-control-4232ce1d618b.exe`,
  SHA256 `4232ce1d618befbfd32490f17a7c3b9e7d68b6b5c3cb138bee91cf0bda5c0cd0`.
  `.runtime/ai-control-visible-native-build.log`: exit 0, final rebuild 1m17s.
  `.runtime/ai-control-visible-version.log`, `ai-control-visible-smoke.log`: exit
  0, 0.2.7 and actual Intel Iris Xe / DX12. Standard launcher selects this hash.
- Cleanup of three obsolete task builds was rejected twice with `blocked by policy`,
  including a second literal-path attempt after confirming the running app and
  current published hash. These files remain (798,185,472 bytes total):
  `.runtime/visible-ui-gui/concat-visible-gui.exe`,
  `.runtime/visible-ui-memory-test.pdb`, and
  `src/target/debug/concat-ai-control-e1500b163909.exe`. Source was excluded from
  the safer second cleanup attempt. No alternate deletion API was used.

- `.runtime/ai-control-visible-final-web-tests.log`: 48/48 full MCP/browser tests.
  `.runtime/ai-control-visible-final-intake-tests.log`: final 21/21 intake tests
  after adding completed plain-clarification support. Intercepted local provider
  fixtures cover data-turn DOM, stale busy bubbles, final-message markers, strict
  correlation, one Send, transient reads and complete streaming output.
- `.runtime/ai-control-visible-receipt-tests.log`: 1 passed, 3 explicitly ignored
  protocol cases. `.runtime/ai-control-visible-protocol-final-tests.log` then ran
  those 3/3 cases with installed Node and `--test-threads=1`. The earlier parallel
  protocol run had a timestamp-named fixture collision, not a transport failure.
- `scripts/test-visible-ui-memory.ps1`: actual generated main UI plus production
  actor source on MinimalSoftwareWindow; no OS windows, no account or network.
  It dispatched pointer events opening Factory and Film, pasted into the real Film
  field, checked FilmStudio.idea, guard show/release and unknown-action refusal.
  It also checked that a busy actor refuses dispatch when its guard is absent.
  Passed first at 1280×900, then at 900×712 logical pixels. The integration caught
  an incorrect requirement for a Default accessibility action on pointer-clickable
  navigation buttons; fixed to check actual button role before final publication.
- `.runtime/ai-control-visible-memory-tests.log`: final in-memory component check
  1 passed; compiler provenance in `ai-control-visible-memory-build.log`.
- Desktop Computer Use refused the owned test app with
  `Computer Use was not approved to use concat-visible-gui`. No alternate desktop
  automation was used. Physical Windows input blocking/overlay behavior and live
  authenticated provider compatibility still require desktop acceptance.

The bounded actor supports opening Factory/Film and filling the Film idea. A
full visual agent across every app feature, animated-film generation and exact
10-second creative production are not implemented by these three actions.

## Latest navigation and web intake verification (2026-10-29)

- `.runtime/ai-control-pages-native-build.log`: exit 0, 5m31s, main UI reused.
- `.runtime/ai-control-pages-version.log`, `ai-control-pages-smoke.log`: exit 0,
  0.2.7 and actual Intel Iris Xe / DX12. Published hash:
  `ab989df940aa7b56517deecc2c34cbe36936f892b7acd16f4631a7a0af3f7412`.
- `.runtime/ai-control-pages-all-node-final-tests.log`: 46 passed, 0 failed,
  using `node --test --test-concurrency=1 server.test.mjs web-chat.test.mjs
  control-web.test.mjs web-chat-browser.test.mjs control-web-browser.test.mjs`.
  New cases cover later code-block extraction, DOM-bound inner JSON on all three
  providers, rejecting unbound/stale/intervening turns, transient read errors,
  streaming completion and exactly one Send. Provider HTTP is locally intercepted.
- The earlier parallel run had five Windows EBUSY fixture-cleanup failures.
  Cleanup now waits/retries those exact temporary profiles; the complete final
  sequential rerun passed. No real provider credentials/request were used.
- Actual desktop, isolated portable profile, 1920×1200 at 150%: initial request
  page has no plan panel; Submit replaces it with the actual one-step plan and
  a Back button. Back restored `Kiem tra chuyen trang va quay lai` unchanged.
  The network grant remained off, so the actual workflow stopped at permission
  input rather than calling a provider. Main app remained visible throughout.
- `scripts/test-kedi-launcher.ps1`: 14 passed. Normal launcher version check
  selected `concat-ai-control-ab989df940aa.exe`, exit 0.

This fixes tested reply-format/DOM intake paths. Authenticated production web
compatibility was not reverified; previously unknown outcomes are not resent.

## Latest separate-window verification

- `.runtime/ai-control-dual-native-build.log`: native build exit 0, final DPI fix
  rebuilt in 1m36s; the large main UI crate was reused.
- `.runtime/ai-control-dual-version.log` and `ai-control-dual-smoke.log`: exit 0,
  version 0.2.7, actual Intel Iris Xe / DX12 smoke success.
- `scripts/test-kedi-launcher.ps1`: 14 passed, including candidate confinement,
  receipt hashes and rollback. Standard `-UseExistingProfile -Version` selected
  `concat-ai-control-c0d60aebae37.exe`, exit 0.
- Actual-source geometry unit test: 1 passed for 1280/1366/1920/2560 logical widths.
- Computer Use, isolated portable profile: both native windows visible at 150%
  DPI; main at physical (8,40), 900×712 logical, AI panel 340 logical wide. Closing
  AI alone left main open; sidebar reopened it with `Giu lenh khi dong bang AI`
  retained. Submit generated real workflow `control-55be76dc0a10d4c3ede6e42beab978c3`;
  one actual `ai.plan` block reported the missing AI permission. Cancel persisted
  revision 2, `cancelled=true`, attempts 0. No external AI request was sent.
- Closing the main test window closed both windows and exited the event loop
  normally. Removed 619,520,377 bytes of verified task-owned obsolete/test build
  artifacts afterwards; retained the current executable, baseline rollback,
  workflow receipt, source and logs. No user video/project/model was deleted.
- Verified executable SHA256:
  `c0d60aebae376b71df9aee5551c7fb748c5535403aa33cc84118b958db2ed235`.

This validates the new window bridge and local workflow controls, not an
authenticated ChatGPT/Claude round trip or a new timeline-edit acceptance run.
The older build status paragraphs below describe earlier checkpoints.

| Check | Result | Evidence / limits |
|---|---|---|
| Workflow core | 9 passed | `.runtime/ai-control-final-core-tests.log`; DAG, schema bounds, exact resolved approval, permissions, duration, stale receipts and crash recovery |
| MCP and browser-adapter regressions | 28 passed | `.runtime/ai-control-mcp-current-tests.log`; includes additive tool contracts and prior browser behavior, not native Control E2E |
| Codex protocol unit tests | 2 passed | `.runtime/ai-control-final-core-tests.log`; protocol errors and authentication/execution separation |
| Native editing bridge regression | 3 passed | `.runtime/ai-control-edit-bridge-tests.log`; actual local queue handoff, DPAPI and authentication checks |
| Existing AI/provider client regression | 19 passed, 1 separately exercised below | `.runtime/ai-control-provider-regression.log`; bounded loopback fixtures, structured replies, exact model receipt, truncation, credentials and no uncertain replay; not provider production inference |
| Bundled local AI runtime | 1 passed | `.runtime/ai-control-local-runtime-live.log`; starts/restarts actual bundled OmniRoute in an isolated profile, verifies authentication, encrypted persistence and process shutdown; no production account or paid inference, 186.27 seconds |
| Final MCP contract regression | 7 passed | `.runtime/ai-control-mcp-contract-final.log`; overlaps the full suite and adds a timeout ambiguity case; do not add overlapping counts |
| Native Control adapter/UI harness | Passed | `.runtime/ai-control-root-export-check-current.log`; checks actual root Slint exports as well as the adapter/page; editor/provider seams deliberately unavailable |
| Factory regression with real FFmpeg | 8 passed | `.runtime/ai-control-factory-regression.log`; explicit duration rejection and matching voice, motion, captions and sound |
| Legacy Rust Factory adapter | 3 passed | `.runtime/ai-control-factory-legacy-tests.log`; existing render signature, real saved Shopee image/voice, Unicode paths, cancellation and validation remain working |
| Durable core + Rust Factory integration | 1 passed | `.runtime/ai-control-real-render-test.log`; real supplied product image/narration, human approval, exclusive store, reopen checkpoint, actual 6-second audio/video output |
| Native media probe integration | 3 passed | `.runtime/ai-control-native-probe-tests.log`; exact native probe reads the supplied image/voice and validates the real 720×1280, 6-second output with audio; missing output is rejected |
| Existing native API service integration | 1 passed | `.runtime/ai-control-existing-api-live.log`; separate test project creates/imports/places real media, undo/redo, saves/reopens, refuses an outside write and exports with native engine; PCM has audible samples |
| New bounded model-plan compiler | 6 passed | `.runtime/ai-control-plan-current-tests.log`; executable requirements, pending-only steps, no model approvals/success, cycles, exact artifact validation references and transactional DAG expansion/reopen. Registered in current source, absent from verified binary; not authenticated general-planner acceptance |
| Current MCP contract and chat regressions | 18 passed | `.runtime/ai-control-mcp-planner-tests.log`; additive workflow template enum/default, rejects unknown template, preserves operator-only permissions; overlaps prior MCP tests |
| Current service source compilation | Passed | `.runtime/ai-control-studio-direct-check.log`; isolated rustc build against the exact cached dependency graph, without modifying Cargo artifacts; not native GUI/runtime acceptance |
| Current Control page and native adapter compilation | Passed | `.runtime/ai-control-ui-direct-build.log` and `.runtime/ai-control-adapter-current-check.log`, exit 0; actual current Slint page/root exports and adapter, with editor/provider seams unavailable; compile-only |
| Full native application build | Passed | `.runtime/ai-control-native-build-current.log`, exit 0, 66m11s; initial missing root export fixed |
| Native launcher version / smoke | Passed | `scripts/windows-studio.ps1 -RunBuilt -Version` and `-Smoke`, exit 0; real Intel Iris Xe / DX12 adapter |
| Native GUI local render | Passed | Three real steps inspect/render/validate, operator-attached actual image + narration, network and MCP Control remain off; workflow `control-6f630840930d3ca9fb2f7141c99a7ccb` |
| Native GUI restart / history | Passed | Normal Alt+F4 exit 0, reopen same binary, select saved workflow; actual inputs/receipts and all three Done steps restored without rerendering |
| Film navigation compatibility | Passed for navigation/data presence | New binary opens existing Film page and shows both `Phim_9964` and `Bát Tràng Ký Sự (Mẫu)`; no new movie generation or account action |
| Isolated installed Codex app-server probe | Passed | `.runtime/ai-control-codex-live-probe.log`: protocolReady=true, authenticated=false, executionEnabled=false; no model turn or copied login |
| Acceptance A–D | Not passed / not yet run | Project import/export, full planning and protected manual continuation remain incomplete |

## Dedicated workspace and browser transport revision

- `.runtime/ai-control-workspace-all-node-tests.log`: **36 passed**, including existing MCP/chat regressions, three real browser DOM adapters using intercepted local pages, correlation/size limits, draft preservation and cancellation without duplicate sends. No live model/account call.
- `.runtime/ai-control-workspace-rust-tests.log`: **3 passed**, actual owned Node protocol processes, successful response, invalid/unknown dispatch result and cancellation. These are protocol fixtures, not inference or GUI acceptance.
- `.runtime/ai-control-workspace-core-regression.log`: core **9**, presentation **2** passed; `.runtime/ai-control-workspace-plan-current.log`: final planner suite **7 passed**, including changing source before dispatch, invalidating old approvals and refusing source changes after a dispatch.
- `.runtime/ai-control-workspace-slint-check.log`: page/root Slint exports parse/type-check. `.runtime/ai-control-adapter-current-check.log`: current native adapter including source rebinding compiles, exit 0. `.runtime/ai-control-studio-workspace-check.log`: actual service sources compile against the pinned cached graph. These remain compile-only checks.
- `.runtime/ai-control-workspace-native-build.log`: current full native rebuild in progress. The prior split build failed its root check against a stale studio dependency and did not replace the working binary. No full native GUI result is claimed for this revision.
- `.runtime/ai-control-workspace-preview-build.log`: preview build queued behind the full UI compile, targeting a separate executable under `dist/kedi-test` while the user's existing application remains open. Not yet a verified artifact. No running user app was terminated. The normal workflow store cannot be shared by two GUI runtimes.

Update: that workspace preview finished at 20:04; waited `--version` and `--smoke`
both exited 0 (`.runtime/ai-control-workspace-preview-version.log` and
`.runtime/ai-control-workspace-preview-smoke.log`), with actual Intel Iris Xe / DX12.
This is the workspace revision before Cozy presentation changes.

## CozyCat presentation revision

See `AI_UI_REFERENCE.md`. Actual native Slint UI was reviewed read-only at 1100 px
and 520 px using an existing workflow receipt. Mouse expansion, Enter collapse,
stacked narrow layout and scrolling were observed working after fixes. Actual
workspace/page Slint checks passed. The normal application and its workflow store
were not controlled by the read-only harness. Current full desktop preview build
is `.runtime/ai-control-cozy-native-build.log`; new desktop smoke/GUI acceptance
is pending. Existing web/MCP 36-test evidence is unchanged; no automation behavior
was imported from CozyCat UI.

The compile-only harness in `tools/ai-control-check` does not run the real editor or prove complete functionality. Previous Factory motion/audio validation belongs to the prior change and is not counted as AI Control E2E evidence.

The real render integration is backend integration, not the master's complete A–D GUI acceptance. Its output is `.runtime/ai-control-real-render/run-1793171070514420400/render-1/film.mp4`, with video and audio streams and a probed duration of exactly 6 seconds. It preserves the workflow store and artifact for review. Temporary media fixtures from the Python regression suite clean themselves up.

The artifact SHA256 is `605D8FCDC392AF29FBAF0C57FC8E85E20908A08C23782FFF8415F090C8150CEE`. `contact-sheet.jpg` beside the integration runs is a review aid showing the actual product, caption changes and camera framing.

The existing-API export is `.runtime/ai-control-existing-api/run-1793173977062662300/editor-export.mp4` (360×640, approximately 6 seconds, audible audio). This proves the existing **headless API** business seam, not a production GUI LiveSessionBridge or Control/MCP exposure of those operations. The test uses isolated AppDirs and never opens or rewrites a user project.

## Native build and review artifact

Verified executable: `src/target/debug/concat.exe`, version 0.2.7, 294,775,296 bytes, SHA256 `0239F77312D36D257AD76C67795F4F104D485708C5C4A9761D25BDF001DD0180` (2026-10-28 15:34 local). `-RunBuilt` uses this executable with the normal existing profile and runtime DLL paths; it does not rebuild.

The actual GUI output and persisted receipt were copied to `dist/kedi-test/review-ai-control-20261028/film.mp4` and `workflow.json`. The video has a 720×1280 video stream, an audio stream, duration 6.000000 seconds, size 1,156,829 bytes, and the same verified SHA256 as the backend integration artifact. GUI rendering used the existing narration file; it does not prove a provider-generated script, online TTS or full workflow A.

Observed UI gaps: reopening restores workflow state/assets but not the command/script form text; result preview still uses existing library/editor paths rather than an inline Control video player; the generic Studio header appears below the Control page. Pause/cancel/manual-edit continuation still needs broader native GUI acceptance. Full A–D remains incomplete.

Those UI observations describe the verified 15:34 binary. Current source addresses form restoration, result open/reveal and header placement, and moves the workflow monitor above the request form. The new `concat-ui` full build is still running (`.runtime/ai-control-split-ui-native-build.log`); these changes have not passed a new desktop GUI run. The model planner has compiler tests and source wiring, but no authenticated model inference was run for this revision.

After verification, 4,338,748 bytes of task-owned legacy adapter outputs and temporary test executables were removed. Sources, logs, the latest Control render/checkpoint and user projects remain available. No source/SDK/model cache or unrelated disk folders were deleted.

Reproduce the core checks from `src` with `CARGO_PROFILE_TEST_DEBUG=0 cargo test -p concat-studio --lib ai_control:: --offline`, followed by the `codex_runtime::` filter. Run `node --test server.test.mjs web-chat.test.mjs web-chat-browser.test.mjs` in `tools/ai-edit-mcp`. Run `scripts/test_factory_edit.py` with the existing speech Python and `CONCAT_TEST_FFMPEG` pointing at the installed FFmpeg executable. Native SDK environment requirements remain those of the existing Windows build scripts.
