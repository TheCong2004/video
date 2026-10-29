# Radius

| Vai trò | Theme token | Giá trị |
|---|---|---|
| Button / input | r-control | 12px |
| Navigation | r-nav | 12px |
| Card | r-card | 16px |
| Elevated/large card | r-card-large | 20px |
| Dialog shell specification | r-dialog | 24px |

StudioCard/StudioButton/StudioField/StudioStatus dùng token mới. Dialog cũ chưa tự động được chuyển toàn bộ sang r-dialog; không ghi nhận migration hoàn thành chỉ vì token tồn tại.

Dense editor vẫn giữ r-xs/r-sm/r/r-lg/r-xl = 3/5/7/12/16px để bảo vệ bố cục hiện có. Pill web có thể dùng 999px; đó là quy tắc portable, chưa phải token native riêng.
