# Xưởng phim AI: phân cảnh native

Các chức năng mới nằm trong các bước 2–7 của Xưởng phim AI. Không có ứng dụng
nhúng hay bản Hotclip thứ hai. Giao diện dùng theme Kedi hiện có.

- Bước 2: nhập kịch bản (mỗi đoạn một cảnh), tách cảnh hoặc gọi model đã cấu hình
  trong OmniRoute để viết JSON phân cảnh. Thêm, xóa, nhân đôi, đổi thứ tự, hoàn tác,
  làm lại; ID cảnh giữ nguyên khi đổi vị trí. Phân trang 24 thẻ.
- Bước 3: chọn nền ảnh/video, lớp nhân vật, giọng đọc, nhạc và tối đa 16 lớp hình
  bổ sung. AI phân tích danh sách tài nguyên cần chuẩn bị thành ghi chú; file
  thực tế chỉ được gắn khi người dùng chọn và probe đọc được.
- Bước 4–5: chỉnh bối cảnh, phong cách, góc máy, chuyển động, ánh sáng, biểu cảm,
  chỉ đạo tiếng động; chỉ đạo bằng tiếng Việt qua model đã chọn. Các lớp bổ sung
  có vị trí, chiều rộng và độ hiện riêng. Có preset điện ảnh/lịch sử/chibi/sản phẩm.
- Bước 6: lời dẫn, phụ đề riêng, tạo giọng bằng dịch vụ giọng đang cấu hình hoặc
  tải file âm thanh; đo thời lượng file thật để khớp số giây của cảnh.
- Bước 7: QA tài nguyên/lời dẫn/thời lượng/lặp nội dung; dựng cảnh hoặc cả phim,
  xuất MP4 720p/1080p dọc/ngang/vuông, SRT và nhập MP4 vào media bin Concat.

## Dữ liệu và bản dựng

`concat-studio::storyboard` là kiểu dữ liệu chuẩn. Bản tự lưu ở
`<project>/.concat-storyboard/draft.json`; khi chưa mở dự án, ở dữ liệu ứng dụng
`studio/storyboard/draft.json`. Có mở/lưu bản JSON riêng. 40 bước hoàn tác giữ
trong phiên; bản dựng cuối được lưu cùng dữ liệu phân cảnh.

Mỗi job dựng chụp cấu hình riêng và tạo thư mục `render-<id>`, giữ video cuối,
SRT, cấu hình và nhật ký. Các đoạn MP4 trung gian được dọn sau khi ghép thành công.
Dừng dựng kết thúc FFmpeg của đúng job. Dừng yêu cầu AI không gửi lại yêu cầu
và không áp dụng kết quả về phân cảnh sau khi người dùng đã yêu cầu dừng.

## Ranh giới công cụ

2D/2.5D được dựng từ ảnh/video/lớp hình thật bằng FFmpeg: crop, zoom/pan, lớp
nhân vật, alpha, phụ đề và trộn âm thanh. Các chỉ đạo biểu cảm/hành động/ánh sáng
được lưu cho công cụ sinh media; trình ghép không tự tạo động tác người từ ảnh.
Phong cách 3D dùng video được tạo bởi công cụ hỗ trợ phong cách đó; đây chưa phải
trình chỉnh mesh/rig hoặc renderer Blender.

Nút đưa cảnh vào quy trình Promax chuyển chỉ đạo/lời dẫn/thời lượng vào form
sản xuất hiện có và dùng nguyên lựa chọn Page Nexora. Cấu hình gửi thêm dữ liệu
phân cảnh dạng chữ, không gửi đường dẫn file cục bộ giả làm file đã upload.
Tạo ảnh/video ở cloud vẫn phụ thuộc workflow/worker thật mà BE xác nhận.
Nút nhận video tạo sẵn cho phép gắn file đã tải về vào cảnh và tiếp tục dựng.

## Kiểm tra

- Unit tests: ID, đổi thứ tự, SRT, phản hồi AI không thể tạo đường dẫn đầu ra.
- `CONCAT_TEST_FFMPEG=<ffmpeg.exe> cargo test -p concat-studio --lib storyboard`
  kiểm tra render hai cảnh thật, lớp nhân vật, chuyển động, chữ tiếng Việt,
  luồng video/audio, thời lượng 2,5 giây và dọn đoạn MP4 trung gian.
- Build Concat qua `scripts/windows-studio.ps1 -Build`.
- Bản debug có smoke thao tác native riêng: đặt `CONCAT_STORYBOARD_SMOKE=1`
  rồi chạy `concat.exe --smoke`. Kiểm tra tách/sửa cảnh, số giây lẻ,
  nhân đôi/đổi thứ tự/xóa/undo/redo, QA và tự lưu trong thư mục tạm riêng;
  không sửa dự án người dùng hoặc gọi provider. Nhật ký ghi
  `storyboard smoke: PASS` khi toàn bộ kiểm tra này đạt.
- Các chế độ preview production-scenes/production-director/production-render
  dùng component Slint thật với dữ liệu kiểm tra bố cục, không gọi AI.

### Kết quả kiểm tra ngày 23/10/2026

- `cargo test -p concat-studio --lib` với `CONCAT_TEST_FFMPEG` trỏ tới
  FFmpeg 8.1 trong `.deps`: 186 đạt, 0 lỗi, 6 bỏ qua. Bao gồm dựng MP4
  thật 2,5 giây, kiểm tra luồng hình/tiếng và dọn các đoạn trung gian.
- Bốn bộ hồi quy Node (production-steps, production-promax,
  production-session, kedi-design): 15 đạt, 0 lỗi.
- Tool software-only `concat-studio-preview` build thành công. Đã xem ảnh thật
  của component ở 1440×1000 (sáng/tối), 360×900 và kiểm tra native wheel
  cuộn xuống phần QA/xuất. Ảnh ở `.runtime/storyboard-ui/`; đây là dữ liệu
  bố cục minh họa, không phải video do provider sinh.
- Concat native build thành công bằng `scripts/windows-studio.ps1 -Build`,
  một job, không tạo thêm profile build. EXE hiện tại:
  `src/target/debug/concat.exe`, 269.902.336 byte, hoàn tất lúc 06:44
  ngày 23/10/2026 (UTC+7).
- Chạy EXE đó với `CONCAT_STORYBOARD_SMOKE=1` và `--smoke`: exit 0;
  `storyboard smoke: PASS` và `smoke: ok` trong log
  `concat-20261022-234453.log`. Đã xác minh thao tác cảnh và tự lưu thực tế,
  không chỉ biên dịch. FFmpeg/FFprobe cùng DLL tương ứng được đặt cạnh EXE;
  hai công cụ chạy được không cần PATH từ PowerShell.
- Đã dọn 5,40 GB rồi 9,87 GB cache tái tạo được trong quá trình kiểm tra.
  Không xóa mã nguồn, dự án, model hoặc video người dùng. Một lượt dọn
  thủ công sâu hơn cuối phiên bị cơ chế an toàn từ chối; các archive/cache
  mới hơn ngưỡng của script dọn chuẩn vẫn được giữ, không giả báo đã xóa hết.

Các kiểm tra trên không xác nhận worker cloud đã tạo hoặc tải video thành công.
