# Zero-instruction native Studio redesign

## Contract (2026-10-19)

The product is a native Slint desktop tool, not a web landing page. First-time
operators need to connect TikTok, choose a character and voice, preview it, open
the clean output, and follow chat. Primary actions must reflect actual callback
behavior; no fake Save, project field, connected state, paid request or publishing.

Direction: quiet operational dark dashboard; existing Floword/Theme and native
controls, flat surfaces, 8/16/24 px spacing, blue for the next primary action,
green for verified state, amber/red for recoverable problems. No extra dependency.

Observed local evidence: the previous LIVE page had a wide unwrappable toolbar,
three fixed minimum-width columns, a small 290px preview, duplicate output and
connection controls, a default portrait presented before selection, technical
catalog IDs and multiple always-on setup paragraphs. The shared shell included
decorative project/Page inputs and a Save button with no callback. The AI Keys
canvas described router functionality instead of exposing actual providers.

## LIVE hierarchy and state contract

1. Controls: connection, character source/choice, voice, clean output.
2. Preview/content: selected subject and actual playback state; product choice.
3. Interaction: event filters/feed, opt-in automatic replies, text/speak/send.

Model/engine diagnostics, room inspection, reload and sales automation stay
reachable in advanced settings. Existing event receiver, reconnect state machine,
AI/TTS mutation callbacks, archive safety and API contracts remain unchanged.

| State | Visible result | Safe next action |
|---|---|---|
| Loading | Stable layout, explicit loading label, busy controls | Wait/cancel when callback supports it |
| Empty | No invented avatar/product/events/keys | Choose or import actual data |
| Error | Existing sanitized error plus contextual retry | Retry only the relevant read/connect action |
| Partial | Available voices/characters remain usable; missing AI shown separately | Configure only missing capability |
| Success | Verified connected/playback/applied label, real content | Follow chat / open output |
| Permission | Explicit AI auto-reply/web auto-apply/quota/import rights boundary | User opts in; never triggered on mount |

Read-only catalog hydration may run once behind existing loaded/busy guards.
Never automatically connect TikTok, turn on replies, scan/import/delete media,
start/stop a browser, call AI/TTS, publish, or refresh shared catalogs repeatedly.

## Responsive and verification targets

Target content widths: 1600×900, 1080×720, 740×720, 360×900. These exercise wide
desktop, 1366-class desktop beside the existing sidebar, narrow desktop, and a
constrained pane. LIVE uses 3 columns when they fit, 2 plus chat below, then one
column. Long real names/messages must wrap; chat has its own bounded scroller.
Also capture provider/local-avatar/editor/settings/production/empty states.

Use real Slint offscreen rendering with explicitly isolated layout fixtures, not
fake runtime data. Capture artifacts under .runtime/zero-ui-review. Verify native
keyboard/pointer callbacks with stubbed counters: no provider, TikTok or posting.
Then run focused regressions and one native build, not a redundant full check.

## Audited shared-state hazards

- OpenTalking avatar selection already starts builtin LIVE motion and clears local
  active ID; don't invoke it as an automatic catalog/default hydration step.
- AI model reload resets selected index; hydrate once, retain manual reload.
- Workflow catalog reload resets selection/preflight; run-list reload invalidates
  preflight. Do not auto-refresh during edits/confirmation.
- In the local-first build, cloud production/bulk/publishing may be unavailable;
  health is not execution readiness. Present unavailable actions honestly.
- Local avatar archive needs exact-ID confirmation, no active LIVE/scanning,
  and must preserve original source and recoverable library copy.

The native render/interaction evidence and untested limits are recorded separately
after implementation; source-only inspection does not satisfy acceptance.

## Implemented surfaces

- LIVE: bounded controls, large native video preview, contextual product actions,
  event feed, explicit reply toggle and advanced diagnostics. No automatic connect,
  voice synthesis or paid AI request. Catalog hydration uses loaded/busy guards.
- AI connections: real provider list and friendly service chooser; trial requests,
  key deletion, custom IDs and router information remain explicit/advanced.
- Characters/MC: choose video before name/scan, recoverable archive confirmation,
  local library and builtin catalog, history actions, advanced generation inputs.
- Editor AI: chat connection first; API prompt only in API mode; manual JSON/copy
  and MCP under disclosures. Auto-apply and context-sharing boundaries retained.
- Overview/shell, Nexora/settings, production, scripts, local channels, products,
  bulk CSV, exported-video ledger, and Auto Shorts follow the same control system.

Unsupported cloud workflows and social publishing are labelled unavailable rather
than advertised as ready. Their existing native contracts are not changed.

## Interaction corrections found during review

- A Slint horizontal status layout recursed during layout measurement: replaced
  its self-referential minimum with an intrinsic, bounded status layout.
- Grid cards stretched labels and source tabs: start-aligned their body and
  bounded tab height, retaining a large preview instead of stretched controls.
- A focused disabled shared button still accepted Space: the keyboard handler
  now checks enabled explicitly; native input smoke exercises this regression.
- Empty ready avatar picker now chooses a video and routes to the creation form,
  without automatically scanning it. The separate scan confirmation remains.
- Restored manual fresh-context copy and MC generation confirmation during the
  callback audit. Loading connection no longer incorrectly says disconnecting.
- Editing/replacing CSV clears its old validation presentation before recheck.

## Verification commands

Native UI review (isolated software renderer, no runtime/provider/user data):

```powershell
cargo build --manifest-path tools/studio-preview/Cargo.toml --target-dir src/target
powershell -NoProfile -File scripts/ui-responsive-check.ps1
```

The suite renders 22 states/pages at 320, 360, 560, 960, 1440 px, and checks native
API prompt input, fresh clipboard copy, LIVE pointer/Enter/Space/busy behavior,
advanced disclosure, opt-in AI, video selection without automatic upload, and
native wheel scrolling to the lower controls of a constrained LIVE page.
The contract's four LIVE viewports are reviewed separately.

Regression scripts were updated to assert current callback/state behavior, not
obsolete literal wording, container types or previously removed cloud login.
No native authentication, project-revision fencing, TTS, archive, paid-request
confirmation or reconnect assertions were removed to accommodate the redesign.

Limits: layout fixtures prove presentation, not TikTok/AI/provider availability.
This task does not send real viewer messages, invoke paid AI, create real jobs,
post socially, or mutate the user's character library. A screen-reader session
and extended real LIVE soak are not part of the isolated UI run.

## Final result (2026-10-19)

- Native preview build and 110 renders / 5 native input checks: pass.
- Four contract LIVE viewports inspected; narrow lower chat reached by wheel.
- JavaScript structural/runtime regression checks: 43/43 pass.
- Avatar Python suite: 13 run, 12 pass, 1 skipped.
- Native debug EXE build: pass (19m04s on this memory-constrained machine).
  Existing missing-docs, unused-code and unrelated padding warnings remain.
- New-EXE integration smoke: connection fixture / disconnect pass; real motion
  decoder 12 frames pass; real local avatar runtime + 50 frames + LIVE/output
  match + recoverable archive + active-LIVE guard + original preservation pass.
- Evidence schema checker and diff whitespace check: pass.

Executable: `src/target/debug/concat.exe` (built 2026-10-19 20:47 local time).
Local visual/input report: `.runtime/zero-ui-review/evidence.json` and PNGs beside
it. No real viewer posting, paid AI calls, user-library mutation, push or deploy.
