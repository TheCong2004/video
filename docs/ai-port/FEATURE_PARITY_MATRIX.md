# Feature parity matrix

Statuses describe the port, not whether a similar primitive already exists.
`TESTED` means automated local tests; only a real Concat GUI acceptance run may
use `E2E_VERIFIED`. None currently qualifies as E2E_VERIFIED.

| Capability | Source | Status | Evidence/gap |
|---|---|---|---|
| Atomic native batch including ID mint rollback | Native foundation | TESTED | Nested failure regression; prior mint consumption reproduced |
| Isolated draft, reserved IDs, opaque owner/revision, one undo | OpenChatCut/WeftCut behavior | TESTED | Native unit tests; worker snapshot test being added |
| Structured plan parse + sequential created aliases | Cartcut/OpenChatCut behavior | TESTED | Initial split/remove/move/shortening trim/title vocabulary; 6 automated tests |
| GUI-owned Session planning/approval boundary | Native foundation | TESTED | Host worker snapshot, stale/reopen/undo and disk-save tests; GUI acceptance pending |
| Structured provider EditPlan route | OpenChatCut behavior | TESTED | 11 backend mocks + native response fixtures; real provider not yet verified |
| Full agent tool execution/usage budget/durable runtime | OpenChatCut | NOT_STARTED | Structured proposal endpoint is not full agent parity |
| GUI agent pane/diff/composited draft preview/approval | OpenChatCut | COMPILES | Native dock/controller wired; CheckOnly passed; GUI acceptance pending |
| Native draft save/preview/export acceptance | Native/OpenChatCut | TESTED | Real generated footage, title timed pixels, disk reopen and MP4 export; GUI E2E pending |
| Named checkpoints, work-session lease, activity, durable recovery | WeftCut | NOT_STARTED | Existing native undo is not checkpoint parity |
| Full editing skills/styles/motion/effects/audio/keyframes | Cartcut | NOT_STARTED | First vocabulary does not claim full plan parity |
| Composited timeline contact sheets/text metrics/programs | Cartcut | NOT_STARTED | Existing renderer reusable, no new contact sheet service |
| Word ASR alignment, transcript edits/fillers, Vietnamese captions | OpenChatCut | NOT_STARTED | Existing segment ASR insufficient for word alignment |
| Confident silence compression, detected beats/scenes | OpenChatCut | NOT_STARTED | Must preserve source-time/revision binding |
| Semantic cache/search, subject tracking/reframe | OpenChatCut | NOT_STARTED | Model/license/readiness and cache tests pending |
| Authenticated scoped MCP mutating live GUI Session | WeftCut/Cartcut/OpenChatCut | NOT_STARTED | Headless Api currently owns separate Sessions; bridge required |
| Generation dependency graph/capability checks | Velorn | NOT_STARTED | Existing Promax jobs reusable, no full graph port |
| Verified generation artifacts/import/dedup/assembly | Velorn | NOT_STARTED | No paid job/model download executed |

## Follow-up implementation — 2026-10-15

These rows supersede the corresponding first-slice rows above. This is not full
parity. Authentication blocks only real-provider E2E; independent code must continue.

| Capability | Source files/revision (pins in SOURCE_INVENTORY) | Status | Target / evidence / remaining work |
|---|---|---|---|
| Native expanded EditPlan vocabulary | Cartcut `features/agent/commands/{edit,clip,tracks,appearance,media}.ts`, OpenChatCut plan behavior | TESTED | `concat-agent/src/plan.rs`, `extended_tests.rs`; place existing media, two-way source-bounded trim, ranges/ripple, multiple remove, speed, audio, transform, absolute-time keys, text, track aliases; 14 agent tests total. Import, full style/motion/effect/transition vocabulary still missing. |
| Read/activate/propose multi-turn runtime | OpenChatCut `src/agent/{runtime,api-round,api-retry,tool-schema,tool-activation,execution-policy}` | PORTING | `concat-agent/src/runtime.rs`, `runtime_tests.rs`; seven tools, actual usage/turn/context/deadline limits, whole-round compaction, typed native refusal, cancellation and no uncertain paid retry; 3 fixture runtime tests pass. Durable ledger, leases, checkpoints, recovery, full skills/dependency closure remain missing. |
| Existing gateway tool-call transport | OpenChatCut tool behavior, existing Promax chat equivalent | TESTED | `concat-studio/src/ai.rs`; authenticated existing `/ai/chat`, exact model capability lookup, actual tool/usage receipt; `tool_turn_uses_existing_authenticated_chat_and_preserves_tool_receipt` passes. Backend normal-request-close cancellation regression passes. Real provider not invoked. |
| Native full-geometry contact sheet and approval wiring | Cartcut `commands/contactSheet.ts` | PORTING | `concat-host/src/contact_sheet.rs`, `preview.rs`, `concat-export/src/lib.rs`, GUI `ai_editing.rs`; cut/changed-region samples, max16, full frame <=8M pixels then bilinear tiles, isolated bounded reader pool, strict missing-layer refusal, cancellation, title budget. UI shows exact row-major timestamps. Tests written for timing/bounds and pixel equivalence to native preview over real encoded MP4, but latest host execution/link blocked by disk/PDB failure. Large proposals explicitly refused rather than silently partially inspected; pagination and richer overlays remain missing. |
| 30th-command failure / 50-operation atomic history | WeftCut actor/history behavior, native equivalent | TESTED | `thirtieth_failed_operation_preserves_live_project_revision_and_next_id`, `track_alias_and_fifty_operations_are_one_undo`; live project/revision/mint unchanged on failure, one undo for 50 titles. Does not establish lease/checkpoint/MCP parity. |
| Native agent-mode UI / contact-sheet UI | Existing Concat pane | PORTING | `concat/src/ai_editing.rs`, `ui/workspace/ai-editing-pane.slint`; opt-in bounded multi-turn mode and before/draft inspection; preserves human approval and live Session. Latest full check blocked by os error112; no GUI E2E claimed. |

WeftCut ownership/checkpoints/durable activity, OpenChatCut intelligence/word
editing, authenticated live-GUI MCP and Velorn generation are still NOT_STARTED
as full ports. Existing Concat primitives are not evidence that these features
are complete. No E2E_VERIFIED status is claimed.
