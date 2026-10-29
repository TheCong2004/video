> Cập nhật mới: Factory đã có 6 bước, tạo giọng Việt tự động, phụ đề theo giọng và chuyển động ảnh. Xem [báo cáo motion/audio](KEDI_FACTORY_MOTION_AUDIO_2026-10-28.md). Nội dung dưới đây là lịch sử bản trước.

> Cập nhật: Làm phim AI / Làm phim từ ý tưởng đã được khôi phục thành mục riêng và build lại. Các ghi nhận bỏ mục film dưới đây là lịch sử trước khi khôi phục. Xem [báo cáo khôi phục](KEDI_FILM_RESTORATION_2026-10-28.md).

# Kedi Factory: giao diện và luồng dữ liệu thật

Phạm vi: sửa Kedi Factory trong ứng dụng Concat/Slint hiện tại. Giữ engine Concat, OpenTalking và quy trình nâng cao đang có.

## Thay đổi

- `src/crates/concat/ui/start.slint`: tách RailItem Kedi Factory ra khỏi RailItem khác, bỏ mục Làm phim AI; giữ Quy trình nâng cao và Video nhân vật độc lập.
- `src/crates/concat/ui/workspace/studio-pane.slint`: tiêu đề không emoji, bỏ thanh điều hướng trùng; route film cũ chuyển sang Factory.
- `src/crates/concat/ui/workspace/studio-kedi-factory.slint`: thay dashboard sáu cột cố định bằng ba bước Nguồn tư liệu / Sản phẩm / Dựng và xuất. Dùng Hanken Grotesk, Theme, StudioCard, StudioField và StudioButton chung. Bỏ sản phẩm túi sưởi, điểm khớp, lượt xem, tài khoản Admin/Pro, job và tiến độ giả.
- `src/crates/concat/src/lib.rs`: ngừng bind engine làm phim từ ý tưởng để không tự tạo dự án mẫu Bát Tràng. Không xóa dự án/tư liệu cũ của người dùng.
- `src/crates/concat/src/studio_kedi_factory.rs`: callback thật cho đọc URL, chọn file, chọn chất lượng, tải/dừng/thử lại, nhập ảnh sản phẩm, dựng/dừng/mở đầu ra. Resolve và download dùng token riêng; kết quả resolve cũ bị bỏ qua. Cập nhật đúng job theo ID, lưu lịch sử, không đánh dấu hoàn thành khi lỗi/hủy/không có đầu ra. Đăng ký đầu ra trong thư viện video chung.
- `src/crates/concat-studio/src/kedi_factory.rs`: kết quả tải phản ánh lỗi/hủy/đầu ra một phần, lưu/khôi phục tác vụ, tạo storyboard từ file thật, ffprobe kiểm tra hình và thời lượng trước khi công bố thành công.
- `src/crates/concat-studio/src/factory_product.rs`: đọc link Shopee VN đầy đủ; nhận mã shop/sản phẩm từ đường dẫn, không lấy nhầm mã referral. Chỉ tải mã ảnh có trong link từ CDN HTTPS cố định, không cookie/đăng nhập/redirect; giới hạn dung lượng và số ảnh. Tên từ slug là gợi ý cần kiểm tra; không tự tạo giá, đánh giá hoặc doanh số.
- `src/crates/concat-studio/src/lux.rs`: chọn bộ tải yt-dlp theo provider registry, mở rộng đúng các nền tảng đã khai báo thay vì chỉ YouTube/TikTok.
- `scripts/run-kedi-test.ps1`, `run-concat.bat`: tìm runtime FFmpeg/ONNX/Sherpa đúng, có đường chạy hồ sơ thật và hồ sơ test riêng; tắt Kedi Live mock trong launcher test.

## Đối chiếu nút và tác vụ

| Nút | Hành vi |
|---|---|
| Đọc thông tin video | Lux/yt-dlp đọc metadata thực, hiển thị lỗi từ nguồn |
| Chọn video/chất lượng | Sửa lựa chọn của đúng video; tải dùng stream ID đã chọn |
| Tải / Dừng / Thử tải lại | Worker thật; progress từ bộ tải; retry khởi động worker mới |
| Chọn ảnh/video | Native picker; giữ file thật để dựng, không sửa file gốc |
| Nhập sản phẩm từ link | Parser Shopee + tải ảnh công khai có mã trong link |
| Chọn từ danh mục | Đọc danh mục qua client hiện có; chọn chỉ sao chép dữ liệu vào Factory, không ghim/lưu sản phẩm lên tài khoản |
| Chọn giọng đọc/nhạc | Import file âm thanh trên máy; không giả lập TTS |
| Dựng / Dừng dựng | Storyboard renderer FFmpeg có sẵn; tiến độ theo số cảnh thực đã dựng |
| Mở vị trí video | Reveal đúng đường dẫn đầu ra đã kiểm tra |

## Kiểm thử đã chạy

- 10 kiểm thử Factory và sản phẩm: lifecycle, hủy/lỗi/đầu ra một phần, thiếu tư liệu/thời lượng sai, lưu và mở lại tác vụ, link referral/mã ảnh không hợp lệ, dựng video thật và từ chối MP4 giả.
- 9 kiểm thử downloader và 5 kiểm thử provider: đều pass.
- Toàn bộ unit suite `concat-studio` chạy tuần tự với localhost: 256 pass, 0 fail, 6 ignored. Lần chạy song song trong sandbox có 3 bài HTTP thất bại; kết quả chạy tuần tự được giữ riêng tại `.runtime/kedi-factory-studio-regression-localhost.log` cùng log lần đầu để đối chiếu.
- 4 kiểm tra design token/contrast/preference hiện có: pass.
- Native Slint pointer/keyboard: chọn bước Sản phẩm và gõ chữ Việt tới state thật; mặc định không có sản phẩm/selection giả.
- Render giao diện component thật ở 360, 720, 1280 px, light/dark; sửa trường hợp hàng nút và chú thích bị cắt ở màn hình hẹp. Ảnh: `.runtime/factory-ui-review/`.
- Video công khai Samplelib: adapter đọc metadata → tải → ffprobe → render MP4 dọc → ffprobe: pass. Log `.runtime/kedi-factory-public-check.log`.
- Sản phẩm người dùng gửi: shop `1185752780`, item `54703416741`, ảnh `vn-11134207-820l4-meyda3stmo0404`. Parser/CDN thật → render → ffprobe: pass; MP4 720×1280, 5.021333 giây. Log `.runtime/kedi-factory-shopee-check.log`; đầu ra `.runtime/factory-shopee-check/render/film.mp4`.

## Giới hạn thực tế

- Không đọc được giá từ HTML công khai trong lần kiểm tra Shopee này. Giá được để trống. Chưa đọc tự động mọi trang thương mại điện tử hoặc link rút gọn; link không có mã ảnh cần chọn tư liệu trên máy hoặc dùng danh mục đã kết nối.
- Không có tìm video bằng từ khóa/điểm khớp AI hay TTS giả. Chọn URL hoặc file thật, âm thanh có sẵn. Khả năng tải từng URL phụ thuộc website và binary; không tuyên bố mọi nền tảng đã được kiểm thử trực tiếp.
- Render dùng thời lượng mỗi cảnh, crop theo tỷ lệ, chữ nhập tay/tên và giá, file âm thanh tùy chọn. Âm thanh file được phát lại từ đầu mỗi cảnh; không tự giữ âm thanh video nguồn. Giao diện ghi rõ hành vi này.
- Không upload hoặc kết nối livestream production; không gọi thao tác ghi tài khoản thật.

## Build và kiểm tra cửa sổ desktop

- Build native hoàn tất: `Finished dev profile` trong 32m38s. EXE mới ở `src/target/debug/concat.exe` và `dist/kedi-test/concat.exe`, cùng SHA256 `60F73CAC5DE36BDA2804E4AA3F95A783ED22C564D4235F212DFAD015027E5016`.
- `--version`: Concat 0.2.7, exit 0. Native `--smoke`: exit 0, Intel Iris Xe / DX12. Log `.runtime/kedi-factory-final-build.log`, `kedi-factory-exe-version.log`, `kedi-factory-desktop-smoke.log`.
- Đã mở EXE mới, kiểm tra menu Factory và Quy trình nâng cao là hai mục độc lập, không còn Làm phim AI; kiểm tra bố cục cửa sổ đầy đủ.
- Đã điền đúng link Shopee đầy đủ người dùng gửi qua ô nhập native, bấm Nhập sản phẩm từ link, thấy ảnh thật; chọn Dựng và xuất rồi bấm Dựng và xuất MP4. Ứng dụng báo Video đã xuất và kiểm tra thành công. ffprobe đối chiếu đầu ra 720×1280, 5.021333 giây; `product.json` giữ giá rỗng và đúng nguồn; video được đăng ký trong thư viện chung.
- Kết quả desktop có cấu trúc: `.runtime/kedi-factory-desktop-verification.json`. Bản 5.021333 giây từ lần bấm trong EXE được giữ tại `dist/kedi-test/portable/kedi-factory/downloads/render-31372-1793124577251722500-1/film.mp4`; `desktop-product.json` được chép vào thư mục review.
- Chạy với hồ sơ hiện có: `run-concat.bat`. Chạy hồ sơ test riêng: `run-kedi-test.bat`. Launcher thiết lập đúng đường dẫn runtime đi kèm trong repo.
- Lệnh IDE `scripts/windows-studio.ps1 -RunBuilt` cũng dùng launcher chung, chờ ứng dụng đóng và in rõ đường dẫn EXE. Nhánh này bỏ qua build setup/cleanup. Đã kiểm tra `-RunBuilt -Version` và `-RunBuilt -Smoke`: exit 0; ba kiểm tra launcher/resource hiện có đều pass. Không thay EXE khi sửa launcher này.

## Dọn build cũ

- Đã xóa 1.811,7 MiB cache thư viện ứng dụng của các bản build cũ trong `src/target/debug/deps`; giữ cache của cấu hình build hiện tại.
- Đã xóa target kiểm thử độc lập `.runtime/factory-check-target`: 1.770,1 MiB. Giữ bản executable kiểm tra đã đối chiếu SHA256, ảnh, video và log tại `.runtime/factory-ui-review` để có thể xem lại mà không giữ toàn bộ cache phụ thuộc.
- Ổ C: xóa đúng hai thư mục rustc tạm từ ngày 23/10 đã kiểm tra chỉ chứa `.rmeta`, `.def`, `.o` (khoảng 0,42 MiB).
- Nhật ký đường dẫn và dung lượng: `.runtime/kedi-factory-cleanup-D.json`, `kedi-factory-cleanup-test-target.json`, `kedi-factory-cleanup-C.json`. Không xóa SDK, model hoặc tư liệu dự án.
- Sau khi EXE mới và thao tác dựng từ GUI thành công, đã xóa bản EXE test cũ 276,2 MiB. Tổng các build/cache cũ và tạm đã xóa khoảng 3,77 GiB; giữ EXE mới và cache native hiện tại. Nhật ký `.runtime/kedi-factory-cleanup-old-exe.json`.

## Video có giọng đọc

- Factory hiện có nhập file âm thanh; chưa có nút tự tạo giọng từ văn bản trong trang Factory. Bản kiểm thử ảnh/chữ ban đầu không gắn âm thanh nên không có tiếng nói.
- Đã tạo lời đọc thật bằng Edge TTS `vi-VN-HoaiMyNeural`, không dùng nhạc/tone thay giọng. Nội dung: “Khoai sâm đất. Gói một ki lô gam. Xem thông tin sản phẩm trước khi đặt hàng.”
- File giọng dài 6.744 giây: `dist/kedi-test/review-20261028/khoai-sam-dat-voice.mp3`. Có thể chọn ở bước 3 và đặt thời lượng mỗi tư liệu 7 giây.
- Bản có giọng đã ghép và kiểm tra: `dist/kedi-test/review-20261028/khoai-sam-dat-co-giong-doc.mp4`, đồng thời cập nhật bản `khoai-sam-dat.mp4` trong thư mục review. H.264 720×1280, AAC có tiếng, tổng 7.103353 giây; âm lượng trung bình -19.5 dB. Lời đọc được giữ đủ, không cắt ở mốc 5 giây.
- Log và metadata kiểm tra: `.runtime/factory-voice-fix/verification.json`, `output-volume.log`. Đây là cập nhật tư liệu/video; EXE không thay đổi.
