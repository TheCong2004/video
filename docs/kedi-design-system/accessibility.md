# Accessibility: implementation và kiểm thử

Không tự công bố AA/AAA hoặc tỷ lệ contrast chưa tính. Palette vẫn cần đo theo từng foreground/background, trạng thái disabled, focus và alpha thực tế. Dark text-dim chỉ dùng metadata không thiết yếu nếu chưa đạt contrast cần thiết.

Đã khai báo trong shared controls:

- StudioButton có role/button label/enabled, forward-focus, Enter/Space và focus outer2px.
- StudioField có accessible-label trên TextInput, forward-focus và focus2px.
- Sidebar theo dõi keyboard focus để tránh thu khi đang tương tác.
- Trạng thái có nhãn, không chỉ dựa vào màu.
- Reduced-motion người dùng mặc định bật, preference được lưu. Hiệu lực là user OR OS; wash nền chỉ chuyển động khi cả hai cho phép.

Các hạng mục chưa tự chứng nhận: screen-reader traversal, native keyboard order của tất cả route, focus restore sau mọi modal, touch target ở mọi layout, font zoom/DPI, contrast toàn bộ trạng thái. Không tuyên bố mọi nút trong app đã đạt accessibility chỉ vì shared component được nâng cấp.

Test checklist: Tab/Shift+Tab, Enter/Space, Escape drawer/modal, nhãn Việt dài/có dấu, focus trên primary button, disabled không kích hoạt, effective reduced-motion0ms, 100/125/150/200% scale khi môi trường hỗ trợ. Windows đọc OS animation preference lúc startup; query fail/non-Windows giảm chuyển động an toàn. Không thay system setting; chưa cập nhật live khi OS đổi trong phiên. Khi hệ thống bắt buộc giảm, toggle Settings bị khóa và có nhãn giải thích.
