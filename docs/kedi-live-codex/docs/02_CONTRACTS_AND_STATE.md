# 02 — Hợp đồng dữ liệu, state machines, API và storage

`contracts/*.schema.json` trong pack là nguồn tham chiếu canonical; implement Pydantic/TypeScript/Rust types theo repo hiện có và test serialization round-trip, reject extra fields ở boundary tin cậy thấp. Mọi event mang `schema_version`, `trace_id`, `session_id`, `tenant_id`, `store_id`; không tin bất kỳ field client nào cho privilege/billing.

## Object quan trọng

**LiveEvent:** `event_id`, `source`, `type`, `received_at` (UTC ISO8601), `viewer_ref` (pseudonym), `text`, `metadata`, `source_event_id`. `event_id` là idempotency key; nếu nền tảng không cung cấp ID, dùng `sha256(platform|room|viewer_ref|normalized_text|time_window)` với TTL ngắn, ghi chú false positives. `received_monotonic_ns` chỉ được tạo local trong runtime, không nhận từ mạng.

**ProductSnapshot:** `tenant_id/store_id/sku/revision/observed_at/valid_until/currency/title`, `price` integer VND nullable, `stock_state` enum, optional `shipping_quote` chứa `quote_id`, `service`, `total_vnd`, `expires_at`, `destination_scope`; `verification` source (`pancake_gateway`/`fufuni_cache`) và signature/etag nếu có. Version monotonic per SKU; không chấp nhận snapshot có revision thấp hơn đã biết; nếu stale -> không được nêu giá/còn hàng.

**LiveTask:** `task_id`, `session_id`, `origin_event_id`, `priority`, `deadline_at`, `generation_id`, `expected_session_revision`, `product_ref`, `speech`, `motion`, `scene`, `platform_actions` (proposal only) và `trace_id`. `speech.text` giới hạn 400–600 ký tự theo ngưỡng config, đồng thời đo duration từ TTS. Motion chỉ enum whitelist. Unsupported motion -> `attentive/idle`. Platform actions luôn qua policy/approval.

**PlatformAction:** `action_id`, `capability`, `actor_kind`, `approval_id`, `requested_sku`, `expected_revision`, `idempotency_key`, `status` (`PENDING`, `SUBMITTED`, `ACKED`, `REJECTED`, `UNKNOWN`, `FAILED`). HTTP 200 gửi lệnh không đồng nghĩa ACK của nền tảng; cần read-after-write/confirmed event khi có capability.

## Live session FSM

```text
CREATED -> PREPARING -> READY -> LIVE -> PAUSED -> LIVE
                             LIVE -> RECOVERING -> LIVE | FAILED
CREATED/PREPARING/READY/LIVE/PAUSED/RECOVERING -> STOPPING -> STOPPED
FAILED -> STOPPING -> STOPPED
STOPPED terminal; muốn chạy lại tạo new session_id.
```

- `PREPARING`: resolve avatar/assets/voice/stream endpoint, pin model weights, verify store tenant and output device.
- `READY`: feature flag enable, health checks OK, operator consent/publish authorization.
- `LIVE`: 1 live publishing owner, one speech channel; script controller active.
- `PAUSED`: idle video/output can remain depending platform; no new auto speech/action.
- `RECOVERING`: restore healthy output, reconcile remote/platform; do not replay expired speech.
- `STOPPING`: reject new interactions, interrupt current speech, flush telemetry bounded, stop output and dispose resources idempotently.

**Transition concurrency:** version compare-and-swap, one transaction for mutable state, reject invalid transition (409). Every action emits StateTransition event with reason/actor/correlation. Manual STOP bypasses queued LLM.

## Product Session FSM

Keep separate:
- `active_sku`: product object in current business context.
- `featured_sku`: product displayed/being explained in scene.
- `confirmed_pinned_sku`: platform-confirmed pinned item (nullable).
- `pending_pin_action`: in-flight request and deadline.
- `product_revision` per product.

`set_featured_sku` changes scene only; `request_pin` submits action but `confirmed_pinned_sku` changes **only on ACK**. If SKU revision changes between planning and playout, invalidate price/stock claims and replan; avoid changing reference after speech starts silently. When uncertain, interrupt speech and speak neutral correction or operator handoff.

## Speech FSM

`PLANNED -> READY -> SUBMITTED -> STARTED -> FINISHED`; alternate `CANCELLED`, `EXPIRED`, `FAILED`, `INTERRUPTED`. `SUBMITTED` (HTTP accepted/queued) is not `STARTED`; `FINISHED` requires media/speech event or validated playback completion. If upstream lacks lifecycle event, implement bounded approximation and mark `completion_source=estimated`, not `verified`.

- `generation_id` increments on new primary speech generation. All late TTS/avatar chunks with older `generation_id` drop before playout.
- `priority` and `expires_at` evaluated at enqueue, dispatch, **and** first audio play.
- Interrupt `P3` with `P1` after ack/fade; do not reuse stale script position. Resume from named checkpoint, not mid-word text.

## Storage minimum

SQLite WAL for session metadata/config/idempotency/event trace (if chosen by P0), **do not persist frame/audio buffers**. Suggested logical tables: `sessions`, `product_snapshots`, `platform_action_log`, `event_dedupe`, `speech_tasks`, `audit_events`, `asset_manifests`. Create migrations, constrained indexes by tenant+store+session, bounded cleanup policy. Do not persist raw viewer text indefinitely; TTL and data minimization.

## New local API (Kedi namespace only — NOT OpenTalking built-ins)

| Method | Path | Required role | Behavior |
|---|---|---|---|
| GET | `/kedi/healthz` | local | liveness, no secret config |
| GET | `/kedi/capabilities` | viewer/admin | runtime+engine features, unavailable reasons |
| POST | `/kedi/live/sessions` | operator | create session with store/avatar/stream profile; request idempotency |
| POST | `/kedi/live/sessions/{id}/start` | operator | guarded transition to LIVE |
| POST | `/kedi/live/sessions/{id}/stop` | operator | immediate idempotent stop |
| POST | `/kedi/live/sessions/{id}/operator/takeover` | operator | cancel speech, disable auto actions |
| POST | `/kedi/live/sessions/{id}/events/simulate` | dev only | inject fake LiveEvent; blocked prod |
| POST | `/kedi/live/sessions/{id}/products/feature` | operator | set featured product by SKU+expected revision |
| POST | `/kedi/live/sessions/{id}/operator/speak` | operator | high priority text with explicit product context |
| GET | `/kedi/live/sessions/{id}/state` | read | state+queue+product refs, redact viewer data |
| GET | `/kedi/live/sessions/{id}/events` | read | SSE status/metrics; auth and backpressure |

Return meaningful 400/401/403/404/409/422/429/503; error envelope `{code,message,trace_id,retryable,details?}`; never leak tokens or stack traces.

## Interfaces (language independent)

```python
class CommentSource(Protocol):
    async def start(self, session: SessionContext, sink: Callable[[LiveEvent], Awaitable[None]]) -> None: ...
    async def stop(self) -> None: ...

class ProductFactProvider(Protocol):
    async def get_snapshot(self, tenant_id: str, store_id: str, sku: str) -> ProductSnapshot: ...
    async def quote_shipping(self, request: ShippingQuoteRequest) -> ShippingQuote | None: ...

class SpeechBackend(Protocol):
    async def create_session(self, cfg: AvatarSessionConfig) -> str: ...
    async def speak(self, ot_session: str, text: str, command_id: str) -> SpeechSubmission: ...
    async def interrupt(self, ot_session: str) -> None: ...
    async def events(self, ot_session: str) -> AsyncIterator[SpeechEvent]: ...

class SceneRenderer(Protocol):
    async def set_scene(self, revision: int, manifest_id: str, product_ref: ProductRef | None) -> None: ...
    async def set_motion(self, gesture: str, start_pts_ns: int) -> None: ...

class PlatformActionExecutor(Protocol):
    async def request(self, action: AuthorizedAction) -> PlatformActionReceipt: ...
    async def reconcile(self, action_id: str) -> PlatformActionStatus: ...
```

Nếu framework thực tế khác, giữ hành vi và semantics, không ép Python protocol sang Rust một cách máy móc.
