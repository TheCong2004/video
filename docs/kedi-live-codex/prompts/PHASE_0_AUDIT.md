# Codex execution prompt — P0

Repo Kedi hiện tại đang mở. Đọc `docs/kedi-live-codex/CODEX_START_HERE.md` (hoặc path thực tế của pack) và AGENTS.md hiện có. **THỰC THI CODE/TEST** cho P0; không chỉ viết kế hoạch.

Task: Đọc `docs/00_REPO_AUDIT.md`, chạy các lệnh audit và baseline, tạo `REPO_AUDIT.md`, `INTEGRATION_MATRIX.md`, ADR path mapping. Chưa thêm framework hoặc refactor engine. Hoàn tất với bản đồ chính xác Concat, OpenTalking, Kedi desktop và integration hiện tại.

Bắt đầu bằng kiểm tra status và baseline; xác định actual paths thay vì tạo copy song song. Nếu path đề xuất không tồn tại, dùng repo audit mapping. Giữ feature flags OFF cho production cho đến khi smoke test. Không dùng production secrets nếu không được cấp. Không chạy live public/đăng pin thật ngoài môi trường được phép.

Bắt buộc cuối công việc: current SHA/branch, actual changed paths, API/contracts, tests commands+exit status, skipped reason, remaining risks, feature flag rollback, next smallest phase.
