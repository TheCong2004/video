# Smart adaptive sidebar

Implementation hiện tại ở `start.slint`: tái dùng Rail/RailItem và cấu hình `StudioPrefs.sidebar_mode`.

| Mode | ID | Hành vi |
|---|---|---|
| Auto | 0 | Đủ rộng mở; trung bình thu, hover/focus mở overlay |
| Ghim mở | 1 | Từ680px trở lên dành252px và giữ mở; dưới680px dùng drawer |
| Thu gọn | 2 | Giữ rail hẹp, nút mở và hover/focus vẫn truy cập nhãn |

Expanded252px, collapsed68px. Auto dưới1100px dành68px và chỉ mở tạm overlay; Ghim mở vẫn dành252px từ680px trở lên. Điều này giữ vùng NexPulse native child ổn định thay vì bị xem như overlay che khuất vĩnh viễn. Dưới680px drawer không dành ngang, rộng tối đa280px hoặc viewport-28px, bất kể preference mode.

Hàng Pin luôn giữ chỗ: khi compact vẫn thấy icon ghim. Mọi tiêu đề nhóm luôn dành16px chiều cao kể cả compact; hover chỉ mở nhãn theo chiều ngang, không đẩy các mục lên/xuống làm trượt vị trí đang trỏ. Không tự reset scroll để mô phỏng mở rail.

Hover mở không tự điều hướng; overlay không đẩy form. Rời chuột chờ420ms và chỉ thu khi không còn keyboard focus bên trong. Tab/Shift+Tab phải giữ rail mở khi focus nằm trong; điều hướng/xử lý Escape/backdrop đóng drawer phù hợp. Không dựa hover cho touch.

Mode được clamp0..2 khi khôi phục và lưu cùng Studio preferences; hover/focus/drawer là trạng thái tạm thời, không ghi thành preference. Resize đóng trạng thái tạm thay vì thay mode người dùng.

Theo yêu cầu sản phẩm mới nhất, không còn mục Hotclip hoặc mục cũ “Video dài → clip ngắn” hiển thị trong launcher. Hotclip chỉ có lối vào native trong editor, cạnh Trợ lý AI; không mở bản Electron nhúng thông qua một mục launcher khác. Việc đổi lối vào không tự chứng minh toàn bộ chức năng Hotclip đã được migrate/kiểm thử; kiểm tra theo component editor thực tế.

Đây là đặc tả khớp source hiện tại; cần native behavior tests cho hover, focus, pin, narrow drawer và retention dữ liệu form, không chỉ tìm chuỗi trong source.
