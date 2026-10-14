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
