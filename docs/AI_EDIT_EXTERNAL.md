# AI Edit: chat web kết nối project và MCP local

## Mục Trợ lý AI bên trái (2026-10-18)

Trong thư viện bên trái, bấm **Trợ lý AI** để mở các điều khiển AI trong vùng
nội dung thư viện. Mục này tách khỏi bộ lọc video/âm thanh. Các tab Phương tiện,
Văn bản, Hiệu ứng vẫn hoạt động như trước. Khi chọn mục AI, các dock AI rời được
đóng để giải phóng chỗ cho xem trước; không đóng browser/bridge hay sửa timeline.
Panel nhúng giữ bộ đồng bộ chạy khi đổi về Phương tiện. Đây là thay đổi Slint/
native, cần khởi động lại bản dev đã biên dịch; không chỉ nạp lại JS adapter.

## Gửi chậm / prompt bị ghép lặp (2026-10-18)

- Diagnostic phiên lỗi trả badge “Yêu cầu cần có nội dung và không quá 8000
  byte”, trong khi ô nhập đang chứa context đã ghép. Tái hiện được cùng lỗi
  khi fixture xử lý `submit` sau `click` bất đồng bộ; ca bấm Gửi lại còn cho
  thấy marker bị lồng 3 lần. Cờ bypass chỉ tồn tại trong lần click đồng bộ
  không đủ bảo vệ pipeline submit.
- Giữ cặp requestId/payload đã chuẩn bị. `submit` chậm hoặc click/Enter lại
  trên đúng payload đó đi qua website, không đọc context/ghép prompt mới.
  Chờ nút enabled tối đa 2s thay vì cố định 100ms. Không tự gửi lại nếu website
  chưa nhận click, không tuyên bố “đã gửi” chỉ từ `button.click()`.
- Payload cũ không thuộc request đang giữ bị chặn với hướng dẫn xóa draft,
  tránh đưa context 30KB vào giới hạn 8000-byte dành cho yêu cầu người dùng.
  Phản hồi native vẫn chỉ áp dụng requestId đúng một lần.
- Hai test mới deferred-submit/manual-send đã fail trước sửa rồi pass sau
  sửa. Chỉ fixture cục bộ, không gửi lại prompt hay gọi AI thật của người dùng.
- Full JS suite sau sửa: 27/27 qua, gồm nút gửi bật chậm 350ms, submit bất
  đồng bộ, bấm gửi lại mà không lồng context; native five-splits smoke qua.

## Cắt nhiều đoạn (2026-10-18)

- Tái hiện lỗi ảnh: cắt tăng dần 12s rồi 14s trên cùng ID của clip bắt đầu
  10s khiến lần sau nằm ngoài phần trái còn lại. Native từ chối toàn bộ plan,
  không commit một phần. `split.time` là giây tuyệt đối trên timeline.
- Browser prompt nay nêu rõ original ID giữ phần trái, `as` là phần phải,
  dùng `@alias` để tham chiếu. Cắt nhiều lần cùng ID phải giảm dần; kèm ví dụ
  18,16,14,12 cho một clip 10–20s. Yêu cầu tổng N đoạn phải trừ số clip video
  đang có, không đếm title. Không tự sửa/replay một plan AI bị từ chối.
- Kiểm chứng: unit native `split_five_parts_uses_descending_absolute_cuts_and_one_undo`
  qua cả từ chối chuỗi sai, 5 phần đúng, clip kế bên không đổi, Undo một lần.
  `browser-native-smoke --five-splits` qua Chrome DOM → bridge → native:
  5 phần, một commit, stale bị từ chối, Undo khôi phục. Browser dùng phản hồi
  fixture, không phải bằng chứng model thật luôn tạo đúng plan.
- JS/TCP tests 16/16 qua; cần ngắt/mở lại chat để helper đang chạy nhận prompt
  mới. Chưa áp dụng lại yêu cầu hay thay đổi project thực của người dùng.

## Chrome mặc định (2026-10-18)

- Sửa cờ khởi chạy: browser hiển thị phải dùng `windowsHide: false` (chỉ ẩn
  khi headless), đồng thời khôi phục window state khi mở/focus. Đã xác nhận
  bằng Computer Use: Chrome xuất hiện trong danh sách cửa sổ Windows và render
  trang `Concat visible browser probe`. Probe không gửi AI; tự đóng/xóa profile
  tạm. Các test DOM trước đây không đủ chứng minh cửa sổ Windows hiển thị.
  Đã ngắt phiên ChatGPT bị ẩn trong Concat; chưa xác nhận việc mở lại từ nút
  native trong phiên người dùng (click automation chưa khởi chạy adapter mới).
- Sửa cửa sổ trắng: bỏ `--app=about:blank`, dùng Chrome cửa sổ thường có
  thanh địa chỉ và gọi `Page.bringToFront` sau khi chuyển tới AI. Trên phiên
  lỗi thực tế, Gemini đã `ready=complete` nhưng `visibility=hidden`, chưa có
  handshake ô chat; không phải lỗi kết nối backend. Chưa xác định thành phần
  browser nào đang che trang, không tự bỏ qua hộp thoại đăng nhập/đồng bộ.
- Browser fixture chế độ hiện cửa sổ kiểm tra bounds/window state. Không dùng
  `visibilityState` làm bằng chứng độc lập: Windows có thể báo hidden do một
  cửa sổ khác đang che Chrome (hai test visibility cũ đã thất bại vì vậy).
  Cửa sổ app-mode đã mở trước bản sửa phải ngắt rồi mở lại để nhận chế độ mới.
- Ưu tiên Chrome cài hệ thống hoặc theo user; Edge chỉ dự phòng khi thiếu Chrome.
- Đã xác nhận máy này chọn `Google/Chrome/Application/chrome.exe`.
- Kiểm chứng: Node chạy cả ba file test, 22/22 qua (14 unit/TCP, 8 browser
  fixture trên Chrome thật, gồm cửa sổ hiện, Enter và tải lại trang). HTTP AI
  trong fixture dùng dữ liệu cục bộ, không gọi model thật.
- Chạy lại binary `browser-native-smoke.exe` có sẵn với adapter JS mới: qua
  trim/title → native commit một lần → từ chối stale → Undo khôi phục.
  Không rebuild Rust; dòng log Edge trong binary cũ là nhãn cũ, browser được
  chọn bởi JS đã đổi sang Chrome. Chưa đóng gói lại bộ cài trong lần sửa này.

## Bấm mở chat và chỉnh timeline tự động (2026-10-18)

1. Mở project, nhập tư liệu và kéo clip lên timeline.
2. Trong **Trợ lý chỉnh sửa AI → Chat web tự động**, chọn ChatGPT/Gemini/Claude
   rồi bấm **Mở chat & kết nối project**. Không cần ghép cấu hình MCP.
3. Đăng nhập tài khoản AI trong cửa sổ riêng lần đầu. Phiên đăng nhập được giữ
   trong profile riêng của Concat, không sao chép tài khoản từ trình duyệt khác.
4. Nhập yêu cầu ngay trong chat, ví dụ: “Ở clip đầu tiên, bỏ 2 giây đầu. Giữ
   nguyên phần còn lại và thứ tự clip. Thêm tiêu đề Xin chào từ giây 1 đến giây 4.”
5. Khi bạn bấm Gửi/Enter, adapter lấy context mới nhất và đính kèm vào yêu cầu.
   Khi phản hồi có JSON đúng requestId/scope, Concat kiểm tra schema/revision
   rồi áp dụng native. Badge trong cửa sổ chat và trạng thái trong Concat báo
   kết quả thực tế. Ctrl+Z hoàn tác cả lần chỉnh sửa.
6. Tắt **AI tự áp dụng sau kiểm tra** nếu muốn duyệt bản nháp. Bấm **Ngắt kết
   nối chat** hoặc đóng Concat để thu hồi bridge và đóng cửa sổ chat riêng.

Không cần nhập prompt lần hai trong Concat, dán JSON hoặc API key. Adapter dùng
Node 22 đã có trong bộ cài và Chrome trên máy; chỉ dùng Edge dự phòng nếu chưa
cài Chrome. Không tải thêm browser. Profile tách theo trình duyệt và nhà cung
cấp (`connected-ai-chat/chrome/chatgpt`, v.v.), không dùng lại profile Edge
cho Chrome. Sau khi cập nhật từ bản ưu tiên Edge: ngắt chat, đợi cửa sổ cũ đóng
rồi mở kết nối lại; đăng nhập AI một lần trong profile Chrome riêng.
Bridge vẫn chỉ nhận EditPlan qua đường native Session/Commands hiện có.

### Phạm vi và giới hạn

Đây là cửa sổ Edge/Chrome riêng do app mở, không phải iframe/engine nhúng trong
pane Slint. DOM adapter thao tác trên ô nhập/nút gửi và phản hồi đang hiển thị;
không đọc cookie, không gọi private API, không bỏ qua đăng nhập/CAPTCHA. DOM
website có thể thay đổi nên cần cập nhật selector. Nếu AI chỉ trả văn xuôi,
Concat báo chưa có thao tác hợp lệ; có thể gửi lại yêu cầu trong chat.

Chỉ cửa sổ riêng này được kết nối, không tự điều khiển tab trình duyệt khác.
Mỗi lần gửi có requestId mới; kết quả cũ/sai phiên bản bị từ chối, phản hồi
render lại không được áp dụng hai lần, timeout không tự gửi lại thao tác.
Context là cấu trúc ID/thời gian/schema, không chứa video, âm thanh, đường dẫn
nguồn hay key. AI chưa có khả năng tự nhìn/nghe nội dung tư liệu qua context này.

## Nhập/xuất JSON thủ công (dự phòng)

1. Mở dự án. Nhập tư liệu và kéo clip vào timeline nếu muốn cắt/chỉnh video.
   Timeline trống vẫn có thể tạo tiêu đề; không có video để AI phân tích.
2. Mở **Trợ lý chỉnh sửa AI → Chat web tự động → Nâng cao: nhập / xuất JSON thủ công**.
3. Bấm **Xuất context**. File chỉ chứa ID/thời gian và schema
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

## Tự chỉnh timeline qua MCP local

1. Trong AI Edit, chọn **Chat web tự động → Nâng cao: dùng MCP client riêng → Bật MCP local**.
2. Bấm **Lưu cấu hình MCP cho client bên ngoài**. File JSON chứa command/args
   của adapter và đường dẫn connection `.dpapi`, không chứa API key hay token rõ.
3. Ghép mục `mcpServers.concat` vào cấu hình MCP hiện có của client local tương thích.
   Không ghi đè toàn bộ settings nếu bạn đã có cấu hình khác. Khởi động lại
   client, kiểm tra danh sách MCP có ba tool dưới đây. ChatGPT Desktop cần một
   plugin/client local hỗ trợ MCP stdio; chế độ ChatGPT web thường không thể gọi
   trực tiếp tiến trình MCP local này.
4. Đăng nhập/quyền model thuộc client bên ngoài. Gemini CLI hỗ trợ đăng nhập
   Google cho các tài khoản phù hợp, không bắt buộc API key. Không sao chép cookie
   trình duyệt hoặc token tài khoản vào Concat.
5. Trong chat của client, yêu cầu AI dùng `concat_get_context`, lập kế hoạch
   rồi gọi `concat_propose_edit`. Concat kiểm tra scope/schema/revision trước
   khi ghi thay đổi native vào timeline. MCP bật tự áp dụng mặc định; tắt nút
   **AI tự áp dụng sau kiểm tra** nếu muốn xem trước và duyệt thủ công.

| Tool | Quyền |
| --- | --- |
| `concat_get_context` | Đọc timeline đang mở + scope + schema |
| `concat_get_status` | Đọc trạng thái bản nháp |
| `concat_propose_edit` | Áp dụng plan đã kiểm tra nếu tự áp dụng bật; nếu tắt thì chỉ tạo bản nháp |

Không có tool approve, shell, chạy command tùy ý hay đọc file tùy ý. MCP context
và JSON nhập tay cùng dùng `EditPlan::parse/compile`, Session/Editor hiện có và
Undo của Concat — không có timeline engine thứ hai. Auto-apply không bỏ qua
kiểm tra scope, schema hay revision.

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
pnpm test:browser
cd D:\capcutpolot\concat\src
cargo test -p concat-studio --profile quick
cargo run -p concat-agent --profile quick --example mcp-native-smoke
cargo run -p concat-agent --profile quick --example browser-native-smoke
```

Native Windows build dùng `scripts/windows-studio.ps1` để nhận đủ FFmpeg/ORT/
Sherpa và scratch trên D. `windows-package.ps1` bắt buộc SDK runtime MCP, copy
Node và dependency hoisted, bỏ `.pnpm`/junction và không đóng gói connection
user. Một EXE cũ chưa rebuild không có các nút/logic mới.

## Tài liệu chính thức đã đối chiếu

- [MCP SDK: stdio, tools và quyền](https://ts.sdk.modelcontextprotocol.io/server)
- [Gemini CLI: cấu hình MCP](https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/mcp-server.md)
- [Gemini CLI: đăng nhập Google hoặc API key](https://geminicli.com/docs/get-started/authentication/)

## Bằng chứng browser adapter (2026-10-18)

- `pnpm test`: 12/12, gồm tạo tab khi Edge chưa có trang ban đầu, correlation,
  trả lời câu hỏi về project mà không sửa timeline, từ chối scope sai, khóa phản hồi lặp và
  không phát lại thao tác có kết quả không xác định.
- `pnpm test:browser`: 8/8 trên Edge thật/CDP, gồm giao diện desktop, textarea
  mobile/guest, cửa sổ hiển thị, click/Enter, câu hỏi thông tin và chuyển trang
  trong lúc chờ phản hồi. HTTP của ChatGPT/Gemini/Claude
  được đáp bằng HTML fixture cục bộ; kiểm tra Send, context, đọc phản hồi và
  đúng một lần handoff qua TCP. Không phải kiểm thử tài khoản AI thật.
- Đã tái hiện lỗi “Không tìm thấy trang chat của Concat” ở cửa sổ hiển thị:
  Edge app mode có thể chưa tạo trang `about:blank` lúc CDP sẵn sàng. Bỏ điều
  kiện URL cố định, chủ động tạo target khi chưa có page; regression trước fix
  thất bại đúng thông báo này và sau fix đi hết Send/context/propose. Sửa ở
  adapter JavaScript; bản dev đọc lại adapter khi bấm mở chat, không cần rebuild Rust.
- Đối chiếu trực tiếp DOM của tab ChatGPT guest `/uc/...` đang chạy cho thấy
  `textarea#mobile-composer-prompt`, nút `aria-label="Send message"` và
  `li[data-message-role="assistant"][data-message-complete]`. Đã nối đủ cả ba,
  chỉ báo connected sau khi nhận diện composer/send và đọc được context native.
- `live-chat-smoke.mjs --allow-live-guest`: một câu hỏi trên trang ChatGPT thật,
  bằng profile khách mới và context tổng hợp, đã nhận message envelope đúng
  requestId/scope và trả lời đúng project mẫu có 2 clip/15 giây. Không đăng nhập,
  không truyền tư liệu/project thật, không gọi API trả phí và không chỉnh timeline.
  Sau sửa JS: ngắt kết nối, chờ cửa sổ chat cũ đóng rồi mở lại để nạp adapter mới.
- `browser-native-smoke`: Edge DOM → DPAPI/TCP → native EditPlan → tự commit
  trim đầu 2 giây và tiêu đề “Xin chào” ở giây 1–4. Kiểm tra clip sau vẫn đúng
  thứ tự, từ chối scope cũ và một Undo khôi phục toàn bộ project fixture.
- `windows-studio.ps1 -CheckOnly`: qua lượt cuối 2m26s; có cảnh báo cũ, không
  có lỗi Rust/Slint. Build/test/scratch trên D. Script dọn khoảng 4,14 GB
  incremental cũ qua hai lượt kiểm tra. Một số fixture cũ trong `.tmp` còn lại
  vì thao tác dọn riêng bị kiểm tra tự động của công cụ chặn.
- Chưa kiểm chứng một lượt phản hồi từ tài khoản AI thật hoặc đóng gói EXE
  mới cho đợt browser adapter này. Bản dev cần khởi động lại để nhận UI/Rust
  mới; EXE cũ chưa có nút kết nối mới.

## Bằng chứng kiểm thử trước browser adapter (2026-10-17)

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

## Tự kiểm tra chất lượng bàn giao cũ (2026-10-17)

Mục dưới ghi nhận phiên bản MCP/JSON trước khi bổ sung browser adapter; cách
dùng hiện tại và bằng chứng mới nằm ở các mục ngày 2026-10-18 phía trên.

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
