# AI Edit: AI bên ngoài và MCP local

## Cách nhanh: mở chat, sao chép và dán trong pane

1. Viết yêu cầu trong **Trợ lý chỉnh sửa AI → Chat web / MCP**.
2. Bấm **Mở ChatGPT** hoặc **Mở Gemini**: mở trình duyệt mặc định, không phải
   một browser nhúng. Bấm **Sao chép yêu cầu + timeline**, tự dán/gửi vào chat.
3. Sao chép JSON AI trả lời, bấm **Dán kết quả từ chat web**, Ctrl+V vào ô và
   **Kiểm tra và tạo bản nháp**. Có thể dán JSON thuần hoặc một khối ```json.
4. Xem trước và tự duyệt. Sai scope/schema hoặc có thêm lệnh khác sẽ bị từ chối.

Không có thao tác tự đọc clipboard, DOM, cookie, token hay gọi API nội bộ của
chat. Chỉ bấm Sao chép mới ghi context ra clipboard. Tài khoản/hạn mức AI do
người dùng quản lý trên website. Ô prompt đã nối sự kiện edited vào state
native; không chỉ cập nhật chữ đang hiển thị.

### Về ý tưởng browser nhúng và DOM

Concat hiện dùng Slint native, chưa có browser engine trong pane. Khung browser
thật cần tích hợp engine/lifecycle/focus/resize và lớp adapter theo từng site;
không thể chỉ nhúng iframe và coi như đã có kết nối. DOM tự động không phải API
ổn định; không được bỏ qua CAPTCHA, lấy cookie hay gọi private endpoint.

Google [không hỗ trợ đăng nhập bằng embedded webviews](https://developers.google.com/identity/siwg/best-practices).
OpenAI [hạn chế trích xuất tự động dữ liệu/output](https://openai.com/policies/terms-of-use/).
Vì vậy bản này giữ thao tác chat web do người dùng thực hiện; không tuyên bố
đã triển khai browser nhúng hoặc scraper ChatGPT/Gemini. MCP local vẫn là đường
tự động riêng dành cho client hỗ trợ stdio, có quyền tài khoản phù hợp.

## Dùng ngay với ChatGPT/Gemini web, không nhập API key vào Concat

1. Mở dự án. Nhập tư liệu và kéo clip vào timeline nếu muốn cắt/chỉnh video.
   Timeline trống vẫn có thể tạo tiêu đề; không có video để AI phân tích.
2. Mở **Trợ lý chỉnh sửa AI → AI bên ngoài** và viết yêu cầu.
3. Bấm **Xuất context cho AI**. File chỉ chứa ID/thời gian, yêu cầu và schema
   EditPlan; không tự gửi video, hình, đường dẫn nguồn hay API key.
4. Gửi file cho AI của bạn với yêu cầu: “Đọc instruction và editPlanSchema.
   Trả một file JSON thuần `{scope,plan}`; giữ nguyên scope, timelineId và
   baseRevision. Không dùng Markdown, không tự bịa ID, không nói đã áp dụng.”
5. Bấm **Nhập bản nháp JSON** trong Concat. Nếu sai schema, ID hoặc phiên bản
   timeline, Concat từ chối; không áp dụng một phần.
6. Bấm **Xem trước bản gốc / bản nháp**, kiểm tra rồi **Áp dụng bản nháp**.
   Undo hoàn tác toàn bộ lần áp dụng. Đổi timeline/dự án làm bản nháp cũ mất hiệu lực.

AI web dùng tài khoản/hạn mức của dịch vụ đó, không phải AI miễn phí do MCP
cung cấp. Context hiện là cấu trúc timeline, không phải nhận diện nội dung video.
Không hứa tự phát hiện diễn viên/cảnh/lời thoại khi không có dữ liệu đó.

## MCP tự động với client local hỗ trợ stdio

1. Trong AI Edit, bấm **Bật kết nối MCP local**.
2. Bấm **Lưu cấu hình MCP cho client bên ngoài**. File JSON chứa command/args
   của adapter và đường dẫn connection `.dpapi`, không chứa API key hay token rõ.
3. Với Gemini CLI, ghép mục `mcpServers.concat` vào cấu hình MCP hiện có của
   client. Không ghi đè toàn bộ settings nếu bạn đã có cấu hình khác.
   Khởi động lại client, kiểm tra danh sách MCP có ba tool dưới đây.
4. Đăng nhập/quyền model thuộc client bên ngoài. Gemini CLI hỗ trợ đăng nhập
   Google cho các tài khoản phù hợp, không bắt buộc API key. Không sao chép cookie
   trình duyệt hoặc token tài khoản vào Concat.
5. Yêu cầu client lấy `concat_get_context`, lập kế hoạch rồi gọi
   `concat_propose_edit`. Quay về Concat để xem trước và duyệt.

| Tool | Quyền |
| --- | --- |
| `concat_get_context` | Đọc timeline đang mở + scope + schema |
| `concat_get_status` | Đọc trạng thái bản nháp |
| `concat_propose_edit` | Chỉ chuẩn bị bản nháp, không commit/save/export |

Không có tool approve, shell, chạy command tùy ý hay đọc file tùy ý. MCP context
và JSON nhập tay cùng dùng `EditPlan::parse/compile`, draft của Session/Editor
hiện có, preview và human approval hiện có — không có timeline engine thứ hai.

Adapter là **MCP stdio**; cổng TCP nội bộ là handoff riêng tới cửa sổ Concat,
không phải URL HTTP MCP để dán vào ChatGPT web. Chưa cung cấp remote HTTPS MCP
hoặc tunnel public. Gemini CLI/Codex cần cấu hình stdio theo client riêng; test
hiện xác minh SDK stdio, không xác nhận mọi phiên bản client/thực tế tài khoản.

Giữ cửa sổ Concat mở. Đóng pane AI không cấp thêm quyền và không tắt Concat;
timer của app tiếp tục xử lý bridge đã bật. Bấm Tắt MCP hoặc đóng app để thu hồi.
Mỗi lần bật tạo cấu hình riêng: sau tắt/bật cần lưu/cập nhật lại cấu hình client.
Không chia sẻ file `.dpapi`; chỉ cùng Windows user có thể giải mã. Nếu app bị
kill đột ngột, file mã hóa cũ có thể còn trên đĩa nhưng không phục hồi cổng/token.

## API / OmniRoute

Giữ cách dùng API cũ: thêm key/kết nối riêng trong AI Keys, tải model, chọn model
rồi tạo bản nháp. Có model trong catalog không chứng minh có kết nối, quyền,
quota hoặc hỗ trợ structured output. UI và native guard chặn gửi khi không có
kết nối đang bật. Không tự retry inference khi kết quả không rõ.

Chẩn đoán 2026-10-17 với runtime sạch: `PROVIDERS=Ok(0)`, `MODELS=Ok(115)`.
Điều này xác minh catalog chạy ở runtime được stage, không tái hiện đầy đủ lỗi
InvalidResponse trong EXE cũ của người dùng. Không kết luận key/model thật hoạt
động vì chưa gọi inference trả phí.

## Build và kiểm tra

```powershell
cd D:\capcutpolot\concat\tools\ai-edit-mcp
pnpm install --frozen-lockfile --ignore-scripts
pnpm test
cd D:\capcutpolot\concat\src
cargo test -p concat-studio --profile quick
cargo run -p concat-agent --profile quick --example mcp-native-smoke
```

Native Windows build dùng `scripts/windows-studio.ps1` để nhận đủ FFmpeg/ORT/
Sherpa và scratch trên D. `windows-package.ps1` bắt buộc SDK runtime MCP, copy
Node và dependency hoisted, bỏ `.pnpm`/junction và không đóng gói connection
user. Một EXE cũ chưa rebuild không có các nút/logic mới.

## Tài liệu chính thức đã đối chiếu

- [MCP SDK: stdio, tools và quyền](https://ts.sdk.modelcontextprotocol.io/server)
- [Gemini CLI: cấu hình MCP](https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/mcp-server.md)
- [Gemini CLI: đăng nhập Google hoặc API key](https://geminicli.com/docs/get-started/authentication/)

## Bằng chứng kiểm thử (2026-10-17)

- MCP SDK tests: 4/4; cùng 4 test qua bản copy mô phỏng package, bundled Node,
  PATH chỉ System32/Windows, không phụ thuộc Node/package toàn cục.
- Rust Studio suite: 104 passed, 0 failed, 4 opt-in ignored; bao gồm queue TCP,
  DPAPI, token sai và cấm thao tác approve/command tùy ý.
- `mcp-native-smoke`: SDK stdio thật → DPAPI → TCP → native EditPlan draft;
  từ chối scope cũ; project chưa đổi trước approval; commit và Undo thành công.
  Fixture không phải GUI, không gọi model, không dùng project người dùng.
- Wiring regressions: 12/12 (9 auth/key/runtime + 3 AI Edit external).
- Dependency audit cho adapter: 0 advisory ở lần kiểm tra này.
- Native `CheckOnly`: passed 8m07s; build EXE thành công 12m46s, sau đó build
  cập nhật cuối thành công 3m37s (gồm chặn JSON trả về sau hủy/đổi dự án).
  Không claim đã kiểm thử GUI/file dialog/model thật.
- Bộ đóng gói `dist/windows/Concat-20261017-005933-878`: DLL loader và
  native startup `--smoke` đều exit 0 với PATH chỉ Windows/System32;
  APPDATA/LOCALAPPDATA/TEMP/TMP cô lập trên D, không mở project người dùng.
  MCP SDK/server có trong gói, 0 reparse point và không cần `.pnpm` bên ngoài.
- SHA256 `Concat.exe`:
  `3C84DA5CA83A1EDFA05BB1F4FABA623819F5E999A4AD7329B275329C5EBC52D2`.
  Đây là build dev/debug; không tuyên bố hiệu năng hay tối ưu release.
- Adapter trong chính bộ đóng gói: import SDK + tạo MCP server thành công
  bằng bundled Node với PATH chỉ Windows/System32 (`PACKAGED_MCP_IMPORT=PASS`).
- Bộ cài NSIS tạo thành công (exit 0), chưa cài đè bản của người dùng:
  `dist/windows/ConcatStudio-Setup-x64-20261017-010143.exe`, 242573676 byte,
  SHA256 `702BFC69B8DF69CA7BCA6C9079ECC39CB0F3B70913A5C6F811DBAFFC698C27F1`.
  Chưa ký số; Windows có thể hiển thị cảnh báo. Scratch NSIS đã tự dọn.
  Hướng dẫn trong bộ cài có quy trình sử dụng; mục bằng chứng bổ sung cuối này
  nằm trong bản tài liệu source. Build tự dọn khoảng 7.64 GB cache tái tạo
  qua hai lần build; giữ lại deliverable và output hiện tại để chạy/dev.

## Tự kiểm tra chất lượng bàn giao

Đánh giá 3.8/5: đã có kết nối MCP thật và đường nhập/xuất không cần key,
nhưng chưa kiểm chứng thao tác GUI và một phiên AI bên ngoài trên tài khoản thật.

| Tiêu chí | Điểm | Bằng chứng và phần cần cải thiện |
| --- | --- | --- |
| Chính xác | 4/5 | SDK stdio, DPAPI, TCP, draft/commit/Undo qua fixture; lỗi InvalidResponse của EXE cũ chưa tái hiện đầy đủ. Cần đối chiếu log đúng bản người dùng. |
| Đầy đủ | 3/5 | Có hai chế độ, import/export, bảo vệ scope và approval; chưa chạy phiên Gemini CLI thật hoặc GUI file picker. Cần kiểm thử có người dùng, không coi fixture là GUI end-to-end. |
| Rõ ràng | 4/5 | UI tiếng Việt theo ba bước, nêu rõ catalog khác kết nối; cấu hình MCP vẫn đòi hỏi ghép settings của client. Cần hướng dẫn theo đúng client/phiên bản người dùng. |
| Dùng được ngay | 4/5 | Có adapter, dependency khóa phiên bản và lệnh kiểm thử; người dùng vẫn cần cấu hình client ngoài hoặc gửi/nhận file JSON thủ công. Chưa tự cài client vào tài khoản. |
| Ngắn gọn | 4/5 | UI tách ngoại/API; hướng dẫn an toàn làm phần kết nối hơi dài. Có thể rút gọn sau kiểm thử sử dụng thực tế. |

Ưu tiên còn lại: (1) kiểm thử GUI và một phiên Gemini CLI thật với người dùng;
(2) chẩn đoán gateway đang được EXE cũ sử dụng nếu vẫn cần nhánh API.
Không tự gửi yêu cầu AI trả phí hoặc thay tài khoản để đạt điểm kiểm thử.
Người dùng có thể chưa đồng ý rằng đã hoàn thiện trải nghiệm cho đến khi thử
bản mới; báo cáo này không tuyên bố đã xác minh toàn bộ trải nghiệm đó.
