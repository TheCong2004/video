# Color tokens

Giá trị đầy đủ và máy đọc được nằm trong [design-tokens.json](design-tokens.json). Đây là giá trị implementation, có một số khác biệt so với palette định hướng trong prompt.

| Vai trò portable | Native | Light | Dark |
|---|---|---|---|
| background-primary | Theme.launch-page | #F7FAFE | #0B1221 |
| background-secondary | Theme.launch-well | #EDF5FC | #121D30 |
| surface-primary | Theme.launch-panel | #FFFFFF | #172339 |
| surface-secondary / field | Theme.launch-field | #F5F8FC | #1D2D46 |
| surface-raised | Theme.launch-raised | #FFFFFF | #1D2D46 |
| text-primary | Theme.fg | #17365D | #F5F5F7 |
| text-secondary | Theme.fg-muted | #506985 | #A1A1A6 |
| text-muted | Theme.fg-dim | #637B97 | #6E6E73 |
| brand-primary | Theme.brand | #315DCC | #7BA6FF |
| brand-hover | Theme.brand-hover | #264DAF | #96B9FF |
| brand-soft / surface-tint | Theme.brand-soft | #E6EEFF | #1E2D4A |
| on-brand | Theme.on-brand | #FFFFFF | #0B1221 |
| accent-yellow | Theme.brand-yellow | #FFC83D | #FFD359 |
| navigation-selected | Theme.nav-selected | #FFF3CF | #382F16 |
| navigation-selected-ink | Theme.nav-selected-ink | #17365D | #FFE8A3 |
| border-default (Studio) | Floword.line | #DCE6F2 | #304460 |
| border-strong | Theme.line-strong | #B4C7DE | #FFFFFF33 |
| border-soft | Theme.line-soft | #EAF0F7 | #243552 |
| success | Theme.success | #087F5B | #38D9A5 |
| warning | Theme.warning | #94600C | #FFBE62 |
| danger | Theme.danger | #D70015 | #FF453A |

Studio dark border-default đọc `launch-field-active`; editor dark `Theme.line` vẫn là #FFFFFF1A trên bề mặt trung tính. Không dùng token shell để tô video stage hoặc thay màu phân loại clip, waveform, meter, playhead. Màu selection của editor vẫn do accent preference riêng quyết định.

Không dùng riêng màu để báo lỗi/thành công: thêm nhãn, biểu tượng hoặc trạng thái. Màu text-muted cần kiểm tra với background cụ thể trước khi dùng cho thông tin thiết yếu.
