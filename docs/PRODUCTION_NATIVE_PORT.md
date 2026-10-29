# Chuyển luồng sản xuất ArtCraft sang Concat

Phạm vi được duyệt: chọn Page → preset/profile Nexora → ảnh/tư liệu → chạy
từng bước → video thật → nhập Concat. Không đổi sang cloud anonymous hoặc
tự đặt endpoint workflow mới.

## Nguồn đã đối chiếu

- ArtCraft `FlowordStudio/.../views/StudioView.tsx`: chọn Page, prompt mặc định,
  pageId, đầu vào ảnh và preset.
- `services/pipeline/floword_job_page_config.rs`: immutable Page snapshot.
- `grok_image_edit_stage.rs`, `grok_expand_9_16_stage.rs`,
  `grok_video_generate_stage.rs`: lease, correlation và chuyển artifact tuần tự.
- Nexora `worker/worker_routes.rs`: acquire/dispatch/heartbeat/release/cancel.

## Thứ tự triển khai và kiểm tra

1. Đưa contract ba stage Grok vào crate độc lập, giữ Page snapshot/profile/output.
2. Nối chọn Page và preset trong UI, khóa thay đổi khi chạy; không tự chọn Page.
3. Thay đường thực thi cloud bị chặn bằng worker local cho đúng workflow Grok.
4. Nhận và xác minh artifact, probe video bằng engine Concat; chỉ nhập vào đúng
   project đã được người dùng chọn, không tự tạo/ghi đè project.
5. Fixture: profile/page đổi sau preflight, hash/MIME/correlation sai, timeout,
   hủy chưa xác nhận, upstream thất bại, chống gửi trùng và import sai project.

Đây là nhánh `grok_content_pipeline` của bản cũ, không phải tuyên bố đã chuyển
tất cả engine research/TTS/publishing của ArtCraft. Không được âm thầm dùng các
ô nghiên cứu, giọng hoặc dựng phim như thể worker ba stage đã hỗ trợ chúng.

Chưa có runtime đang lắng nghe ở các cổng mặc định 10108/9223 tại lúc khảo sát.
Không gọi nhà cung cấp có phí hoặc tạo nội dung bằng tài khoản người dùng trong
fixture. Build/test và E2E thật phải được ghi riêng, không quy đổi thành nhau.

## Bằng chứng triển khai

- Backend: `concat-studio/src/production.rs`, dùng `BrowserClient` đã cấu hình;
  không mở quyền cloud ẩn danh, không gọi ArtCraft EXE.
- Controller: `concat/src/studio_production_local.rs`; thay callback chạy bị chặn,
  giữ bộ chọn ảnh/xuất cấu hình sẵn có. Chọn Page phải rõ ràng, không chọn kênh đầu
  tiên; Page có workflow khác hoặc nguồn URL chưa xử lý bị chặn, không âm thầm bỏ qua.
- UI: dropdown Page/preset/profile/output và ba trạng thái worker thật. Bảy bước
  nhập cấu hình cũ vẫn còn, không đổi chúng thành tiến độ thực thi giả.
- Đầu ra: kiểm tra SHA, probe libavformat, kích thước 9:16, thời lượng dương và
  giải mã khung hình đầu; kiểm tra lại trước mỗi lần gửi vào hàng nhập media.
- Nhập media là bất đồng bộ; thông báo chỉ xác nhận đã gửi vào hàng nhập, còn
  kết quả AddMedia do thông báo gốc của Concat xác nhận. Đường dẫn và epoch dự án
  được chụp trước khi chạy/nhập; đóng rồi mở lại cùng đường dẫn không vượt được
  kiểm tra này. Tác vụ nhập bất đồng bộ tiếp tục dùng epoch của host.
- Đã sửa lỗi qua kiểm tra chéo: hủy trong lúc acquire không được dispatch tiếp;
  xác minh release/cancel receipt; trạng thái chưa rõ được lưu để chặn gửi lại
  sau restart; hủy trong lúc probe không được tự nhập video.

### Kết quả hiện có

- 16/16 Rust production tests đạt trên loopback HTTP fixture: cả ba dispatch thật
  với payload/receipt, sai correlation, đứt kết nối, hủy khi acquire, nhật ký hỏng,
  chặn gửi lại sau restart và snapshot Page/ảnh bất biến.
- 23/23 Node regression đạt (production local, bảy bước và launcher); đây là kiểm
  tra cấu trúc, không phải E2E GUI.
- Fixture bổ sung xác minh WebP, nhật ký gọn không chứa dữ liệu ảnh/URL ký sẵn,
  khóa phục hồi theo profile xuyên Page/thư mục và release chưa xác nhận.
- Rust/Slint toàn app và render UI mới: chưa kết luận. Native check đã phát hiện
  E0308 ở mảng các setter Slint; file hiện tại đã tách ba setter thành ba lệnh
  riêng. Đã dừng lượt check đọc bản cũ; build dùng file mới còn đang chạy.
- Runtime Nexora/sidecar thật và Grok đăng nhập: chưa chạy tạo nội dung thật.

Lệnh kiểm tra có thể chạy lại (PowerShell, từ thư mục `concat`):

```powershell
$env:CARGO_BUILD_JOBS='1'
$env:CARGO_PROFILE_DEV_DEBUG='0'
$env:CARGO_PROFILE_DEV_INCREMENTAL='false'
cargo test -p concat-studio --lib --offline --manifest-path src/Cargo.toml --target-dir .runtime/nexpulse-build production::tests -- --nocapture
node --test scripts/production-local-regression.test.mjs scripts/production-steps-regression.test.mjs scripts/studio-launcher-layout.test.mjs
```

Không chạy lệnh Cargo song song với phiên đang build; dùng lại target hiện có.

### Giới hạn phải giữ nguyên khi bàn giao

Sidecar cũ chỉ fetch ảnh bằng browser context. Video URL công khai đúng host có
thể tải theo transport cũ đã siết HTTPS/allowlist/no-redirect; video chỉ xem được
khi có cookie sẽ báo lỗi, không giả thành công. Sidecar nguồn còn có nhánh có thể
không tải anchor khi thiếu file input và chọn DOM media quá sớm. Adapter chặn
source-echo/byte sai nhưng chưa chứng minh giữ đúng nhân vật qua provider E2E.

Chưa có UI đối soát/resume cho job UNKNOWN; nó bị chặn bảo thủ. Không xóa nhật ký
để lách chặn và không tự retry một yêu cầu có thể đã được nhà cung cấp nhận.
Nghiên cứu/TTS/nhiều cảnh/dựng-phụ đề/đăng bài và các engine khác chưa được chuyển
thành luồng thực thi hoàn chỉnh bởi nhánh ba stage này.

## Tự kiểm tra bàn giao nguồn

Trạng thái: bàn giao nguồn có kiểm thử, chưa phải bản EXE đã nghiệm thu.

| Tiêu chí | Điểm | Bằng chứng và phần còn thiếu |
| --- | --- | --- |
| Chính xác | 4/5 | Đối chiếu contract cũ, 16 fixture đạt; chưa có provider E2E. |
| Đầy đủ | 3/5 | Có Page/preset/profile, ba stage, xác minh và nhập; chưa nghiệm thu EXE, nguồn URL và các engine khác. |
| Rõ ràng | 4/5 | UI phân biệt cấu hình bảy bước với thực thi ba stage; thông báo lỗi vẫn cần xem thêm Nexora. |
| Dùng ngay | 3/5 | Có mã và lệnh test; cần build đạt, runtime/sidecar và hồ sơ đăng nhập thật. |
| Gọn | 4/5 | Có bảng giới hạn và lệnh tái chạy; tài liệu bảo vệ/nguồn khá dài. |

Trung bình: 3.6/5. Ưu tiên còn lại: xác nhận native build mới, sau đó chạy một
chuỗi thật với Page/profile đã cấu hình; chưa được nâng trạng thái thành hoàn tất.
Người dùng có thể đồng ý với kết quả kiểm thử nguồn, nhưng chưa nên chấp nhận là
đã chuyển đủ ArtCraft hoặc đã sẵn sàng sản xuất.

## Yêu cầu dọn build cũ

Đã bổ sung `KeepMainConfigurations` (1–2, mặc định giữ hành vi cũ là 2) trong
script dọn hiện có, để có thể giữ một cấu hình khi được yêu cầu. Script kiểm tra
process/lock trước khi xóa, không đụng model, thư viện phụ thuộc hay dự án.
Cú pháp PowerShell đã kiểm tra đạt. Chưa xóa khi compiler còn chạy.

Lệnh xóa riêng sáu EXE/PDB cũ trong target kiểm thử bị cơ chế bảo vệ chặn;
không có file nào bị xóa bởi lệnh đó, và không thử cách khác để lách chặn.
