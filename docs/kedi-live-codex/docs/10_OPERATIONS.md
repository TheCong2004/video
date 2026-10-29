# 10 — Runbook: environment, startup, recovery, observability

## Minimum configuration (suggested; actual framework after audit)

See `templates/.env.kedi-live.example`. Config validated on startup; secret placeholders do not permit implicit cloud calls. Defaults: `KEDI_LIVE_ENABLED=false`, `KEDI_PLATFORM_WRITE_ENABLED=false`, `KEDI_LIVE_BIND=127.0.0.1`, `KEDI_OT_MODE=mock` for development, `KEDI_STORE_ID=demo` only in simulator. Profile overrides explicit.

## Preflight before session start

1. Confirm tenant/store/channel and operator identity; approved avatar identity+voice rights not expired.
2. Product playlist snapshots recent enough, price/stock/version valid, mapping to platform IDs only where needed.
3. OpenTalking engine health+capabilities, model weights available, output device/OBS/RTMP endpoint in staging, GPU headroom and clip assets validated.
4. Test emergency stop and local preview; verify selected platform data mode (simulator/read-only/authorized write).
5. Verify session already not publishing on same output key; one owner epoch.

## Start/stop flows

Start: `CREATE → PREPARING → READY → LIVE` with health/permission gate; event adapters read-only first; warm idle media output; start scripts after playout ready. Stop: P0 command, stop accepting new tasks, `interrupt`, send idle/terminate output cleanly, dispose API listener/threads, flush bounded analytics, mark STOPPED. Any step idempotent/retry-safe.

## Troubleshooting decision table

| Symptom | First checks | Immediate safe action |
|---|---|---|
| AI no voice | queue, OT health, TTS provider, command ACK vs started | switch approved cached/idle, notify operator |
| video froze | dropped frames, encode/gpu memory, stream status | show fallback clip/idle, scale down only after stable |
| wrong SKU shown | `featured_sku/revision/scene_ack` | hide numeric price, manual correct scene |
| wrong pinned SKU | `pending_action`, actual platform ACK | stop auto pin, notify operator, reconcile manually |
| duplicate speech | event_id, task_id, generation_id, reconnect retries | disable adapter, clear expired queue |
| delay spike | stage latency histogram, queued_audio_ms, TTFT | coalesce, disable P2 engagements and expensive motion |
| bad shipping/price | snapshot TTL, quote/policy version | stop numeric claims; refresh facts and correct |
| GPU OOM | model weights, VRAM, frame budget | fallback video, supervised worker restart |
| authentication alerts | failed auth, exposed host binding | rotate keys, disable remote routes |

## Metrics and alert thresholds (pilot hypotheses)

Alert on sustained dropped frame >1%, media output disconnected >10s, P95 audible reply >3s for selected questions, queued audio >12s, unhandled P1 event age >5s, GPU VRAM saturation >90% for 1 min, repeated cloud provider timeouts, platform pin action UNKNOWN >30s. Threshold tuning from empirical pilot logs, not hard-coded vendor claims.

## Data lifecycle

Event payloads are bounded+redacted, summary metrics retained according to policy, pseudonymous viewer refs no permanent ID unless consent/legitimate integration. Purge temporary TTS audio, caches, clips on lifecycle end/retention, obey asset rights revocation. Secure deletion requirements may differ by OS/filesystem; document limitations.

## Incident timeline

1. Freeze external write actions and switch seller to manual override.
2. Preserve sanitized trace/session state, no tokens.
3. Identify failure domain (`platform`, `LLM`, `TTS`, `avatar`, `output`, `commerce`).
4. Restore known-good mode via feature flag/fallback; stop session if output unsafe.
5. Postmortem: root cause, affected sessions/products, wrong claims correction, tests added.
