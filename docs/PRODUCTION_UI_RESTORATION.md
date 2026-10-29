# Xưởng phim AI: phục hồi quy trình

Contract: retain seven directly selectable steps: source/idea, script, assets,
character/anchor, visuals, voice/captions, editing/export. Basic production
controls must not be hidden in advanced JSON. Use existing native Slint controls,
wrap step navigation at 360×900 and 1080×720, scroll vertically, keep input values
when switching steps. Show configuration progress separately from real job state.

Anchor selection reads a bounded PNG/JPEG locally with a preview; it is not a
video source or a cloud artifact upload. Export a versioned JSON configuration
through an explicit file dialog. No network requests or generation on navigation.
Visible settings are included in the immutable preflight/start plan and cannot
be overridden by advanced JSON. Every edit invalidates preflight.

The current local-first `studio_session::client()` returns Configuration. UI
restoration does not authorize cloud execution or install a production worker.
Keep preflight, explicit start confirmation and unknown-outcome protection. Do not
claim seven executed stages merely because seven configuration steps exist.

States: empty/unconfigured -> edit settings or export configuration; loading ->
disable mutation controls; error -> readable sanitized feedback and retry;
partial -> local anchor/settings available while remote execution unavailable;
success -> successful export or actual run summary; permission -> no anonymous
cloud access, preserve configuration without creating a fake session.

## Kiểm tra bản sửa

1. Vào **Xưởng phim AI**. Bảy nút bước luôn có trên đầu trang, có thể chọn
   trực tiếp hoặc dùng Bước trước / Bước tiếp.
2. Nhập ý tưởng ở bước 1, đổi sang bước 2 rồi quay lại: nội dung vẫn giữ trong
   phiên ứng dụng. Bước 2 nhập kịch bản; bước 3 nhập URL tư liệu và phong cách.
3. Bước 4 chọn ảnh PNG/JPEG, xem ảnh thu nhỏ và tên file. Bỏ ảnh chỉ xóa tham
   chiếu trong cấu hình, không xóa file gốc. Ảnh này chưa được tải lên máy chủ.
4. Bước 5 thiết lập công cụ và các yêu cầu hình/chuyển động. Bước 6 chọn kiểu
   đọc, ID giọng, phụ đề. Bước 7 chọn tỷ lệ, chuyển cảnh, tốc độ, âm lượng nhạc.
5. **Lưu cấu hình ra file** xuất JSON (có đường dẫn ảnh cục bộ nếu đã chọn).
   Không gửi JSON này cho người khác nếu không muốn lộ đường dẫn thư mục.
   Hiện chưa có nhập lại file cấu hình này; không coi đây là lưu project video.
6. Dịch vụ quy trình chưa được cấu hình nên nút tạo vẫn không khả dụng. Các
   thiết lập được đóng gói trong `production_settings` cho preflight/start,
   nhưng chưa có xác nhận worker thực thi những tùy chọn đó. Không đồng nghĩa
   đã có phim hoàn chỉnh, khớp môi hay giọng được sao chép.

Build warnings `missing documentation` và `account_changed is never used` không
phải bằng chứng crash. Lần crash được báo trong lượt này có log
`Recursion detected` khi vào Xưởng phim AI; chưa xác định binding gốc. Bản mới
đã qua kiểm tra StartScreen thật: bấm thanh bên vào Xưởng phim, bấm bước 2, dựng
đủ 7 bước bằng renderer phần mềm. Chưa xác nhận lại tương tác trên GPU của người
dùng. Hook
panic nay ghi thêm stack frames, script Windows chỉ ra log mới của lần chạy lỗi.
Không xóa settings/project để thử sửa lỗi khi chưa xác định nguyên nhân.
