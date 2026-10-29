# Kedi Live Commerce — integration matrix

The table below preserves the earlier audit/adapter-only checkpoint. Current native integration supersedes the old "not wired" entries; see the current mapping below and `P2-P6_IMPLEMENTATION_STATUS.md` for release gates.

| Spec component | Actual path/source | State and evidence | Decision | Test / risk |
|---|---|---|---|---|
| Concat editor and media authoring | `src/crates/concat`, `src/crates/concat-studio`, `src/crates/concat-host`, `src/crates/concat-media` | Present; native Rust + Slint workspace and existing project/export/media code | REUSE | Existing changes already in user worktree; preserve editor/export behavior |
| LIVE connector protocol | `concat-studio/src/live.rs`, `live_runtime.rs`; UI in `concat/src/studio_live.rs` | Present; JSONL validation and owned TikTok connector process. Existing behavior is separate from P1 simulator | REUSE | Do not start/publish/comment against real accounts for this phase |
| OpenTalking client / voice | `concat-studio/src/opentalking.rs`, `concat/src/studio_opentalking.rs`, `studio_live_director.rs`, `studio_lipsync.rs` | Existing configured local client and speech/lipsync flow. No session API lifecycle wiring found | REUSE + P2 adapter later | Existing tests cover local client behavior, but no running OT session or playback ACK verified here |
| OpenTalking session REST | `artcraft/engines/opentalking/apps/api/routes/sessions.py`, `services/session_service.py` | Source confirms replace-only `speak`, `command_id`/idempotency key, interrupt endpoint, events route, STT semantics for `speak_audio` | EXTEND through P2 adapter | No runtime, GPU, stream, or ACK test in P0 |
| LiveEvent validation and dedupe | New `concat-studio/src/live_brain.rs` | Version 1 event, bounded text, RFC3339, platform/event allowlists, stable ID dedupe + 1s normalized text coalescing; Serde round-trip rejects extra fields | NEW | Unit and integration tests; schema fields remain a hand-implemented Serde equivalent |
| LiveSession FSM | New `concat-studio/src/live_brain.rs` | Legal lifecycle transitions and monotonic revision | NEW | Unit test; in-memory only, no persistence/concurrency store |
| LiveBrain rules | New `concat-studio/src/live_brain.rs` | Vietnamese keyword intent, deterministic post-planning PolicyGate, safe neutral response for unknown facts, rejects prompt injection, no LLM/tools | NEW | Unit and simulator integration; intentionally limited vocabulary |
| ProductSession mock | New `concat-studio/src/live_brain.rs` | Tenant/store scoped snapshot lookup, revision monotonicity, expiry check, featured SKU distinct from confirmed pin; fixtures source `mock` | NEW | Unit/integration tests; not real commerce integration |
| Bounded priority scheduler | New `concat-studio/src/live_brain.rs` | Max task count and estimated queued speech duration; P0 can evict lower priority; FIFO within equal priority | NEW | Unit test; no persisted queue, expiry clock, aging, or per-viewer fairness yet |
| Fake speech adapter | New `concat-studio/src/live_brain.rs` | In-memory idempotent task ID sink; no network/media side effects | NEW | Unit/integration tests; real audio not generated |
| Comment simulator/demo | `concat-studio/src/live_brain.rs`, `tests/live_brain_simulator.rs`, `examples/kedi_live_simulator.rs` | Local event-to-facts-to-queue-to-fake-speech route | NEW | Offline; demo requires explicit `KEDI_LIVE_ENABLED=true` |
| Pancake/Fufuni product gateway | No current app path found | Absent from active Rust sources | BLOCKED to P3 | Requires approved store gateway contract/credentials and staging tests; no secrets requested |
| Pin/send-comment actions | Existing TikTok connector has a separate send-message command; no Kedi authorization/approval/ACK policy | P1 implements no platform action | BLOCKED to P5 | Keep platform-write flag OFF; never treat submit as ACK |
| Kedi settings/control UI | Existing LIVE panel in `concat/ui/workspace/studio-live.slint` | Existing UI is reused unchanged; new module is not yet wired into it | EXTEND later | Feature is available to code/tests, not an operator-facing panel yet |
| Real output, OBS, RTMP/WHIP | Existing app/engine paths exist but were not verified against a live target | Unverified in this audit | BLOCKED to P2/P5 | No public stream or account operation performed |
| OpenTalking live REST adapter | `concat-studio/src/live_adapters.rs` + audited `artcraft/engines/opentalking/apps/api/routes/sessions.py` | Opt-in loopback-only adapter for `/sessions/{id}/speak` (`mode=replace`, same `command_id` and `Idempotency-Key`) and `/interrupt`; queued response maps only to `SUBMITTED` | NEW / P2 | Local mock HTTP test; no real session or playback event consumer wired |
| Speech lifecycle | `concat-studio/src/live_adapters.rs::SpeechLifecycle` | Explicit `queued → started → finished`, stale generation rejection; completion only verified after `finished` event | NEW / P2 | Unit tests; production event bridge and playout epoch drop remain to wire |
| Product catalog / shipping quote | `concat-studio/src/live_adapters.rs::CommerceGatewayClient`, `ProductCatalog`, `ShippingQuote`; existing `live_brain.rs::PolicyGate` | HTTPS off loopback, explicit bearer token, bounded timeout/body, tenant+store+SKU response match, monotonic revisions, TTL; quote expiry and destination scope | NEW / P3 | Local mock HTTP + unit tests; no staging credentials, webhook refresh or Twenty analytics sink |
| Avatar scene/motion manifest | `concat-studio/src/live_adapters.rs::SceneManifest` | Whitelisted gestures/emotions, allowed-root containment, file presence and SHA-256 | NEW / P4 | Fixture tests only; decoder, codec, compositor and GPU validation remain |
| Platform pin action approval | `concat-studio/src/live_adapters.rs::PinApproval` | Requires flag + authorization + capability + human approver; no ACK without matching confirmed SKU | NEW / P5 | State tests only; deliberately no platform write client or official read adapter |
| TikTok read-only comment mapping | `concat-studio/src/live_adapters.rs::TikTokReadOnlyAdapter` + existing `live_runtime.rs`/`live.rs` | Converts validated chat event frames, binds configured tenant/store, hashes account ID into pseudonymous viewer ref, denies all writes | NEW / P5 | Mapper/capability tests; not yet wired into comment worker or operator UI |
| Local live metrics/preflight | `concat-studio/src/live_adapters.rs::LiveMetrics`, `Preflight` | Bounded latency sample/counters and preview readiness predicate | NEW / P6 | Unit tests; not wired to UI/deployment checks |

## Baseline evidence

- Concat base: `c64c1517516226a2c7a221260be972b74d0cd1aa`, branch `main`, remote `TheCong2004/video`.
- OpenTalking source checkout base: `204cef9f24ac454b511ccde8a6a4bd4e53650bec`, branch `main`; source file routes noted above.
- `cargo test -p concat-studio` baseline before code: 212 passed, 2 pre-existing failures, 6 ignored.
- The baseline test build used `concat/src/target` (~16.7 GB before its earlier cleanup). Verified Cargo build artifacts are removed after the final checks.

## Current native mapping

| Boundary | Current implementation |
|---|---|
| Validated platform chat to owner | `concat/src/studio_live.rs::monitor` → `studio_kedi_live::receive_frame` → `TikTokReadOnlyAdapter` → `kedi_live::KediLiveRuntime::ingest` |
| Operator controls | Existing `studio-live.slint` Kedi card → `studio_kedi_live::bind` → role/CAS/idempotent commands; comment text cannot invoke commands |
| Rules/facts to scheduler | Canonical `live_brain` contracts/rules/policy + serialized `kedi_live` owner with rate/deadline/generation guards |
| Literal speech to A/V | `studio_live::synthesize_and_play` → existing director TTS/lipsync/native player; audio clock evidence and cancellation before/during output |
| Prepared motion | Consent/hash-bound `live_scene::SceneController` → existing director source selection; native decoder preflight |
| Scoped commerce | Authenticated gateway in independent refresh/analytics worker; no controller/media lock held during HTTP |
| Crash recovery | Independent checkpoint worker → scoped app-data metadata file → RECOVERING on restart; no automatic replay |
| Agent tooling | Standalone local stdio MCP preview process; no HTTP listener, platform write tool or attachment to a running desktop process |
| External gates | Authorized storefront/platform staging, actual GPU/avatar A/V output and real-time soak, source/weights/voice/identity distribution signoff |

