# Portable adaptation: CSS / Next.js / Tailwind

Chỉ là hướng dẫn cho sản phẩm web tương lai; **không tạo app React/Next.js trong repo native**. Palette canonical native nằm trong Theme; JSON là snapshot portable cần giữ đồng bộ.

Ánh xạ kebab-case trong `design-tokens.json` thành CSS variables với tiền tố `--kedi-`. Ví dụ:

```css
:root {
  --kedi-background-primary: #F7FAFE;
  --kedi-surface-primary: #FFFFFF;
  --kedi-text-primary: #17365D;
  --kedi-brand-primary: #315DCC;
  --kedi-on-brand: #FFFFFF;
  --kedi-navigation-selected: #FFF3CF;
  --kedi-navigation-selected-ink: #17365D;
  --kedi-radius-control: 12px;
  --kedi-space-md: 16px;
  --kedi-motion-hover: 0ms; /* safe default */
}
[data-theme="dark"] {
  --kedi-background-primary: #0B1221;
  --kedi-surface-primary: #172339;
  --kedi-text-primary: #F5F5F7;
  --kedi-brand-primary: #7BA6FF;
  --kedi-on-brand: #0B1221;
  --kedi-navigation-selected: #382F16;
  --kedi-navigation-selected-ink: #FFE8A3;
}
[data-motion="full"] { --kedi-motion-hover: 160ms; }
@media (prefers-reduced-motion: reduce) {
  :root, [data-motion="full"] { --kedi-motion-hover: 0ms; }
}
.kedi-button {
  background: var(--kedi-brand-primary);
  color: var(--kedi-on-brand);
  border-radius: var(--kedi-radius-control);
  transition: background-color var(--kedi-motion-hover);
}
.kedi-button:focus-visible { outline: 2px solid var(--kedi-brand-primary); outline-offset: 3px; }
```

Hoàn thiện tất cả biến bằng palette JSON, không chỉ copy subset ví dụ. SSR nên khôi phục preference trước paint để tránh flash; dùng kho cấu hình hiện có của ứng dụng web, không tự thêm localStorage song song. OS media query trên là adapter web; native Windows dùng SPI_GETCLIENTAREAANIMATION một lần lúc startup. Cả hai nên kết hợp user OR system reduced preference; web media query không thay cho kiểm thử native hoặc chứng minh native cập nhật OS trực tiếp trong phiên.

Tailwind ánh xạ semantic value:

```ts
// Tailwind configuration syntax phải phù hợp phiên bản của dự án đích.
const colors = {
  "kedi-brand": "var(--kedi-brand-primary)",
  "kedi-surface": "var(--kedi-surface-primary)",
  "kedi-text": "var(--kedi-text-primary)",
};
```

Dùng button/input/label HTML thật, aria-current cho navigation, aria-expanded cho drawer, focus trap và restore khi cần. Không thêm fake search/notification hoặc nút không nối hành động. `design-preview.html` minh họa token local, không phải dashboard chạy backend và không chứa logo/tài nguyên website tham chiếu.
