# Codex execution prompt — P4

Repo Kedi hiện tại đang mở. Đọc `docs/kedi-live-codex/CODEX_START_HERE.md` (hoặc path thực tế của pack) và AGENTS.md hiện có. **THỰC THI CODE/TEST** cho P4; không chỉ viết kế hoạch.

Task: Bám `docs/05_CONCAT_AVATAR_MOTION.md`: export Concat scene/motion manifest, safe loader, motion tags, scene controller, face-hand cutaways, preserve speech continuity. Benchmark QuickTalk/MuseTalk/FasterLivePortrait only if installed/licensed; no proprietary DEEPCORE `.pyd`.

Bắt đầu bằng kiểm tra status và baseline; xác định actual paths thay vì tạo copy song song. Nếu path đề xuất không tồn tại, dùng repo audit mapping. Giữ feature flags OFF cho production cho đến khi smoke test. Không dùng production secrets nếu không được cấp. Không chạy live public/đăng pin thật ngoài môi trường được phép.

Bắt buộc cuối công việc: current SHA/branch, actual changed paths, API/contracts, tests commands+exit status, skipped reason, remaining risks, feature flag rollback, next smallest phase.
