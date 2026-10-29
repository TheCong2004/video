# CODEX — LỆNH TỔNG TRIỂN KHAI KEDI LIVE COMMERCE

## Mission

Bạn là senior staff engineer chỉnh sửa **chính repo Kedi đang mở** để nâng ứng dụng dựa trên Concat + OpenTalking thành AI livestream bán hàng tương tác thực. Hãy thực sự sửa code, không chỉ viết plan. Tuy nhiên, mọi sửa đổi phải tăng dần theo acceptance gates. Mục tiêu buildable, testable, backwards-compatible; không tuyên bố đã tích hợp với TikTok/Pancake thật nếu chỉ có mock.

## Quy tắc không được vi phạm

1. **Audit trước mọi thay đổi** (`docs/00_REPO_AUDIT.md`). Đọc `AGENTS.md`, README, CI, package manifests, cấu trúc git, cấu hình app, các service đang có, phiên bản/số commit. Ghi `docs/kedi-live-audit/REPO_AUDIT.md` và `INTEGRATION_MATRIX.md` vào repo (hoặc vị trí được chính repo quy định).
2. Không xóa, viết lại, migrate framework Concat, OpenTalking, hoặc ArtCraft/Tauri khi chưa có bằng chứng buộc phải làm. Mở rộng qua adapter/plugin/public API. Không sử dụng export offline loop làm live renderer.
3. Không giả định upstream OpenTalking giống fork. Phải phát hiện session endpoints, `speak` mode, `interrupt`, events, WebRTC, capabilities, `speak_audio` semantics, source của outbound video. Có khác biệt phải cập nhật integration matrix, không hardcode.
4. Không thêm microservices/Redis/Kafka/NestJS bắt buộc cho MVP. Nếu Redis đã là phụ thuộc cốt lõi của OpenTalking thì **reuse**, không tự xoá. Sidecar có thể dùng asyncio/FastAPI/SQLite, nhưng dùng theo conventions repo sau audit.
5. **An toàn nghiệp vụ:** LLM không được tự xác nhận giá, tồn, vận chuyển, giảm giá, chốt đơn, hoàn tiền; chỉ dùng versioned authoritative product facts. Không ghi bí mật, token, cookie hay PII vào logs/prompts. Không có API thật -> provider fake/mock rõ ràng.
6. **An toàn livestream:** GPU/LLM/TTS lỗi không được dừng A/V; stop/manual override luôn được ưu tiên. Không kích hoạt platform automation thật khi chưa có xác nhận quyền API/policy.
7. Tất cả thay đổi nằm sau feature flags mặc định OFF khi chưa test thật. Không break giao diện editor, export, avatar có sẵn. Không để route API AI mặc định exposed `0.0.0.0` không auth.
8. Không tự động tải model gigabyte, không cần secrets production; dùng mock smoke tests trước. Kiểm tra giấy phép source/weights/giọng/nhân vật khi chuẩn bị thương mại.
9. **Không hỏi lại cấu trúc mà có thể tự đọc được**. Nếu gặp blocker khách quan (missing repo/service/secrets/GPU), ghi evidence và hoàn thiện mock path với bounded scope.
10. Mỗi phase chỉ merge khi tests và fallback hoạt động, sau đó ghi report thay đổi và cách rollback. Không chỉnh cùng lúc tất cả phase trên nhánh production.

## Xây theo order (bắt buộc)

- P0: mapping Kedi fork, test baseline, capability/contract inventory, đề xuất location cụ thể; không refactor lớn.
- P1: types/schema + event simulator + LiveSession FSM + bounded priority scheduler + rule-only brain + fake speech adapter + unit/integration tests.
- P2: OpenTalking adapter, capability detection, reliable speech lifecycle/interrupt, timeout/circuit breaker, safe fallback output; chạy mock trước, GPU test khi có.
- P3: ProductSession và Fufuni gateway adapter mock/real protected by flags; price/stock revision, policy gate, safe answer; 1 storefront test.
- P4: Concat asset/scene manifests, motion library, face-hand compositor với A/V sync; không yêu cầu sinh full-body bằng AI.
- P5: platform connector có read-only trước, capability action gate + ACK; admin panel và MCP tools điều khiển, không truyền media qua MCP.
- P6: benchmark GPU/long-run, recovery, rollout plan, security/license review, pilot.

Tài liệu nguồn: `docs/00_REPO_AUDIT.md` đến `docs/10_OPERATIONS.md`. `contracts/*.schema.json` là chuẩn canonical; nếu framework dùng Pydantic/TypeScript thì generate hoặc implement equivalent và validate roundtrip. Các ví dụ không phải production config.

## Luồng nghiệp vụ phải được tạo và kiểm thử

`viewer comment → event adapter → normalize/dedupe → intent → product facts → policy gate → speech/motion/scene plan → bounded priority scheduler → OpenTalking speak → playback events → compositor/output`; riêng pin/reply nền tảng phải đi qua `capability/permission/idempotency/ACK`, analytics bất đồng bộ. LLM cloud chỉ dùng khi rule/facts không đủ và có timeout/budget.

## Output format cuối mỗi phase

```
PHASE: Pn
BASELINE: current SHA / branch / test baseline
CHANGED FILES: filename + purpose
DECISIONS: ADR identifier + reason
TESTS: actual commands, exit status, brief result; unavailable tests and reason
EVIDENCE: screenshot/log/metrics where applicable (redacted)
RISKS: functional/security/license
ROLLBACK: flags/revert commands
NEXT: smallest feasible task
```

Nếu Codex chỉ có một lượt thực thi, ưu tiên hoàn thành P0 + P1 có code/tests chạy và report; **không thay bằng một bản proposal thuần túy**.

## Khi thư mục file không khớp

Tìm code có sẵn và tích hợp vào cùng ngôn ngữ/framework. Những tên như `services/kedi-live-runtime` là **gợi ý mục tiêu**, không phải yêu cầu bắt buộc. Tránh tạo module song song làm cùng việc chỉ vì tài liệu đặt tên khác. Ghi mapping `spec component → actual path → existing/extend/new → test`.
