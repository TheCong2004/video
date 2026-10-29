# Motion, OS preference và static fallback

Một nguồn preference người dùng: `Theme.reduced-motion`; `Floword.reduced-motion` là alias hai chiều. Mặc định **true**. Root khôi phục/lưu preference trong cấu hình Studio, không tạo kho cấu hình thứ hai.

`Theme.system-reduced-motion` được đọc một lần lúc khởi động qua `platform::system_reduced_motion()`. Windows dùng GET `SPI_GETCLIENTAREAANIMATION`; không sửa system setting. Query thất bại hoặc non-Windows dùng fallback giảm chuyển động. `Theme.motion-reduced = reduced-motion || system-reduced-motion`: yêu cầu hệ thống không thể bị user toggle ghi đè. Settings hiển thị nhãn theo hệ điều hành và khóa toggle khi system yêu cầu giảm. Thay đổi OS trong lúc app mở chưa được theo dõi; cần mở lại app để đọc lại.

| Token | Khi cho phép motion | Reduced |
|---|---|---|
| motion-hover | 160ms | 0ms |
| motion-press | 100ms | 0ms |
| motion-tab | 220ms | 0ms |
| motion-panel | 260ms | 0ms |

Các token press/tab/panel là API có thể tái dùng, không chứng minh mọi component đã có transition tương ứng. Studio chrome dùng motion-hover, easing cubic-bezier(0.16,1,0.3,1). Sidebar có transition 240ms và delay rời 420ms; reduced-motion bỏ transition nhưng delay tương tác vẫn giữ.

LiquidBackground gồm hai wash brand/yellow alpha 0.06/0.07, chỉ đặt trong StartScreen (launcher/Studio), không đè preview/timeline. `moving = active && !Theme.motion-reduced`. Khi moving, native Timer mỗi10s đảo phase; x/y/border-radius animate10s ease-in-out, tạo chu kỳ20s. Khi moving tắt, drift về false và duration0ms: fallback tĩnh. Không shader/backdrop-filter hoặc polling media.

StartScreen bị hủy khi vào editor nên component/timer không còn ở editor. Timer hiện chưa được đình chỉ riêng lúc minimize; việc renderer hệ điều hành giảm drawing không phải bằng chứng timer đã dừng. Chưa kiểm thử lifecycle minimize hoặc benchmark CPU/GPU so sánh, vì vậy không khẳng định mức tăng hiệu năng.
