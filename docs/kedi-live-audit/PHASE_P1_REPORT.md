# Kedi Live Commerce — P0/P1 execution report

**Status:** P0 audit complete; P1 mock implementation complete. P2+ not started.  
**Base:** `main` at `c64c1517516226a2c7a221260be972b74d0cd1aa` in the pre-existing dirty Concat worktree.  
**Rollback:** set `KEDI_LIVE_ENABLED=false` (default); the P1 module is not wired into the existing LIVE path. To remove the code, remove the `live_brain` module export in `concat-studio/src/lib.rs` and the added `live_brain.rs`, integration test, and example. No migration or persisted data was introduced.

## Implemented

- Versioned LiveEvent validation, RFC3339 timestamp validation, allowlists, Unicode-whitespace normalization, and Serde round-trip tests that reject unknown fields.
- LiveSession finite-state transitions with monotonic revisions.
- Mock ProductSession with VND integer price, tenant/store scoping, monotonic SKU revisions, validity windows, and a separate featured SKU versus confirmed platform pin state.
- Rule-only Vietnamese intent handling and a post-planning PolicyGate. Unknown/stale product facts produce neutral answers; stale revisions, expired work, and unapproved platform actions do not reach fake speech.
- Bounded priority scheduling (40 tasks / 12 seconds estimated speech), P0 preemption, event dedupe, 1-second duplicate-comment coalescing, fake idempotent speech sink, and offline simulator.
- Contract-shaped LiveTask and ProductSnapshot serialization plus simulator-only PlatformCapabilities. Four feature flags parse only explicit `1`, `true`, or `on`; defaults are off.
- Demo: `$env:KEDI_LIVE_ENABLED='true'; cargo run -p concat-studio --example kedi_live_simulator; Remove-Item Env:\KEDI_LIVE_ENABLED` (PowerShell). It queues a mock price answer, rejects prompt injection, and coalesces a duplicate. It makes no network, platform, account, audio, or OpenTalking call.

## Files added or changed for this phase

- `concat/src/crates/concat-studio/src/live_brain.rs` — P1 types, FSM, rules, mock facts, policy, scheduler, simulator, and unit tests.
- `concat/src/crates/concat-studio/src/lib.rs` — exports the new service module.
- `concat/src/crates/concat-studio/tests/live_brain_simulator.rs` — local end-to-end integration scenarios.
- `concat/src/crates/concat-studio/examples/kedi_live_simulator.rs` — opt-in demo.
- `concat/docs/kedi-live-codex/` — retained copy of the supplied 31-file implementation pack.
- `concat/docs/kedi-live-audit/REPO_AUDIT.md`, `INTEGRATION_MATRIX.md`, `ADR_0001_RUNTIME_BOUNDARY.md`, and this report — audit evidence and phase status.

Existing user changes in Concat and the sibling ArtCraft/OpenTalking repo were left intact. The existing Concat LIVE panel/media flow was not wired to the new simulator.

## Tests and evidence

- Before P1: `cargo test -p concat-studio` — exit 1, 212 passed, 2 failed, 6 ignored. Failures: two `production::tests::*` mock NEXORA/lease errors, captured before the new module.
- P1-focused: `cargo test -p concat-studio live_brain --quiet` — 15 passed, 0 failed.
- Integration: `cargo test -p concat-studio --test live_brain_simulator --quiet` — 2 passed, 0 failed.
- Full service crate: `cargo test -p concat-studio --quiet -- --test-threads=1` — exit 0; 229 passed, 0 failed, 6 ignored, plus 2 integration tests and 1 local-avatar runtime test passed. Serial mode avoids intermittent socket contention in pre-existing mock service tests; default parallel mode failed intermittently in unrelated NEXORA/browser mock tests, while the failing tests passed when isolated or serialized.
- Demo: enabled simulator command above — exit 0; produced one mock price reply, rejected the hostile event, coalesced the duplicate.
- Formatting: targeted `rustfmt --edition 2024 --check` for the three new Rust files passed. Workspace-wide `cargo fmt --all -- --check` finds formatting differences in unrelated, already-dirty files; they were not reformatted.
- App-level `cargo test -p concat --lib --quiet` could not compile because FFmpeg development dependencies are missing (`pkg-config` and Vcpkg/FFmpeg). No dependency installation or model download was attempted.

## Remaining scope and risks

- P1 is an offline simulator, not an interactive live connection or an operator UI. It does not run in the current app path until a later adapter is built.
- OpenTalking session behavior was source-audited in the sibling `artcraft/engines/opentalking` checkout but was not run. P2 must map the current `speak`/`interrupt`/events flow and real playout acknowledgment; `speak_audio` is STT-driven and is not raw audio injection.
- No real Pancake/Fufuni facts, platform comments, pins, RTMP/WHIP publishing, model weights, or GPU path were exercised. P3+ requires authorized staging and fact gateways.
- Scheduler duration is capacity accounting only, not media timing. P1 has no persistence or multi-viewer fairness/aging.

## Next

Implement P2 as an opt-in adapter to the actual OpenTalking session API, with mock lifecycle tests and playout-generation safety. Keep platform writes and commerce integrations disabled until their own acceptance tests and permissions exist.


