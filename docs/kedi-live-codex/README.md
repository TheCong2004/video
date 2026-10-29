# KEDI LIVE COMMERCE — Codex Implementation Pack (26/10/2026)

**Mục đích:** thả toàn bộ thư mục vào bản sao hoặc working tree của repo **Kedi hiện có**, dùng `CODEX_START_HERE.md` làm instruction để Codex audit, sửa, kiểm thử và tạo các PR nhỏ liên tiếp. Đây là **bản đặc tả triển khai**, không phải bản sửa mã nguồn Kedi đã hoàn tất.

## Bắt đầu

1. Tạo nhánh mới, bảo đảm Git sạch hoặc ghi lại các thay đổi đang có. Không commit token, cookie, mô hình bản quyền.
2. Copy thư mục này vào `docs/kedi-live-codex/` (giữ cấu trúc). **Không ghi đè** `AGENTS.md` của repo; so sánh `AGENTS.kedi-template.md` trước.
3. Trong Codex mở đúng repo Kedi; gửi: `Đọc docs/kedi-live-codex/CODEX_START_HERE.md, thực hiện PHASE 0 (AUDIT) trước, rồi PHASE 1 có tests; không tự ý sửa engine cũ.`
4. Nếu Codex chạy theo PR/session: dùng prompts trong `prompts/`. Bắt đầu bằng `PHASE_0_AUDIT.md`.
5. Kết thúc mỗi phase Codex phải đưa: files changed, command/test output, caveats, risk, rollback, next phase; **không tuyên bố production-ready khi chưa test runtime + GPU + nền tảng thật**.

## Điểm quyết định cố định

- Concat là editor và offline media/scene authoring; upstream Rust + Slint và JSON-RPC/MCP, không mặc định Tauri. Kedi fork có Tauri hay không **phải audit**.
- OpenTalking là engine avatar/speech/session đã có. **Không viết lại** nếu có API; phiên bản/fork/API thực tế **phải audit**.
- `POST /sessions/{id}/speak_audio` của upstream hiện nhận audio để **STT → speech pipeline**, không phải API phát raw TTS vào avatar. Muốn đưa âm thanh tạo ngoài vào thì **tìm đúng audio-ingest interface trong repo hiện tại**, chỉ làm adapter mới nếu thiếu.
- Kedi Live Runtime là **modular monolith local sidecar** trước; Cloudflare/Fufuni/Pancake/Twenty chỉ lo data/control, không mang luồng video qua Workers/MCP.
- Không tải/chạy/import binary `.pyd` DEEPCORE chưa có quyền/kiểm định. Tham khảo thiết kế queue, face-hand, sản phẩm, không copy implementation bừa.
- P0/P1 cần demo offline/simulator để có test; kết nối live thật, giao dịch/ghim/bình luận thật nằm sau capability + operator approval.

## Danh mục tài liệu

| File | Nội dung |
|---|---|
| `CODEX_START_HERE.md` | Instruction hợp nhất cho Codex, hard rules, thứ tự sửa |
| `AGENTS.kedi-template.md` | Quy tắc thường trực để hợp nhất vào AGENTS.md nếu cần |
| `docs/00_REPO_AUDIT.md` | Audit, bản đồ repo, dependency/capability, report bắt buộc |
| `docs/01_ARCHITECTURE.md` | Component boundaries, data flow, deployment, failure domains |
| `docs/02_CONTRACTS_AND_STATE.md` | Schemas, state machine, storage, API, idempotency |
| `docs/03_LIVEBRAIN_SCHEDULER.md` | Intent, FAQ/facts/LLM, priority, preemption, fairness, pseudocode |
| `docs/04_OPENTALKING_MEDIA.md` | Session API thực, adapter, tiếng/hình clock, fallback, GPU |
| `docs/05_CONCAT_AVATAR_MOTION.md` | Concat adapter, assets, clone, gesture, emotion/face-hand |
| `docs/06_COMMERCE_PLATFORMS.md` | Fufuni/Pancake, shipping, TikTok/Shopee, Twenty, MCP |
| `docs/07_SECURITY_AND_LICENSING.md` | Network/auth/privacy/clone consent, supply chain/license |
| `docs/08_TESTS_AND_BENCHMARKS.md` | Tests, SLO, fixtures, fault injection, PR gates |
| `docs/09_PHASES_AND_ACCEPTANCE.md` | Tasks tuần tự P0–P6, files changed, DoD, rollback |
| `docs/10_OPERATIONS.md` | Cấu hình, runbook, triage, shutdown/resume |
| `contracts/*.schema.json` | Hợp đồng JSON Schema v2020-12 |
| `examples/` | Event/snapshot demo, không có thông tin thật |
| `prompts/` | Prompt từng phase cho Codex |

## Nguồn đối chiếu — đọc theo ưu tiên

- Concat architecture: https://github.com/jub0t/Concat/blob/main/src/README.md
- Concat base: https://github.com/jub0t/Concat
- OpenTalking upstream: https://github.com/datascale-ai/opentalking
- OpenTalking API source: https://github.com/datascale-ai/opentalking/blob/main/apps/api/routes/sessions.py
- OpenTalking API guide: https://github.com/datascale-ai/opentalking/blob/main/docs/en/docs/api/index.md
- OmniRT: https://github.com/datascale-ai/omnirt
- Các tài liệu DEEPCORE/SLive là reverse analysis do người dùng cung cấp; không phải chứng cứ license hay hiệu năng production.

**Giới hạn quan trọng:** Chưa có URL/branch/SHA của **fork Kedi thực tế**, nên tên thư mục nội bộ ở tài liệu là *target proposal*, phải mapping từ repo audit, không tạo bản sao Concat/OpenTalking nếu bản hiện tại đã tồn tại.
