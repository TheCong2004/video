# KEDI LIVE — hướng dẫn để hợp nhất vào AGENTS.md

> **Không tự động ghi đè AGENTS.md.** Nếu repo có file, đọc và thêm phần tương thích với quy tắc sẵn có.

- Repo mục tiêu là Kedi hiện tại; Concat và OpenTalking là dependency/baseline, bảo vệ compatibility và video pipeline cũ.
- Mọi đổi mới cần: schema + tests + metrics + feature flag + rollback. Audit first.
- Architecture: `platform ingest → LiveBrain → bounded scheduler → OpenTalking adapter → A/V output` và nhánh commerce/product/analytics tách khỏi media critical path.
- Audio master clock; không block GPU loop bởi LLM/network call. Giữ stream idle khi AI hỏng.
- Every fact about price, inventory, shipping must originate from trusted, versioned store gateway, never from LLM free text.
- No arbitrary tool execution or account operations from viewer comments or LLM output. `request_pin` etc require capability/role/confirmation.
- Do not read/process secret tokens/cookies into logs or tests. Bind local APIs to loopback and require token for control writes. Require avatar/voice consent.
- Do not assume `/speak_audio` on OpenTalking plays generated audio. Verify API behavior from code.
- When editing, run existing tests first. Add tests for event idempotency, expiry, interruption, stale SKU, platform ACK, LLM outage and output continuity.
- Avoid direct dependency on proprietary DEEPCORE native modules. Respect source and model licences.
- Always give real command/test outputs; mark unknowns unverified.
