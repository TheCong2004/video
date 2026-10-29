# Kedi Live Commerce — implementation and handoff

PHASES: P0–P6 local implementation, with real deployment gates explicitly unverified.

BASELINE: `main`, `c64c1517516226a2c7a221260be972b74d0cd1aa`. Changes are local and uncommitted; the large pre-existing user worktree was preserved.

## Current implementation

| Phase | Delivered code and behavior | Evidence / remaining external gate |
|---|---|---|
| P0/P1 | Existing Rust/Slint + ArtCraft/OpenTalking mapping, canonical events/snapshots, rules, session FSM, dedupe/coalescing and bounded priority scheduler | Existing simulator/contract coverage retained. Optional product fields from the canonical fixtures now round-trip. |
| P2 | Native owner wired to existing literal TTS and A/V player; generation cancellation after synthesis and at the real audio clock; first-audio deadline, timeout fallback and provider cooldown. Experimental REST adapter retains replace/idempotency semantics but declares that it is not a literal commerce speech ingress. | HTTP mock and local lifecycle/STOP tests. See ADR-0002. Actual configured avatar/GPU audible latency and A/V skew are not measured. |
| P3 | Exact tenant/store facts, schema/source/revision/TTL checks, safe unknown answers, queued/active claim invalidation, canonical shipping/mapping fields, authenticated bounded gateway client, separate snapshot-refresh + best-effort analytics worker | Gateway HTTP contract tests and fixture replay. Gateway `/v1/live/events` may relay to Twenty, but no real Twenty/store connection was verified. Shipping is not fabricated; unsupported or unscoped quotes remain unavailable. No orders/refunds/reservations are created. |
| P4 | Avatar consent/expiry manifest, root containment and streaming SHA-256 checks, prepared gesture library, deterministic staging/revision commit and idle fallback without resetting audio PTS. Native source selection reuses the existing lip-sync/director path and actual decoder preflight. Authoring export utility probes real clip metadata. | Consent/traversal/hash/scene-revision tests; real FFmpeg decode of a generated two-second video. Actual face/hand fusion quality, GPU and frame pacing still require target hardware. No proprietary binary or weights were imported. |
| P5 | Native Kedi panel in existing LIVE page, start/pause/resume/takeover/STOP/new session, simulation, safe manual speech and fact-bound introduction. Existing connector comments feed the new owner read-only with store-scoped pseudonyms. One automatic speech writer; buyer interruption saves a script checkpoint. Local stdio MCP preview/control tools enforce roles/CAS/idempotency. | Runtime and role/tool tests; software UI renders at 960px/light and 360px/dark. Platform pin/send transports are deliberately absent until authorized API/staging capabilities exist; submission cannot set confirmed pin state. |
| P6 | Bounded state/analytics/metrics, viewer rate limits, monotonic ingress-to-audio deadline budget, metadata-only checkpoint worker, RECOVERING restore with no automatic replay, reproducible offline verifier, asset/GPU preflight and operator runbook | Accelerated 7200-step state/queue soak, restart/STOP tests and replay. This is not a real-time two-hour GPU/media soak or a public production pilot. |

## Changed files for this continuation

- `src/crates/concat-studio/src/kedi_live.rs`: serialized owner, control roles/CAS/receipts, facts, queue/speech epochs, script checkpoints, bounded metrics/analytics, crash recovery.
- `src/crates/concat-studio/src/kedi_control.rs`: local stdio MCP/JSON-RPC control surface; no network listener or platform-write tool.
- `src/crates/concat-studio/src/live_scene.rs`: consent-bound manifests and scene revision/clock controller.
- `src/crates/concat-studio/src/live_brain.rs`, `live_adapters.rs`, `opentalking.rs`, `lib.rs`: contracts, bounds, revision/expiry guards, gateway/event adapters, truthful engine capabilities and exports.
- `src/crates/concat/src/studio_kedi_live.rs`: native panel binding, read-only ingress, one speech worker, independent commerce/checkpoint workers and asset preflight.
- `src/crates/concat/src/studio_live.rs`, `studio_lipsync.rs`, `studio_live_director.rs`, `lib.rs`, `ui/workspace/studio-live.slint`: small integration hooks into the existing app and audio clock.
- `src/crates/concat-studio/tests/kedi_live_runtime.rs`: cross-module acceptance/security/recovery/soak scenarios; existing fixture literals updated for canonical optional product fields.
- `src/crates/concat-studio/examples/kedi_live_control.rs`, `kedi_live_replay.rs`, `kedi_scene_manifest.rs`: reproducible local tools; prior simulator retained.
- `scripts/kedi-live-verify.ps1`, `scripts/kedi-live-preflight.ps1`: offline verification, bounded asset preflight and explicit build cleanup.
- `tools/studio-preview/src/main.rs`: Kedi operator UI fixture route.
- `docs/kedi-live-audit/`: ADR-0002, current integration matrix and local runbook.

## Verification

Commands and retained logs are under `.runtime/kedi-live-verification/`:

- `scripts/kedi-live-verify.ps1`: exit 0; **241 unit + 20 integration = 261 tests passed**, 7 environment-dependent tests ignored. All examples compile and the canonical offline replay passes. Logs retain the actual commands/results.
- `rustfmt --edition 2024 --check` on the changed/new Kedi modules: exit 0. Both new PowerShell scripts pass the PowerShell parser check.
- `cargo check -p concat --lib --offline`: **exit 0** on the finalized source, with existing FFmpeg/Sherpa/ORT dependencies and the locked Skia prebuilt artifact. Evidence: `.runtime/kedi-native-check-final.log` and `.runtime/kedi-live-verification/native-check.log`. Existing missing-docs/dead-code warnings remain.
- Real native software UI rendering: `live-kedi-960.png`, `live-kedi-360.png` (no account/backend calls).
- FFmpeg-generated synthetic fixture and `preflight.json`: hash/consent-reference/decode checks pass; NVIDIA utility unavailable, GPU check remains NOT VERIFIED (preflight exit 2).
- Cleanup completed after verification: removed only verified Cargo output roots `src/target` (3,260,093,031 bytes) and `.runtime/nexpulse-build` (20,381,451,767 bytes), plus the temporary Skia archive. About 22 GiB recovered; retained the small evidence folder and source/assets/dependencies. Exact entries are in `cleanup.log`.

An earlier whole-suite run in the previous continuation hit the pre-existing flaky production mock socket fixture. The current final verifier records its actual result rather than treating the earlier intermittent result as a pass.

## Release and rollback

See `KEDI_LOCAL_RUNBOOK.md`. Every new path defaults off. Set `KEDI_LIVE_ENABLED=false`, `KEDI_OT_ADAPTER_ENABLED=false`, `KEDI_COMMERCE_ENABLED=false`, `KEDI_MOTION_ENABLED=false`, `KEDI_PLATFORM_WRITE_ENABLED=false` and restart to return to existing controls. No datastore or engine was migrated. Checkpoints exclude raw comments, reply text, tokens and media.

Not performed: public streaming, real account writes/pinning, creating orders/refunds, downloading AI models, real storefront staging tests, real GPU/A/V soak, commercial source/weights/voice/identity license signoff. The machine did not expose `nvidia-smi`; approved platform/store access and target avatar/model profiles were not supplied. These are production acceptance gates, not simulated successes.
