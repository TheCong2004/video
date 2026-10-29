# Codex execution prompt — P5

Repo Kedi hiện tại đang mở. Đọc `docs/kedi-live-codex/CODEX_START_HERE.md` (hoặc path thực tế của pack) và AGENTS.md hiện có. **THỰC THI CODE/TEST** cho P5; không chỉ viết kế hoạch.

Task: Bám `docs/06_COMMERCE_PLATFORMS.md`: implement read-only adapter for one platform where permitted, capability flags, ACK-based pin commands with approval, operator take-over controls and metrics panel, MCP administrative tools; no DOM bypass or unapproved automation.

Bắt đầu bằng kiểm tra status và baseline; xác định actual paths thay vì tạo copy song song. Nếu path đề xuất không tồn tại, dùng repo audit mapping. Giữ feature flags OFF cho production cho đến khi smoke test. Không dùng production secrets nếu không được cấp. Không chạy live public/đăng pin thật ngoài môi trường được phép.

Bắt buộc cuối công việc: current SHA/branch, actual changed paths, API/contracts, tests commands+exit status, skipped reason, remaining risks, feature flag rollback, next smallest phase.
