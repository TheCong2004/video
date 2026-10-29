# 00 — Audit và ánh xạ đúng repo Kedi trước khi sửa

## Mục tiêu

Xác định thực tế: Kedi đang chạy Concat upstream hay fork; ArtCraft/Tauri có hay không; OpenTalking bản nào và engine nào đang hoạt động; API/plugin/runtime hiện có; tính tương thích; build/test commands; điểm chèn module hợp lý. **P0 không xây lại hệ thống.**

## Procedure có evidence

1. `git status --short`, `git rev-parse HEAD`, `git branch --show-current`, `git remote -v` (redact auth). Xác nhận có working changes, không overwrite. Nếu monorepo: liệt kê workspace manifests (`Cargo.toml`, `package.json`, `pyproject.toml`, `uv.lock`, `requirements*.txt`, `docker-compose*`, `wrangler*`). Đọc repo AGENTS/CONTRIBUTING/README.
2. Dò bằng `rg -n` theo tên cụ thể: `OpenTalking`, `opentalking`, `sessions`, `speak_audio`, `interrupt`, `webrtc`, `rtmp`, `mcp`, `Concat`, `concat-api`, `Tauri`, `Slint`, `Pancake`, `Fufuni`, `Twenty`. Nếu Windows thiếu rg dùng PowerShell Get-ChildItem/Select-String.
3. Kiểm tra Concat: phiên bản/commit/fork; `src/crates/` (nếu có), `concat-api`, JSON-RPC `version`, MCP và scene/project/export assets; xem editor UI nằm ở Slint hay shell khác. Tìm bridge IPC đang có, không nhân bản.
4. Kiểm tra OpenTalking: fork/commit/tag; routes thực tế `apps/api/routes/sessions.py` hoặc tương đương, API `/sessions`, `/speak`, `/interrupt`, events, `speak_audio`, publishing RTMPS/WHIP, model capability. Dùng OpenAPI schema nếu server mock/dev đang chạy; đừng assume API từ docs upstream.
5. Thử mock/sandbox only: session lifecycle, speak short text, interrupt, subtitle/event ack, A/V local preview; ghi hạn chế khi GPU/models không có. Kiểm tra background pipeline và fallback (idle frame, silent audio) bằng công cụ test, không mở live thật.
6. Kiểm tra dữ liệu: Fufuni per storefront (Worker nào), Pancake POS gateway, SKU mapping, stock/price revision, webhook, auth; không đòi nhập API key để làm P0. Tìm Twenty/MCP, quyền trên service.
7. Kiểm tra logging/tracing/metrics và data retention; network exposure, CORS, API authentication; nguồn mô hình, consent và license obligations.
8. Baseline test trên Windows/Linux phù hợp: Rust `cargo test`/`cargo fmt --check` (nếu repo có), Python `pytest`, JS `pnpm test` nếu có; **không bịa lệnh**, đọc scripts từ manifests rồi chạy.
9. Liệt kê GPU/OS/VRAM nếu môi trường có. Chỉ benchmark GPU nếu model weights và quyền tải sẵn có; không giả định máy hiện tại có NVIDIA.

## Báo cáo Codex bắt buộc

Tạo `docs/kedi-live-audit/REPO_AUDIT.md`, `INTEGRATION_MATRIX.md` và `ADR_0001_RUNTIME_BOUNDARY.md` với các bảng:

| Capability | Actual path/source | Current state | Evidence | Decision | Risk |
|---|---|---|---|---|---|
| Concat editor + scene | ... | present/absent | file/test | keep/extend | ... |
| OpenTalking speak/interrupt | ... | tested/unverified | route/method | adapter | ... |
| Publishing output | ... | ... | ... | ... | ... |
| Commerce product | ... | ... | ... | ... | ... |

Classify: `REUSE`, `EXTEND`, `NEW`, `BLOCKED`; không được dùng từ `VERIFIED` nếu chưa xem code/test. Báo mapping proposed layout → exact actual path. Ghi dependency/lockfile changes nếu sẽ thêm.

## Điều kiện P0 hoàn thành

- Phải có SHA, branch, worktree state, build/test logs.
- Có bảng path mapping và API compatibility thực tế với cả Concat/OpenTalking.
- Có ít nhất một mock end-to-end smoke path khả thi (nếu GPU thiếu, chỉ mock).
- Có quyết định rõ `sidecar Python` hoặc runtime phù hợp dựa trên repo, không xây tràn lan.
- Không sửa engine core nếu không có bằng chứng từ một test lỗi/giới hạn và ADR.
