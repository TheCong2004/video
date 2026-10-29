# Nhân vật chuyển động local trong Concat

Tính năng quét video đã chuyển khỏi Promax. Không cần Cloudflare, Node, tài khoản
Promax, API key hoặc ArtCraft để dùng chức năng này.

## Cách dùng

1. Mở lại Concat vừa build, vào **Tạo Video AI → Kho Nhân Vật**.
2. Bấm **Kiểm tra Concat local / Tải lại danh sách**. App tự mở bộ quét ẩn khi cần.
3. Chọn MP4/WebM dài 1–30 giây, tối đa 25 MB; một người, khuôn mặt rõ.
4. Đặt tên và bấm **Quét nhân vật trên máy**. Video không gửi ra Internet.
5. Chờ trạng thái **Sẵn sàng**, chọn nhân vật và bấm **Phát nhân vật Concat local
   trên LIVE**. Cửa sổ đầu ra sạch nhận cùng khung hình chuyển động.

Nếu nhãn engine OpenTalking cũ phía trên báo OFFLINE, đó không phải trạng thái
của bộ quét local mới. Xem dòng **Concat local đã sẵn sàng** ngay trong thẻ này.

Video phát lặp chuyển động **đã có trong nguồn**, tiếng gốc được loại bỏ. Chưa
sinh cử chỉ mới, chưa khớp môi theo câu AI mới và chưa sao chép giọng. Luồng giọng
AI/LIVE khác không bị đổi thành offline bởi việc chuyển bộ quét này.

### Điều phối giọng mẫu và khớp môi (cần build mới)

Trong **TikTok LIVE → Cài đặt nâng cao**, có thể gán clip chào/trả lời/giới thiệu,
nối dịch vụ giọng VieNeu từ mã nguồn SLive đã có và nối MuseTalk GPU. Đây là chọn
chuyển động có sẵn, không tự sinh cử chỉ. Giọng mẫu chỉ gửi sau xác nhận quyền sử
dụng; profile lưu trên server. Chưa cài model hoặc đo trên GPU thật. Xem
[cách nối và thử ngoại tuyến](../integrations/lipsync/README.md#native-director-and-existing-slive-sample-voices).

## Runtime và dữ liệu

### Xóa bản lặp trong thư viện

Mỗi bản đã quét có nút **Xóa → Xác nhận xóa bản này**; **Hủy** không gửi yêu cầu.
Nút bị khóa khi bản đó đang quét hoặc đang dùng cho LIVE. Bấm **Dừng video** ở
LIVE trước nếu muốn xóa bản đang phát. App cập nhật danh sách và lựa chọn sau khi
nhận xác nhận từ runtime; không tự xóa các bản trùng tên.

Video gốc do bạn chọn không bị thay đổi. Bản sao và dữ liệu khuôn mặt được chuyển
vào `%LOCALAPPDATA%\app.concat.editor\local-avatars\library\.trash\<id>`;
thao tác này chưa giải phóng dung lượng vì dữ liệu còn có thể khôi phục.
Khi cần khôi phục thủ công: đóng Concat, đưa `record.json` về `library\<id>.json`
và thư mục `media` về `library\<id>`, không ghi đè mục đã có, rồi mở lại app.
Chưa có nút khôi phục hoặc dọn vĩnh viễn trong giao diện.

- Bộ quét SLive 1.0.18 và adapter cũ được copy sang `integrations/slive`;
  mã chuẩn bị khuôn mặt không viết lại. Lớp HTTP loopback là adapter mới cho app.
- Python/dependency/model nằm trong `.runtime` của Concat. Chạy
  `scripts/install-slive-avatar-runtime.ps1` khi cần cài lại;
  thêm `-CheckOnly` để kiểm tra mà không tải thêm.
- Bản đóng gói chứa `AvatarRuntime` cạnh EXE, Python/packages/model và FFmpeg.
  `scripts/windows-package.ps1` gọi `stage-local-avatars.ps1` để copy tài nguyên,
  không đóng gói video cá nhân, thư viện người dùng hoặc secret.
- Không dùng `CONCAT_PROMAX_ORIGIN` hay `CONCAT_PROMAX_AVATAR_TOKEN` nữa.
  `CONCAT_AVATAR_RUNTIME_DIR` chỉ là thư mục tài nguyên local nâng cao.
- Dữ liệu người dùng lưu trong AppDirs của Concat, thư mục `local-avatars/library`.
  Thư viện giới hạn 5 nhân vật active/ready, 100 bản ghi; mỗi lần chỉ quét một video.
- App cấp token ngẫu nhiên mỗi lần mở runtime qua stdin, dùng cổng loopback ngẫu
  nhiên. Chặn Origin trình duyệt, Host không đúng, redirect và request không có
  token. Không tắt xác thực của Promax, không mở dịch vụ ra LAN/Internet.
- Đóng app sẽ dừng process tree do app sở hữu. Job đang chạy khi dừng được báo
  gián đoạn ở lần mở sau; không giả trạng thái sẵn sàng hoặc tự upload lại.

## Kiểm thử

```powershell
# Trong concat/src; test transport không chạy mô hình.
cargo test -p concat-studio --profile quick promax_avatars
$env:CONCAT_LOCAL_AVATAR_RUNTIME_TEST='1'
cargo test -p concat-studio --profile quick local_avatars
cargo test -p concat-studio --profile quick --test local_avatar_runtime

# Trong concat; test adapter và các kiểm tra an toàn.
.\.runtime\slive-venv\Scripts\python.exe -B -m unittest discover -s integrations/slive/runtime -p 'test_avatar*.py' -v

# Opt-in chạy UI native, app tự mở runtime local và quét thật clip mẫu 2 giây.
.\.runtime\slive-venv\Scripts\python.exe -B scripts/local-avatar-smoke.py
```

Tên module `promax_avatars` và route `/api/v1/concat/avatars` được giữ nội bộ để
tái sử dụng transport đã kiểm thử, không có nghĩa là còn gọi Promax. Native smoke
kiểm tra 50 khung hình và chuyển động ở cả hai cửa sổ, không phát sóng TikTok.
Chưa triển khai lên Cloudflare và không cần triển khai cho chức năng local này.

## Đã xác minh 2026-10-19

- Native EXE build thành công; callback UI tự khởi động runtime, quét thật clip
  mẫu 2 giây thành 50 khung hình và phát ít nhất 3 khung hình khác nhau.
  Cửa sổ đầu ra sạch khớp khung hình LIVE. Không chạy Promax trong lượt này.
- 5 test transport, test process/phiên local, test readiness từ launcher thật và
  9 test Python không cần model đều qua. Bài quét thật Python chạy riêng đã qua.
- Runtime đóng gói sang thư mục khác vẫn khởi động và báo ready. Đã thêm hồi quy
  cho lỗi Python con thừa kế stdin theo dõi parent; worker dùng stdin riêng.
- Promax đã gỡ module/API quét mới, script cài và Python/model tương ứng;
  build Promax sau khi gỡ đã qua. Runtime cũ và output build cũ được giữ làm
  backup trong `.runtime/migrated-promax-avatar-backup-20261019` của Concat.
- Chưa chạy UIA/chuột thủ công, chưa tạo bộ cài Windows mới và chưa phát sóng
  TikTok. Các test native dùng callback thật và thư mục người dùng tách riêng.
