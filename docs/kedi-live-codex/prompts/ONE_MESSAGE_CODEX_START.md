# Prompt dán vào Codex (bản ngắn)

Hãy mở repo Kedi hiện tại và đọc toàn bộ thư mục `docs/kedi-live-codex/`, bắt đầu từ `CODEX_START_HERE.md`, sau đó `docs/00_REPO_AUDIT.md` và `docs/09_PHASES_AND_ACCEPTANCE.md`.

Mục tiêu: nâng cấp Concat + OpenTalking hiện có thành Kedi AI Live Commerce với comment realtime → LiveBrain → facts từ Fufuni/Pancake → policy → bounded priority scheduler → OpenTalking avatar/voice → scene/motion/video output. Giữ editor, engine và feature cũ. Không đưa video frames qua MCP/Cloudflare. Không chép `.pyd` DEEPCORE. Không đoán giá/tồn kho/ship. `speak_audio` của OpenTalking upstream là STT input; xác minh fork. Đặt mọi bổ sung sau feature flags.

**Trong lượt này hãy triển khai P0 + P1**, không chỉ lập kế hoạch: audit chính repo, tạo matrix map đường dẫn, thêm contracts/JSON schemas, simulator, LiveSession FSM, comment dedupe/coalescing, rule-only Brain, bounded priority scheduler, fake speech backend, tests và demo script. Nếu thiếu GPU/API keys vẫn phải chạy mock E2E. Chạy test hiện có + test mới; báo commands và exit code, changed files, safety/rollback, blockers. Không thực hiện P2–P6 trước khi P1 tests đạt.
