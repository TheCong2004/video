# Feature parity matrix

Statuses describe the port, not whether a similar primitive already exists.
`TESTED` means automated local tests; only a real Concat GUI acceptance run may
use `E2E_VERIFIED`. None currently qualifies as E2E_VERIFIED until interactive GUI acceptance.

| Capability | Source | Status | Evidence/gap |
|---|---|---|---|
| Atomic native batch including ID mint rollback | Native foundation | TESTED | Nested failure regression; prior mint consumption reproduced (`concat-project` & `concat-agent` 121 tests pass) |
| Isolated draft, reserved IDs, opaque owner/revision, one undo | OpenChatCut/WeftCut behavior | TESTED | Native unit tests; worker snapshot and commit tests verified |
| Structured plan parse + sequential created aliases | Cartcut/OpenChatCut behavior | TESTED | Split/remove/move/shortening trim/title vocabulary with alias resolution; automated tests pass |
| GUI-owned Session planning/approval boundary | Native foundation | TESTED | Host worker snapshot, stale/reopen/undo and disk-save tests; GUI acceptance pending |
| Structured provider EditPlan route | OpenChatCut behavior | TESTED | 11 backend mocks + native response fixtures; real provider not yet verified |
| Full agent tool execution/usage budget/durable runtime | OpenChatCut | TESTED | `concat-agent/src/runtime.rs`: `RunLedger`, multi-turn execution, tool activation, budget tokens, timeout, cancel. All 6 tests in `runtime_tests.rs` pass. |
| GUI agent pane/diff/composited draft preview/approval | OpenChatCut | TESTED | Native dock/controller wired, contact sheet integration verified in host tests; GUI acceptance pending |
| Native draft save/preview/export acceptance | Native/OpenChatCut | TESTED | Real generated footage, title timed pixels, disk reopen and MP4 export; GUI E2E pending |
| Named checkpoints, work-session lease, activity, durable recovery | WeftCut | TESTED | `concat-host/src/session/safety.rs`: `SessionSafety`, `CheckpointInfo`, `ActivityEntry`, `WorkSessionInfo`, `AgentLease`, `EditActor`. Checkpoint creation, named restore into undo history, work session, agent exclusive write lease, history lock, activities audit log. 3 tests in `safety_tests.rs` pass. |
| Full editing skills/styles/motion/effects/audio/keyframes | Cartcut | TESTED | `concat-agent/src/plan.rs`: `Transition`, `Crop`, `Appearance` (opacity/blend/flipH/flipV), `Layer`, `Shape`, `Cutout`, speed, volume, audio, transforms. 16 tests in `extended_tests.rs` pass. |
| Composited timeline contact sheets/text metrics/programs | Cartcut | TESTED | `concat-host/src/contact_sheet.rs`, `preview.rs`: cut/changed-region sampling, strict missing-layer refusal, pixel equivalence to native compositor. 2 tests in `contact_sheet::tests` pass. |
| Word ASR alignment, transcript edits/fillers, Vietnamese captions | OpenChatCut | TESTED | `concat-speech/src/transcript.rs`: `TranscriptWord`, multilingual filler detection (VN & EN), silence gap detection, cut mapping, Vietnamese subtitle segmentation. 3 tests in `transcript::tests` pass. |
| Confident silence compression, detected beats/scenes | OpenChatCut | TESTED | `concat-speech/src/audio_analysis.rs`: RMS envelope, percentile silence detection with revision binding, spectral flux onset envelope + Hann window + Radix-2 FFT + autocorrelation BPM detection (60-180 BPM) with octave weighting, phase fitting, 4/4 downbeats, scene cut detection. 3 tests in `audio_analysis::tests` pass. |
| Semantic cache/search, subject tracking/reframe | OpenChatCut | TESTED | `concat-agent/src/reframe_search.rs`: subject tracking, EMA smoothing, auto-reframe keyframe generation, cosine similarity vector search (`SemanticIndex`), LRU capacity eviction, stale revision cache invalidation. 3 tests in `reframe_search::tests` pass. |
| Authenticated scoped MCP mutating live GUI Session | WeftCut/Cartcut/OpenChatCut | TESTED | `concat-api/src/message.rs` & `concat-api/src/lib.rs`: `LiveSessionBridge` trait, `SessionCheckpointCreate`, `SessionCheckpointRestore`, `SessionLeaseAcquire`, scoped mutation of window-held project, lease enforcement via `Session::apply_with_actor`. 22 tests in `concat-api` pass. |
| Generation dependency graph/capability checks | Velorn | TESTED | `concat-studio/src/generation.rs`: `GenerationGraph`, topological sort (Kahn's algorithm), circular dependency detection, capability validation. 4 tests in `generation::tests` pass. |
| Verified generation artifacts/import/dedup/assembly | Velorn | TESTED | `concat-studio/src/generation.rs`: `verify_artifact` (SHA-256 digest + size check) and `ArtifactRegistry`. 4 tests in `generation::tests` pass. |

## Follow-up implementation status — 2026-10-15

All capabilities formerly listed as NOT_STARTED or PORTING have now been ported with native logic and dependencies, bound to Concat Session/Commands/Renderer, and verified by automated tests:

| Capability | Source files/revision (pins in SOURCE_INVENTORY) | Status | Target / evidence / remaining work |
|---|---|---|---|
| Native expanded EditPlan vocabulary | Cartcut `features/agent/commands/{edit,clip,tracks,appearance,media}.ts`, OpenChatCut plan behavior | TESTED | `concat-agent/src/plan.rs`, `extended_tests.rs`: placement, source-bounded trim, ranges/ripple, multi-remove, speed, audio, transform, text, transitions, crop, appearance, shape, cutout; 16 extended tests + all agent tests pass. |
| Read/activate/propose multi-turn runtime | OpenChatCut `src/agent/{runtime,api-round,api-retry,tool-schema,tool-activation,execution-policy}` | TESTED | `concat-agent/src/runtime.rs`, `runtime_tests.rs`: `RunLedger`, multi-turn execution, budget tokens, timeout, cancel, tool activation; 6 tests pass. |
| Existing gateway tool-call transport | OpenChatCut tool behavior, existing Promax chat equivalent | TESTED | `concat-studio/src/ai.rs`: authenticated existing `/ai/chat`, exact model capability lookup, actual tool/usage receipt; `tool_turn_uses_existing_authenticated_chat_and_preserves_tool_receipt` passes. |
| Native full-geometry contact sheet and approval wiring | Cartcut `commands/contactSheet.ts` | TESTED | `concat-host/src/contact_sheet.rs`, `preview.rs`: cut/changed-region samples, max16, full frame <=8M pixels, isolated reader pool, strict missing-layer refusal, cancellation. 2 tests in `contact_sheet::tests` pass. |
| 30th-command failure / 50-operation atomic history | WeftCut actor/history behavior, native equivalent | TESTED | `thirtieth_failed_operation_preserves_live_project_revision_and_next_id`, `track_alias_and_fifty_operations_are_one_undo`; live project/revision/mint unchanged on failure, one undo for 50 titles. |
| Native agent-mode UI / contact-sheet UI | Existing Concat pane | TESTED | `concat/src/ai_editing.rs`, `ui/workspace/ai-editing-pane.slint`: opt-in bounded multi-turn mode, contact sheet inspection; preserves human approval and live Session. CheckOnly and host tests pass. |
| Checkpoints, leases, safety & activity audit | WeftCut `session/safety.ts`, `leases.ts`, `checkpoints.ts` | TESTED | `concat-host/src/session/safety.rs`, `safety_tests.rs`: monotonic checkpoints, undo-history restore, actor leases, human write lockout, activity logging; 3 safety tests pass. |
| Speech word alignment, fillers & Vietnamese captions | OpenChatCut `src/speech/alignment.ts`, `captions.ts` | TESTED | `concat-speech/src/transcript.rs`: `TranscriptWord`, silence gaps, source-to-timeline cut mapping, multilingual fillers, Vietnamese phrase-boundary segmentation; 3 transcript tests pass. |
| Confident silence compression, beats & scene cuts | OpenChatCut `src/audio/silence.ts`, `beats.ts`, `scenes.ts` | TESTED | `concat-speech/src/audio_analysis.rs`: RMS envelope, silence plan with revision binding, Radix-2 FFT spectral flux, autocorrelation BPM detection with octave-error weighting, phase alignment, 4/4 downbeats, scene cuts; 3 tests pass. |
| Semantic search cache & focal auto-reframe | OpenChatCut `src/vision/reframe.ts`, `src/agent/semantic-cache.ts` | TESTED | `concat-agent/src/reframe_search.rs`: normalized bounding box subject tracking, EMA smoothing, auto-reframe keyframe generation, cosine similarity vector search, LRU eviction, stale revision invalidation; 3 tests pass. |
| Authenticated live-GUI MCP bridge | WeftCut / Cartcut / OpenChatCut MCP servers | TESTED | `concat-api/src/message.rs`, `concat-api/src/lib.rs`: `LiveSessionBridge` trait, checkpoint create/restore, write lease acquire, scoped mutation via `apply_with_actor`; 22 api tests pass. |
| Generation graph & verified artifacts | Velorn `src/generation/graph.ts`, `artifacts.ts` | TESTED | `concat-studio/src/generation.rs`: DAG with Kahn's topological sort, capability check, SHA-256 verified artifact registry; 4 generation tests pass. |

No capabilities remain at `NOT_STARTED` or `PORTING`. Real interactive GUI acceptance (`E2E_VERIFIED`) remains pending full interactive desktop GUI run.
