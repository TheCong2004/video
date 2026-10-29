# 09 — Codex implementation phases, dependencies, Done criteria and rollback

## P0 — Repo audit (no risky refactor)

**Tasks:** read actual repo; map Concat/OT, toolchain, URLs/capabilities, simulate current baseline; write ADR and integration matrix. **Output:** `REPO_AUDIT.md`, `INTEGRATION_MATRIX.md`, `ADR_0001_RUNTIME_BOUNDARY.md`, baseline tests. **DoD:** every source path in implementation references verified; unknowns labeled; branch safe. **Rollback:** docs revert only.

## P1 — Runnable deterministic LiveBrain with mocks

**Tasks:** implement contracts, local state store, FSM, event ingress simulator, normalized intents, rule-only FAQ, product fake, bounded priority queue, fake speech driver, trace logs. **Tests:** T01–T06, T14,T16,T19,T20. **DoD:** Run a scripted sequence of Vietnamese comments, emit timed actions, no external API/GPU required, keep existing Concat test green. **Rollback:** `KEDI_LIVE_ENABLED=false`, no other app behavior affected.

## P2 — Real OpenTalking adapter

**Tasks:** probe actual OT routes/events, adapter with idempotency, start/speak/interrupt/stop, session mapping, explicit playback ack or documented estimated completion, graceful timeout, mock WebRTC/output smoke and fallback source. **DoD:** T03,T04,T09,T10,T18,T19, smoke true avatar where available; no stream stoppage on LLM timeout. **Rollback:** `KEDI_OT_ADAPTER_ENABLED=false`, mock backend or existing OT direct UI intact.

## P3 — Commerce snapshot and policy

**Tasks:** per-store Fufuni gateway adapter, mock/contract tests, version/TTL, SKU mapping, shipping quote only when supported, LLM structured output validate facts, safe script template, Twenty async sink. **DoD:** T05,T06,T13,T17 and shipping tests; no made-up prices, tenant isolation. **Rollback:** `KEDI_COMMERCE_ENABLED=false`; readonly demo product sandbox available.

## P4 — Avatar quality/motion scenes

**Tasks:** Concat project/asset export, manifest validation and versioning, motion library tags, scene selection, Face–Hand cutaways, voice continuity, output fallback; compare supported OpenTalking backends (no proprietary model import). **DoD:** T11,T12 plus side-by-side quality metrics; editor render/export regression green. **Rollback:** `KEDI_MOTION_ENABLED=false`, default stationary avatar remains.

## P5 — Real platform interactions + operator console

**Tasks:** selected platform official API/approved connector first; read-only comments and capability manifest; then guarded write actions (pin product/send comment) only with authorization; UI operator takeover, manual override, SSE metrics, optional MCP control. **DoD:** T07,T08,T15 + auth/security, no unauth actions; actual pin ACK tested in permitted staging setting. **Rollback:** `KEDI_PLATFORM_WRITE_ENABLED=false`, platform reader only.

## P6 — Production hardening / deployment

**Tasks:** setup/update packaging for Windows/GPU runtime, health, resource preflight, crash recovery, staging E2E soak, metrics dashboard, runbook, privacy/rights review, source+weights license inventory, rollout/canary. **DoD:** T01–T20, SLO evidence, operator runbook and rollback rehearsed; pilot report. **Rollback:** disable new service/publisher + restore previous stable desktop build; protect session/store data.

## Implementation track to minimize regressions

- Keep work small and observable. One phase per PR/commit; P1 may split into schema + FSM, then brain/queue, then simulator/tests.
- If repo has existing JSON-RPC/MCP, extend documented interface or create adapters rather than coding a separate websocket server.
- Add feature flags and dependency injection before wiring live production. Avoid creating a new database if existing storage already provides equivalent behavior.
- P2 and P3 can be developed in parallel **only after** P1 contracts freeze; P4 motion quality must not block P3 sale safety.

## Every phase report template

```text
STATUS:
BASE SHA/BRANCH:
CHANGED PATHS:
NEW/EXISTING MODULE MAPPING:
CONTRACTS / API CHANGES:
TEST COMMANDS + RESULT:
PERFORMANCE MEASURED (when applicable):
SECURITY / LICENSE CHECK:
KNOWN BLOCKERS (missing creds/GPU/livestream account):
FEATURE FLAGS / ROLLBACK:
NEXT PHASE INPUTS:
```

Do not claim `deployed`, `fully realtime` or `tested with TikTok/Pancake` without actual logs/proof and permission.
