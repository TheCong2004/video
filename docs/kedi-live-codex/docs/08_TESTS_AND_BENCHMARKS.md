# 08 — Testing, CI, GPU benchmark, quantitative gates

## Test layers

1. **Unit (mock):** schema validation, intent routing, coalescing, priority/deadline, state machine, product price revisions, policy, config, source capability.
2. **Contract:** verify JSON Schema/Pydantic/TS or Rust parity, API 2xx/4xx, explicit idempotency and version conflicts, OpenTalking mock adapter.
3. **Integration:** real local Kedi Runtime with fake platform+fake Fufuni+mock OpenTalking, deterministic clock; no network/paid models needed.
4. **Media smoke:** playback audio/video mux and scene changes, clip decoding, no black frame on missing clip, fallback; if GPU absent mark skipped with reason.
5. **Staging E2E:** true OpenTalking backend + OBS/staging stream + platform test room with permission. Do not use production shop/account for uncontrolled load.
6. **Soak:** 2h+ single live, with scheduled injected failures and full traces. Only when GPU and output path available.

## Required test matrix (MVP)

| ID | Scenario | Pass condition |
|---|---|---|
| T01 | 100 comments /10s | bounded queues, no crash, selects few useful answers, dedupe |
| T02 | duplicate platform events | exactly one speech action per idempotency window |
| T03 | P1 buyer during P3 script | interruption ack, old audio not played, resume checkpoint |
| T04 | operator stop during TTS hang | stop completes without waiting for provider timeout |
| T05 | unknown price/stock | no fabricated number and safe answer |
| T06 | price changes after queue | queued price claims invalidated or replanned |
| T07 | pin request no capability/approval | rejected, no remote action |
| T08 | pin submitted but no ACK | pinned_sku unchanged; UI pending/unknown |
| T09 | LLM 503/timeout | fallback, output stream continues |
| T10 | OpenTalking 500/restart | fallback video, recovery attempt bounded |
| T11 | bad motion asset/codec | fallback scene, no output stop |
| T12 | unsupported emotion | maps to idle/neutral |
| T13 | cross-store SKU | unauthorized/unknown without leakage |
| T14 | malicious chat prompt injection | no tool or secret disclosure |
| T15 | disconnected platform | no phantom comments or multiple publishers |
| T16 | two concurrent start commands | single publishing owner, idempotent result |
| T17 | CRM unavailable | events bounded/dropped, stream unblocked |
| T18 | speech submit HTTP 200 | not counted PLAYING until event/playout evidence |
| T19 | stale generation TTS frame | rejected by epoch guard |
| T20 | emulator zero internet access | replay demo can run offline |

## SLO pilot (targets only, not upstream measured)

- `p95(comment ingress → intent routing)` <250ms in mock/local.
- `p95(event accepted → first **audible** reply)` <3s for buyer intents in configured pilot environment; track upstream stream delay separately.
- At 720p/25–30 fps chosen profile, source output retains frame pace; dropped-frame sustained rate below agreed threshold (proposal 1% over 5-minute window), A/V skew p95 <100ms if instrumentation allows.
- 2-hour continuous pilot without live output outage or unbounded queue/memory growth; manual STOP meets operational safety objective (suggest 1s UI acknowledgment, actual shutdown tracked separately).
- 0 fabricated prices/stock/ship quotes across test fixtures; 0 unauthorized write actions.

## Required telemetry

`trace_id`, `event_received_monotonic`, `deduped_at`, `intent_ready`, `facts_ready`, `llm_started`, `llm_first_token`, `tts_first_audio`, `open_talking_submitted`, `playout_started`, `playout_finished`, `scene_committed`, `pin_ack`, `rtmp_connected`, `stream_health`, `gpu_vram_bytes`, `queued_audio_ms`, `dropped_video_frames`, `av_skew_ms`, `failed_tasks`. Never log auth tokens or full private data.

For timings, collect monotonic durations within same host; across machines use synchronized or trace-propagated causal spans, not unsynchronized wall clocks.

## Replay harness

Read `examples/events.ndjson`, deterministic virtual time and seeded rules; `simulate` endpoint DEV_ONLY; record outputs `actions.ndjson`, metrics histogram, dropped-events counters. Fixture includes Vietnamese spelling variations, accents, spam, simultaneous questions, SKU ambiguity, hostile comments, provider outages. Baselines diffable across builds.

## CI commands

No fixed mandatory toolchain until P0. Examples conditional on repo: `cargo fmt --check && cargo test`, `python -m pytest`, `ruff check`, `mypy/pyright`, `pnpm test`, `jsonschema` validation. Codex must paste **actual** invoked commands and exit codes. E2E-GPU can be manually gated; never mark skipped as passed.

## Release gate

No release unless P0–P3 mock+commerce safe, P2 real avatar successfully smoke-tested in target environment, permissions/licensing reviewed, operator takeover/STOP validated, staging E2E & security tests green. Phase P4 high-quality motion can ship under feature flags without blocking base commerce MVP.
