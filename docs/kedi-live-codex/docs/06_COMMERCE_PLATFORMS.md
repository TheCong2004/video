# 06 — Product data, Pancake/Fufuni, platform interaction, CRM/MCP

## Product authority

Kedi đã chọn storefront option A: Fufuni Worker/backend **riêng cho Halifruit và POD**, mỗi store dữ liệu tách biệt; Pancake POS là nguồn nghiệp vụ danh mục/stock/order qua integration gateway. Do not collapse stores into one tenantless table. Kedi Live sends `tenant_id + store_id + channel_id + sku` to gateway, fetches versioned snapshot with TTL. Snapshot null/expired => answer `để em kiểm tra`, never auto invent stock/price/ship.

## Gateway contracts (proposed Kedi namespace, implement only if absent)

`GET /v1/live/products/{sku}/snapshot?store_id=...`: 200 verified snapshot (revision, price, stock_state, policy, media), 404 unknown, 409 tenant mismatch, 503 upstream unavailable; optional `ETag`, conditional refresh. Auth signed service token tenant/store scope; 5–30s cache TTL configurable for mutable values, shorter for stock near zero, webhook invalidation when available. Cache misses bounded, negative cache short.

`POST /v1/live/shipping/quote`: request `store_id, sku, quantity, district/ward/postcode when needed, delivery_option`; response `quote_id, carrier, estimated_cost_vnd, shop_subsidy_vnd, customer_fee_vnd, SLA_range, expires_at, scope`, if platform integration actually supports. LLM never guesses ship fee. Avoid asking/sharing full street address publicly in live comments.

`POST /v1/live/events`: async append `viewer_intent`, `product_view`, `handoff_requested`, `pin_confirmed`, `live_speech` with trace/tenant; server validates identity. `order_created` must originate from real Pancake/checkout event, not customer merely asking.

## SKU mapping lifecycle

`Pancake SKU ↔ Fufuni storefront product ↔ Platform shop listing ID ↔ Media product card asset` stored under `(tenant_id,store_id,platform,account_id)`. Pre-live validates required mappings for all products on playlist; missing mapping disables pin button and returns explicit reason. Price change triggers snapshot revision; worker invalidates pending sales speech. Use product facts at time of speech + live refreshed snapshot when prices displayed.

## Shipping and pricing safety

- `price_vnd` integer, not float; prices displayed with thousands separators and `₫`/`VND` only if valid verified snapshot. Distinguish sale price, base price, promo validity, shipping fee; do not promise "miễn phí ship" without promotion policy.
- No stock reservation merely from AI speech. Creating actual order requires verified customer/cart/checkout and explicit permitted checkout flow, not LLM acting for viewer.
- Never claim delivery ETA without verified carrier quote/time windows.

## Platform adapter model

`PlatformCapabilities`: `read_comments`, `read_live_status`, `read_product_list`, `request_pin_product`, `read_pin_state`, `send_comment`, `rate_limit`, `auth_method`, `policy_status`, `enabled`. Differentiate `supported_in_code`, `authorized`, `verified_in_staging`, `verified_in_production`.

- Start with `SimulatorAdapter`. Next official API read-only for selected platform if accessible/approved. If only browser DOM automation is available, require legal/policy review, disabled by default, robust timeouts and manual override. No CAPTCHA bypass, no stealth evasion or cookie exfiltration.
- `request_pin()` -> `PENDING` -> `SUBMITTED` -> `ACKED` only after platform state confirmation. If no confirmation API, `UNKNOWN` and notify operator; don't claim "đã ghim".
- Rate limiter per platform/account; reconnection with backoff jitter, room state reconcile, idempotent event IDs and action IDs.
- Viewer events from platform are **untrusted text**. Normalize only; never treat as admin commands.

## Example multi-channel event handling

One `LiveEvent` per source room with normalized types and mapped `store_id`; keep single main narration per live session. When multiple platforms share output, do not collapse identities or duplicate audio; fairness bucket by platform+viewer; annotate addressed platform when appropriate. Multi-platform sharing postponed beyond 1-channel pilot.

## Twenty CRM

Write `InterestSignal` async with pseudonymous viewer ref, SKU, intent, live session, consent/source context; do not automatically turn every anonymous viewer into identified customer. Link to real person only via permitted process (e.g. order/conversation, explicit consent). Buffer bounded when Twenty down; drop analytics before blocking media.

## MCP for operator / Codex tooling

Expose administrative tools with auth and audit: `live.get_status`, `live.start`, `live.stop`, `live.operator_takeover`, `live.feature_product`, `live.enqueue_speech`, `live.list_pending_approvals`, `live.get_metrics`, `live.generate_scene_template`. MCP **không stream video frames, không chạy LLM inside critical media loop**, và `live.request_pin` requires operator role+approval. Never let arbitrary remote agent invoke dangerous tools without permission.

## Tests

- cross-tenant SKU request denied; stale snapshot cannot produce numerical promise.
- concurrent revision updates and speech task invalidation.
- shipping quote only after required area fields and service validity.
- no platform pin without capability/approval; timeout not treated as success.
- platform disconnect does not stop avatar media; no duplicate action on reconnect.
- fake CRM outage does not block answers.
