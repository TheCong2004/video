# Crash khi thu nhỏ cửa sổ Studio

Log của lần lỗi 2026-10-20 ghi `Recursion detected` trong `StudioLivePage`,
qua `StudioStatus` và bộ đo Flexbox. Không phải lỗi cleanup, API key hay backend.

## Tái hiện và sửa

- Kiểm tra dùng StartScreen/Slint thật, bấm TikTok LIVE từ thanh bên rồi đổi
  lần lượt 1600×900, 1080×720, 940×720, 680×720, 650×720, 640×720, 360×900,
  64×64, 1×1, 0×0, 1080×720. Không gọi mạng hoặc mở project của người dùng.
- Trước sửa: panic tại 64×64. Chỉ thay breakpoint là chưa đủ: vẫn panic.
- Stack có số dòng từ preview xác định nhãn trạng thái ở header: đo chữ xuống
  dòng đọc chiều rộng của Flexbox trong khi Flexbox đang tính chiều cao.
- Cố định riêng badge trạng thái 144×32 và hàng tiêu đề cao 36; các vùng khác
  vẫn responsive. Không khóa kích thước cửa sổ hoặc tắt tính năng LIVE.
- Truyền chiều rộng viewport từ launcher xuống LIVE thay vì dùng chiều rộng
  layout đang được đo để quyết định breakpoint/grid. Clamp chiều rộng nội dung
  về >=0 khi cửa sổ tạm nhỏ hơn thanh bên/thanh cuộn.
- Sau sửa: đủ 33 chuyển đổi trên (3 vòng) qua, bao gồm 0×0 và phục hồi.

`scripts/ui-responsive-check.ps1` nay chạy cả kiểm tra chuyển trang Xưởng phim
và resize này. Bài preview có cảnh báo decode ảnh nền trang trí SVG trong renderer
phần mềm; không liên quan panic, nội dung/điều khiển vẫn render.

Script Windows cũng lấy log mới/thay đổi bằng snapshot file thay vì so đồng hồ
local, và giữ nguyên exit code nếu việc đọc log thất bại.

Kiểm tra người dùng sau khi có EXE mới: chạy
`./scripts/windows-studio.ps1 -RunBuilt`, mở LIVE, thu nhỏ/phóng lại nhiều lần;
sau đó thử Xưởng phim AI. Không cần xóa cache, settings hoặc dự án để thử.

## Kết quả bản sửa

- 51 kiểm tra hồi quy qua.
- Bộ QA đầy đủ qua: 105 lượt render và 7 bài nhập liệu/chuyển trang/resize,
  trong đó 33 chuyển đổi kích thước trên StartScreen/LIVE thật bằng Slint
  software renderer. Đây không phải thao tác nút minimize Windows bằng UIA.
- Build debug thành công trong 13m38s. EXE mới: `src/target/debug/concat.exe`,
  238488576 byte, 2026-10-20 01:58 giờ máy (Asia/Bangkok).
- Kiểm tra EXE tách biệt: `LIVE_UI_SMOKE=PASS`, `LIVE_MOTION_SMOKE=PASS frames=12`.
  Không kết nối TikTok thật, gọi AI hoặc dùng dữ liệu dự án thật.
