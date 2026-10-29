# Typography

Giữ Hanken Grotesk đã đóng gói cho native; không thêm dependency font. Portable web dùng cùng font nếu được cấp phép/đóng gói, fallback system-ui/sans-serif; không bắt buộc tải Inter.

| Token native | Giá trị |
|---|---|
| Theme.fs-display | 34px |
| Theme.fs-page-title | 24px |
| Theme.fs-section-title | 18px |
| Theme.fs-body | 14px |
| Theme.fs-input | 15px |
| Theme.fs-xs / fs-sm / fs / fs-lg / fs-xl | 10 / 11 / 12 / 13 / 14px |
| Theme.weight-body / weight-title / weight-display | 400 / 500 / 700 |

StudioHeading mặc định 16px đậm; button 13px (compact 12px), field 13.5px (compact 12px). Các số nhỏ được giữ cho editor dày đặc; không phóng to timeline hay timecode hàng loạt.

Web khuyến nghị line-height 1.45 cho body, 1.25 cho heading. Native dùng text metrics của renderer. Test chữ Việt có dấu, đường dẫn dài, zoom/display scale và nhãn xuống dòng; không bảo đảm không cắt chữ chỉ bằng việc chọn font.
