# 03 — Kedi LiveBrain, LLM routing, Bounded Priority Scheduler

## Thành phần tối thiểu

`EventNormalizer`, `DedupeWindow`, `BurstCoalescer`, `IntentRouter`, `SessionContextResolver`, `FactResolver`, `AnswerPlanner`, `PolicyGate`, `TaskScheduler`, `SpeechCoordinator`, `ScriptController`, `Metrics`. Dùng dependency injection để thay provider bằng fake; không import thẳng `.pyd` proprietary.

## Intent taxonomy

`PRICE_QUERY`, `STOCK_QUERY`, `SHIPPING_QUERY`, `PRODUCT_DETAILS`, `COMPARE_PRODUCTS`, `ORDER_HELP`, `AFTER_SALES`, `PRODUCT_PIN_REQUEST`, `ENGAGEMENT`, `SPAM`, `UNSAFE`, `UNKNOWN`. Multi-intent comment được tách nhu cầu thành intents, có thể một câu trả lời bao phủ nhiều fact. Xác định SKU với confidence; hỏi rõ khi ambiguous giữa hai SKU, không tự chọn mẫu đắt hơn.

## Pipeline xử lý

1. Ingest event + trace, reject malformed/oversized, normalize Unicode whitespace, không làm mất dấu tiếng Việt.
2. Dedupe by platform_event_id + TTL. Rate limit per viewer/room, chống spam, prompt injection; không gửi nguyên user message làm system prompt.
3. Burst coalescing 1000–2000ms với câu giống nhau (normalization, aliases, semantic optional); chọn comment mua hàng có ưu tiên, phân bổ công bằng theo người xem; high volume không làm bùng calls.
4. Route intent qua rules/regex/keyword + small model tùy chọn; nếu confidence thấp, fast-LLM schema output. Không dùng model reasoning cỡ lớn trong hot path mặc định.
5. Resolve SKU/state, verify snapshot/TTL/tenant, avoid cross-store lookup. If price/stock unknown, only neutral response. Quote shipping only with sufficient destination and product weight and current carrier quote, không tự đoán phí.
6. `AnswerPlanner`: dùng template deterministic khi có facts (ví dụ "Giá hiện tại là {price}đ, đã xác minh lúc..."); cloud LLM diễn đạt linh hoạt theo verified structured facts và policy only. Guardrails hậu kiểm cấm số tiền/stock/ETA không nằm trong facts; nếu fail fall back template.
7. `PolicyGate`: validate `product_revision`, permissions, timeouts, motion enums, length, platform action, any claims. Emit structured LiveTask.
8. `TaskScheduler`: priority+deadline+aging/fairness; queue bounded both task count and audio_duration; older script dropped on interaction burst.
9. `SpeechCoordinator`: submit to OpenTalking, ack, scene binding via playout sync, invalidate old chunks, finish/fail, resume script checkpoint.
10. Send nonblocking event to Twenty/metrics; do not await business CRM write before speech.

## Routing decision matrix

| Intent | Facts | Model strategy | Safe answer |
|---|---|---|---|
| price/stock | trusted current snapshot | deterministic template | unknown -> `để kiểm tra lại` |
| shipping | quote by district/ward etc | deterministic | no destination/quote -> ask area |
| product comparison | multiple verified snapshots | fast LLM + whitelist | no unsupported comparison |
| complaint/refund | policy, order verified only with auth | ESCALATE | handoff staff |
| greeting/like/follow | no facts | template + cooldown | cap frequency |
| prompt injection/system access | none | filter/no answer | no tool execution |
| script CTA | approved claim list | pre-generated | scripted, no fabricated guarantees |

## Scheduler policy (defaults = suggested pilot; flags can tune)

- `P0` emergency controls; `P1` buyer question; `P2` engagement; `P3` script. P0 priority strict. `P1` fairness round-robin by viewer/intent; duplicate questions folded into one answer. No starvation of seller script: after a bounded burst or cooldown, return to a safe checkpoint.
- `max_pending_tasks=40`, `max_pending_speech_ms=12000`, `max_per_viewer_per_10s=2`, `coalesce_window_ms=1000`, `max_llm_parallel=2`, `llm_timeout_ms=1800` initial *configuration suggestions*, not DEEPCORE constants, tuned by benchmark.
- P3 prefetch only if backlog below e.g. 3000ms and P1 queue empty; never synthesize minutes of voice ahead.
- `P1` deadline measured `received_monotonic`+3000ms target first audio; may fall back to fast neutral statement if facts are delayed. Deadline expiration cancels stale tasks; don't suddenly answer a 30s old comment.
- Speech duration estimation from TTS metadata preferred; fallback `len(text)/chars_per_second` *only for capacity estimate*, not AV PTS.

### Pseudocode controller

```python
async def on_event(event: LiveEvent) -> None:
    normalized = normalizer(event)
    if dedupe.seen(normalized.event_id) or limiter.reject(normalized):
        metrics.drop(normalized, reason="duplicate_or_rate")
        return
    bucket = coalescer.add(normalized)
    if not bucket.ready():
        return
    intent = await intent_router.classify(bucket.representative)
    snapshot = await fact_resolver.resolve(intent, timeout_ms=350)
    plan = await planner.create(intent, snapshot, timeout_ms=1800)
    safe_plan = policy.validate(plan, snapshot, session_state)
    if safe_plan is None:
        metrics.drop(normalized, reason="policy")
        return
    await scheduler.offer(safe_plan)   # capacity/drop policy, NEVER wait forever
```

```python
async def speech_loop(session_id: str) -> None:
    while session.is_live:
        task = await scheduler.take_next(session_id)
        if task.is_expired() or not session.version_matches(task):
            continue
        if speech.is_playing_lower_priority(task):
            await speech.interrupt_with_ack(timeout_ms=500)
            scene.mark_cancelled_generation(speech.generation_id)
        await scene.stage(task.scene, task.product_ref)
        ack = await speech.submit(task, command_id=task.task_id)
        if not ack.accepted:
            await fallback.on_speech_failure(task)
            continue
        started = await speech.wait_started(task, timeout_ms=900)
        if not started:
            await fallback.ensure_stream_continues()
            metrics.speech_start_timeout(task)
```

**Pseudocode only**, Codex phải diễn giải theo upstream OpenTalking event API, không dùng `sleep` giả lập thời lượng nếu có playback event.

## LLM cloud adapter

- `OpenAI-compatible` base URL configurable, no hardcoded SiliconFlow/provider model; use fast models by evaluation. LLM chỉ trả typed `AnswerDraft {text,intent,used_fact_ids,requested_motion,reasoning_summary?}`; **không expose private chain-of-thought**, không lưu hidden reasoning; mô hình không xuất lệnh HTTP/tool executable.
- Prompt có persona người bán tiếng Việt, max length, approved product facts with revision, forbidden claims; viewer content explicitly `untrusted_data`. Use JSON schema response if provider supports; parse/validate; strict LLM output cap.
- `timeout`, `retry` only on safe transient before dispatch, rate limiting, circuit breaker, token quota/day/session, LLM budget alert, fallback FAQ/neutral.
- Tenant separation: key and prompt scope per tenant; no cross-tenant cached answer; never send customer phone/address except minimally required and authorized via privacy policy.

## Test required

`test_burst_100_comments_coalesces`, `test_duplicate_event_not_replayed`, `test_p0_takes_priority`, `test_p1_preempts_p3`, `test_old_generation_frames_dropped`, `test_stale_sku_cancels_price`, `test_llm_timeout_fallback`, `test_comment_prompt_injection_denied`, `test_platform_action_no_approval`, `test_fairness_under_comment_storm`, `test_queues_bounded`, `test_operator_override_under_load`.
