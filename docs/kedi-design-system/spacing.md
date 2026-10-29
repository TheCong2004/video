# Spacing

| Portable | Theme token | Giá trị |
|---|---|---|
| space-xs | space-xs | 4px |
| space-sm | space-sm | 8px |
| space-md | space-md | 16px |
| space-lg | space-lg | 24px |

StudioCard mặc định spacing 16px, padding ngang/dọc 18px (caller có thể chỉnh). Dense editor giữ gap 6px, pad 14px và control-height 28px; không đổi kích thước track/splitter vì token shell.

StudioButton cao 40px, compact 34px; StudioField một dòng tương tự, multiline mặc định 128px. Ưu tiên wrap và min-width 0 cho vùng co giãn. Kiểm tra nội dung thực tại viewport 320–1920px và display scaling; các mốc test không phải breakpoint bắt buộc.
