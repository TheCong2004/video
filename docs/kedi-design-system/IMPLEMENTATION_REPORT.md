# Kedi Liquid — báo cáo triển khai 22/10/2026

Trạng thái: đang chốt build EXE; chưa đánh dấu toàn bộ tiêu chí hoàn thành.

## A. Tổng quan

Giữ Rust workspace và Slint native, không tạo ứng dụng React/Next.js thay thế.
Theme là nguồn token duy nhất; Floword giữ API tương thích và đọc Theme.
Light dùng trắng/xanh pastel, navy, brand blue và navigation vàng kem;
Dark dùng shell navy. Màu media-kind, timeline và nền preview trung tính được giữ.
Studio dùng chung card, button, field, status; không bỏ bước sản xuất hay callback.

Theo yêu cầu mới nhất, luồng Hotclip chỉ có lối vào trong Edit cạnh Trợ lý AI.
Không còn mục Hotclip hoặc mục cắt video dài trùng lặp trên launcher. Component
Hotclip do phiên làm việc khác triển khai. Sau xác nhận của người dùng, đợt này
chỉ thay palette cố định bằng token Kedi và bổ sung hai nút lịch sử lời thoại;
giữ nguyên callback xử lý, layout và controller của phiên đó.
Không thêm lại một trang Tổng quan giả; màn dự án dùng dữ liệu recent hiện có.

Website tham chiếu không truy cập được qua công cụ đọc web. Hướng thiết kế dựa
trên screenshot và bảng màu người dùng cung cấp, không phải HTML/CSS đã trích xuất.
Không sao chép logo hoặc tài nguyên thương hiệu.

## B. Nhóm file thực tế

- `ui/theme/{theme,light,dark,floword}.slint`: semantic color, type, radius,
  spacing, motion; palette Studio không còn độc lập với editor.
- `ui/start.slint`, `ui/launcher-ui.slint`: adaptive rail, drawer, pin,
  topbar thật, bỏ lối vào Hotclip ngoài editor.
- `ui/workspace/studio-chrome.slint`: shared card/button/field/status và hai wash.
- `ui/workspace/studio-pane.slint`: Settings giao diện/motion/sidebar và theme
  toggle lưu qua callback, giữ các màn/chức năng hiện có.
- `ui/primitives/{modal,select}.slint`, `ui/dialogs/settings.slint`: radii,
  motion policy và cài đặt dùng chung cho editor.
- `ui/app.slint`, `src/lib.rs`, `src/prefs.rs`: export global, restore/save chung
  `settings.json`, migration serde mặc định, không tạo kho preference thứ hai.
- `src/platform.rs`: đọc accessibility animation preference Windows, không đổi
  cài đặt hệ điều hành.
- `ui/workspace/media-pane.slint`: sửa đúng một điều kiện hiển thị để media grid
  không phủ lên Hotclip tab8 của phiên đang triển khai.
- `ui/workspace/hotclip-pane.slint`: đồng bộ màu Light/Dark; thêm Hoàn tác/Làm lại
  gọi `transcript-history(false/true)`, khóa khi busy hoặc không có lịch sử.
  Không có điểm thật thì hiện chưa có đánh giá, không gán mặc định 90.
- `tools/studio-preview/{preview.slint,src/main.rs}`: render native với Hanken
  Grotesk, behavioral pointer/key sidebar, theme pixel changes và motion bindings.
- `scripts/{kedi-native-check.ps1,kedi-design-regression.test.mjs}`: kiểm tra
  responsive, hành vi và tương phản token. Regression launcher/Hotclip được cập
  nhật theo yêu cầu một lối vào editor; không dùng chúng thay thế functional QA.
- Thư mục này: 10 đặc tả portable, token JSON, HTML specimen và báo cáo này.

Không xóa model, database, project, media hoặc cache build dùng chung. Không
thay video engine, hợp đồng backend, auth, đường dẫn output hay logic timeline.

## C. Sidebar

Auto: rộng từ1100px dành252px; trung bình dành68px, hover/focus mở overlay.
Pinned: từ680px dành252px, không làm native NexPulse bị che vĩnh viễn.
Collapsed: rail68px, vẫn có nút và tooltip. Dưới680px dùng drawer/backdrop/Escape.
Rời hover chờ420ms và chỉ thu khi không còn focus; hover không tự điều hướng.
Hàng Pin luôn giữ chỗ, heading nhóm luôn16px nên icon không nhảy theo chiều dọc.
Mode0..2 được clamp và lưu; hover/drawer/focus không được lưu như preference.

Đã sửa lỗi click đầu tiên: ba FocusScope mới dùng focus-on-click:false để không
ăn sự kiện Pressed trước TouchArea. Focus ring vẫn hiển thị khi điều hướng phím.
Escape chưa khôi phục focus chính xác về nút menu; không tuyên bố focus restoration
đã hoàn chỉnh.

## D. Liquid và giảm chuyển động

Hai wash alpha thấp, đổi vị trí/bo góc bằng native animation chu kỳ20s khi được
phép. Không shader, backdrop-filter hay hiệu ứng trong video preview/timeline.
Reduced-motion mặc định bật; Windows startup đọc SPI_GETCLIENTAREAANIMATION.
System reduced-motion có ưu tiên cao hơn lựa chọn cho phép motion trong ứng dụng.
Không theo dõi thay đổi OS trong lúc app chạy. Timer của wash chưa có cơ chế
đình chỉ riêng khi minimize; không khẳng định đã benchmark CPU/GPU hoặc playback.
Launcher được hủy khi chuyển vào editor nên không chạy wash sau vùng dựng phim.

## E. Bằng chứng hình ảnh

Ảnh là render phần mềm từ component Slint thật với state kiểm thử cô lập,
không phải mockup HTML, không dùng provider/phiên LIVE thật.

- [Production Light](../../.runtime/kedi-ui-20261022/production-light-960.png).
- [Production Dark](../../.runtime/kedi-ui-20261022/production-dark-960.png).
- [LIVE hẹp](../../.runtime/kedi-ui-20261022/live-light-320.png).
- [Launcher rộng](../../.runtime/kedi-ui-20261022/kedi-sidebar-light-1280.png).

Đã xem trực tiếp các ảnh đại diện trên. Chưa có ảnh desktop before/after được
chụp tự động từ EXE cuối; screenshot cũ do người dùng cung cấp là tài liệu trước.
`design-preview.html` chỉ là token specimen, không được trình bày như ảnh app.

## F. Test evidence

| Kiểm tra | Lệnh/công cụ | Kết quả hiện tại |
|---|---|---|
| Theme/launcher/web regressions | `node --test scripts/kedi-design-regression.test.mjs scripts/studio-launcher-layout.test.mjs scripts/web-embed-regression.test.mjs` | PASS23/23 |
| Main Rust/Slint typecheck | `cargo check -p concat --offline` với native dependency env và target trên D | PASS, 10m05s; còn warning cũ |
| Native preview build | Cargo studio-preview, package debug0/codegen16 | PASS, bản cuối có receipt motion |
| Sidebar native | Pointer/key events thực, các width320..1920, Light/Dark | PASS18 lượt; click ghim/bỏ ghim thật, hover/drawer/Escape/Enter/Space/theme/retention |
| Native page renders | `scripts/kedi-native-check.ps1` | PASS80 renders ở13 route; đợt đầu78 renders nhưng thiếu motion receipt được giữ là FAIL, không che kết quả |
| Reduced motion native bindings | `settings 960 900 kedi-motion` trong preview | PASS: default0ms, opt-in160ms, OS override về0ms |
| Format toàn workspace | `cargo fmt --all -- --check` | FAIL do khác biệt có sẵn ở mã ngoài phạm vi; không format đè phiên khác |
| Whitespace diff phần được giao | `git diff --check` theo file scoped | PASS |
| Preferences serde migration test mới | `prefs::tests::kedi_preferences_migrate_and_roundtrip_without_touching_editor_choices` | Đã thêm; chưa chạy test harness riêng |
| Hotclip native contract | `scripts/hotclip-integration-regression.test.mjs` | PASS7/7 sau khi được phép bổ sung transcript-history; gồm kiểm tra palette và điều kiện undo/redo |
| Bộ regression tổng hợp sau sửa Hotclip | Bốn script Node theme/launcher/web/Hotclip | PASS30/30; structural tests không thay thế thao tác editor thật |
| OS DPI100/125/150/200% | Desktop setting thực | Chưa thực hiện; native viewport tests không được gọi là DPI tests |
| Provider, AI tạo video, TikTok LIVE thật | Backend/account/model thực | Không gọi, không tính phí; không tuyên bố end-to-end |

Các lỗi sidebar đầu tiên đã được tái hiện bằng native events rồi sửa, không chỉ
đổi regex để gọi là PASS. Các structural tests cũ được sửa đúng theo navigation mới.

## G. Giới hạn còn lại

- EXE cuối và smoke khởi động đang chờ build; chưa thay bản dùng hiện tại.
- Native preview đã build lại và chạy lại toàn script, gồm motion receipt mới.
- Chưa có benchmark GPU/CPU trước-sau, OS DPI matrix hoặc toàn bộ workflow editor
  drag/scrub/export trên project người dùng.
- Hotclip feature migration/độ chính xác ASR là công việc của phiên khác và không
  được chứng minh bởi đổi theme hoặc ảnh render giao diện.
- Một số control kỹ thuật giữ mật độ/radius riêng; không tuyên bố mọi màn hình
  đạt WCAG toàn phần chỉ từ tương phản các cặp token chính.

## H. Đánh giá hiện tại

Mã theme/sidebar/shared UI đã nâng cấp và có native behavior evidence. Các
callback/config/engine được giữ, tài liệu portable có thể áp dụng cho Next.js mà
không tạo app web giả. Build readiness đã qua typecheck; chưa gọi toàn bộ nhiệm vụ
hoàn tất khi EXE, smoke và các kiểm tra còn lại chưa được chốt.
