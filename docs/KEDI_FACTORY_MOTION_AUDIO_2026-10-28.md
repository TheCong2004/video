# Factory: giọng đọc, chuyển động và luồng dựng

## Vấn đề và thay đổi

Factory cũ tạo mỗi ảnh thành một cảnh đứng yên 5 giây, dùng âm thanh im lặng khi chưa chọn file, và phát lại file âm thanh từ đầu mỗi cảnh. Đầu ra không đáp ứng luồng video giới thiệu sản phẩm.

Luồng mới có 6 bước: sản phẩm → tư liệu → kịch bản → giọng/phụ đề → dựng video → kết quả. Dùng thông tin sản phẩm đã nhập làm bản nháp; người dùng có thể sửa lời đọc. Không tự thêm giá, thông số hay nhận xét chưa được xác nhận.

- Tạo giọng Việt Hoài My/Nam Minh bằng môi trường Edge TTS hiện có; phụ đề lấy timing dịch vụ trả về. Không có tiếng giả hoặc ảnh thay thế khi dịch vụ lỗi.
- File âm thanh riêng phát liên tục toàn video, không lặp lại đầu câu mỗi cảnh. Phụ đề của chế độ này chỉ ước lượng theo kịch bản, được ghi rõ trong UI.
- Có thể giữ âm thanh clip gốc; chỉ ảnh/clip câm thì báo lỗi, không báo thành công cho một video không tiếng.
- Video tự tăng thời lượng theo giọng để không cắt câu. Ảnh có nhịp máy tiến/lùi, giữ đầy đủ trên nền mờ, fade mềm; clip giữ chuyển động thật. Có xuất 9:16 Full HD.
- Mỗi render lưu request, kịch bản, giọng, phụ đề, manifest và log trong thư mục mới. Tiến trình thuộc worker, có giới hạn thời gian và hủy cây tiến trình. Xóa các đoạn MP4 trung gian sau khi ghép thành công.
- Bộ dựng storyboard cũ, Concat editor, OpenTalking và Làm phim AI được giữ nguyên; Factory dùng adapter riêng.

## Tệp mã

- `src/crates/concat-studio/src/factory_edit.rs`: worker Rust, cấu hình đóng băng, tìm Python, cancel/timeouts.
- `src/crates/concat-studio/src/factory_edit.py`: adapter media/TTS/FFmpeg đóng gói vào EXE bằng `include_str!`.
- `src/crates/concat-studio/src/lib.rs`: xuất module adapter.
- `src/crates/concat/src/studio_kedi_factory.rs`: nối callbacks thật, gửi request và đăng ký kết quả.
- `src/crates/concat/ui/workspace/studio-kedi-factory.slint`: sáu bước, kịch bản, giọng/phụ đề và tùy chọn xuất.
- `scripts/test_factory_edit.py`: kiểm thử logic và FFmpeg thực tế.

## Xác minh

- 7/7 kiểm thử adapter pass: thời lượng theo giọng; giới hạn; timing phụ đề; sự cố TTS không tạo video thành công; clip giữ tiếng; chuyển động đo giữa khung hình; âm thanh hai tần số kiểm tra không phát lại đầu cảnh.
- 7/7 kiểm thử design/resource hiện có pass.
- 3/3 kiểm thử adapter Rust thực tế pass: gọi đúng Python/worker đóng gói, render ảnh/giọng thật với đường dẫn Unicode, hủy tác vụ không báo hoàn thành và xác thực không có kịch bản. Mã tái chạy: `scripts/test_factory_edit_adapter.rs`; log `.runtime/factory-edit-adapter-tests.log`. Dùng đúng Rust 1.93.1 của workspace và rlib concat-studio đã build.
- Render mạng thật với ảnh sản phẩm khoai sâm đất: `dist/kedi-test/review-motion-20261028/film.mp4`, 720×1280, 12.699675 giây, AAC có mean volume −16.6 dB, max −1.5 dB. Phụ đề tiếng Việt, 3 cảnh, tiến/lùi xen kẽ. Lần gọi TTS đầu trả NoAudioReceived; một lần thử lại thủ công thành công.
- Render thêm đúng ảnh tăm nước trong phản ánh của người dùng: `dist/kedi-test/review-flosser-motion-20261028/film.mp4`, 10 giây, 720×1280, AAC mean −18.8 dB/max −1.5 dB. Khung hình đã kiểm tra giữ trọn ảnh sản phẩm; có phụ đề và 3 nhịp máy.
- Nhật ký: `.runtime/factory-edit-tests.log`, `.runtime/factory-edit-native-build.log`, thư mục review nói trên.
- Native build thành công, exit 0, 65m58s. `--version` và `--smoke` qua launcher: exit 0, phiên bản 0.2.7.
- Desktop QA trên EXE mới: 6 bước hiển thị; nhập đúng ảnh tăm nước đã dùng trước đó; bản nháp lấy đúng tên sản phẩm; chỉnh lời đọc; chọn chế độ tạo giọng Việt mặc định; nhập thời lượng ảnh 0.5 giây và dựng thành công ngay từ nút Dựng và xuất MP4. Ứng dụng tự khớp thời lượng theo giọng, chuyển sang bước 6 khi hoàn thành.
- Đầu ra của chính EXE: `dist/kedi-test/review-native-20261028/film.mp4`, 6.399675 giây, 720×1280, mean −16.8 dB/max −1.5 dB; 2 nhịp máy, phụ đề theo speech boundaries. Bản gốc nằm trong AppData tại thư mục `render-19060-1793165555538187400-0`.
- Đã kiểm tra Danh sách video: 12 video, mục mới nhất là video Factory mới 1.2 MB, có thumbnail/phụ đề đúng ảnh tăm nước. Mục Video đã xuất là trang dành cho publish riêng, không đồng nhất với thư viện này.
- Làm phim AI vẫn mở trang Làm phim từ ý tưởng với 2 dự án đã lưu. Checksum dự án Bát Tràng đã ghi nhận trước đó vẫn khớp; không sửa hoặc xóa dự án.
- EXE source và `dist/kedi-test/concat.exe` được cập nhật sau khi QA đạt: SHA256 `F7A7770A1BDC12FACA72F3E8E14943B7A2342991BB012886B08DDC5B77EDB889`, 291333632 bytes. Thông tin build: `dist/kedi-test/BUILD_INFO.json`; bằng chứng QA: `.runtime/factory-edit-desktop-verification.json`.
- Mở lại bằng `run-concat.bat` hoặc `scripts/windows-studio.ps1 -RunBuilt`. Cửa sổ test hiện được giữ mở ở kết quả Factory.
- Dọn 6.58 MiB artifact adapter test trong đúng `.runtime/factory-edit-adapter-check` sau khi kiểm tra đường dẫn; giữ nguồn test trong `scripts/`. EXE test cũ đã được thay bằng bản mới. Không xóa dự án, video người dùng, SDK hoặc cache build chính.

## Giới hạn thực tế

Đây là dựng ảnh/video thật có giọng và chuyển động máy, chưa phải mô hình tạo hành động nhân vật từ ảnh. Chưa thêm tìm kiếm clip tự động theo từ khóa, xếp hạng tương đồng hoặc đăng bài production. TTS cần Internet và Python có `edge_tts`; có thể cấu hình `KEDI_TTS_PYTHON`. Nguồn tư liệu vẫn dùng bộ tải đa nền tảng và file trên máy hiện có.

Đã quan sát dịch vụ Edge TTS trả `NoAudioReceived` nhiều lần trước khi thành công ở bản tăm nước, gồm cả hai giọng. Không nâng cấp hoặc thay đổi môi trường dùng chung (đang 7.3.1). Có báo cáo tương tự trong [issue chính thức của edge-tts](https://github.com/rany2/edge-tts/issues/473). Chế độ file âm thanh không phụ thuộc dịch vụ này; lỗi tạo giọng được báo để người dùng thử lại hoặc chọn file, không thay bằng tiếng giả. Chưa tuyên bố TTS ổn định cho production.

Còn cần hoàn thiện: phần trăm tiến độ chưa lấy mốc từ adapter mới (trạng thái văn bản từng cảnh đã có); các nhịp tái dùng một clip hiện bắt đầu từ đầu clip, chưa có timeline cắt liên tục theo offset; câu hướng dẫn cũ ở bước Sản phẩm còn nhắc bước 1 thay vì bước 2 khi chọn tư liệu. Khi cần cắt nối clip chính xác, tiếp tục dùng Concat editor. Các kiểm thử hiện xác nhận chuyển động/âm thanh tồn tại và giọng ngoài phát liên tục, chưa xác nhận dựng nhiều nhịp video nguồn theo timeline.
