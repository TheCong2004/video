# Kedi Liquid Design System V1

Đặc tả dùng chung cho Concat native Rust/Slint và các sản phẩm web tương lai. Đây là tài liệu của mã nguồn hiện tại, không phải tuyên bố mọi màn hình đã được kiểm thử.

Nguồn chuẩn: `ui/theme/theme.slint`, `light.slint`, `dark.slint`. `Floword` chỉ giữ API tương thích và đọc từ `Theme`, không có palette thứ hai.

- [Màu sắc](colors.md), [Typography](typography.md), [Spacing](spacing.md), [Radius](radius.md).
- [Motion](motion.md), [Components](components.md), [Sidebar](sidebar-behavior.md), [Accessibility](accessibility.md).
- [Áp dụng Next.js/CSS](nextjs-adaptation.md).
- [Token JSON](design-tokens.json): giá trị shell Light/Dark đã ánh xạ từ native.
- [Token specimen HTML](design-preview.html): mẫu minh họa token, **không phải screenshot ứng dụng**, không gọi backend.

Không thay Slint bằng React/Electron; không đổi engine, nội dung video, màu chuyên môn timeline/media hoặc hợp đồng backend để phục vụ trang trí.

Giảm chuyển động mặc định bật; preference được lưu trong cấu hình Studio hiện có. Windows đọc thiết lập animation hệ điều hành một lần lúc khởi động; trạng thái hiệu lực là lựa chọn người dùng OR yêu cầu hệ thống. Khi cho phép chuyển động, tối đa hai wash native trong StartScreen dịch chuyển/đổi góc theo chu kỳ 20 giây; khi giảm chuyển động, chúng đứng yên. Không theo dõi thay đổi OS trực tiếp trong phiên. Chưa đình chỉ timer riêng lúc minimize; không tuyên bố hiệu năng hay mức tuân thủ accessibility khi chưa đo/kiểm thử thực tế.
