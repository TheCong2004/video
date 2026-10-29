# Shared component contracts

Tái dùng `workspace/studio-chrome.slint`; callback vẫn do caller sở hữu, không đổi API backend.

| Component | Presentation / contract |
|---|---|
| StudioCard | surface, 1px border, radius 16px; elevated 20px và raised; spacing16/padding18px |
| StudioHeading | icon + title wrap, default16px, không thay page header bằng control giả |
| StudioButton | label/primary/enabled/compact, callback clicked(); 40/34px, radius12; primary brand/on-brand; secondary field; pressed rõ, focus outer2px; disabled0.55 |
| StudioField | label/value/placeholder/multiline/secret/enabled/compact/input-height; callback edited(string); draft giữ khi focus; label accessible, selection colors; focus2px |
| StudioStatus | tone 0neutral/1success/2warning/3error/4loading; nhãn và màu đồng bộ; không tự báo backend thành công |
| StudioMetric | label/value từ dữ liệu caller, không hardcode số liệu giả |
| LiquidBackground | hai wash trong StartScreen, không bắt pointer/đặt trên video; chuyển phase10s và animate10s (chu kỳ20s) chỉ khi active và effective motion cho phép; reduced fallback tĩnh |

StudioButton forward-focus tới FocusScope, Enter/Space kích hoạt khi enabled. StudioField forward-focus tới TextInput. Kiểm thử native keyboard/screen-reader vẫn cần thực hiện; declaration không phải chứng nhận accessibility.

Loading/error/empty/cancel thuộc trạng thái thật của module. Khi backend chưa sẵn sàng, giữ nguyên giới hạn và thao tác cấu hình thật; không tạo nút giả. Dialog/tooltip/select/switch hiện có tiếp tục dùng primitives, không viết hệ thống trùng lặp.
