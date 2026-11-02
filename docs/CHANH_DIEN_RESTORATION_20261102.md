# Chánh Điện Tịnh Tâm — restoration and integration

Reference: the user's image B controls the presentation. Work is confined to
Concat; no source from the Artcraft project is used. The existing local LIVE
service was identified at `D:\chanh-dien-online` from the running Node process.
Its MIT-licensed source/assets (commit `17feae0905f4892fe09656d19c09fc3bb8713738`)
are now bundled under `integrations/temple-live`; the original directory is preserved.

## Checkpoint and findings before changes

- Git checkpoint: `codex/restore-temple-ui-20261102` (points to the starting HEAD).
  The starting uncommitted patch is saved in `.runtime/temple-ui/before.patch`.
- The 156 deleted tracked documentation/contract files were restored at the
  user's request before this work. The workflow JSON is a compile-time input,
  so deleting the docs directory also breaks the application build.
- The original server supports `POST /api/control` with `{cmd: ...}` and
  WebSocket `/ws?role=control` with `{t:'ctl', cmd: ...}`. Its newer `studio.html`
  incorrectly sent `a:simChat/simGift/demo` and parsed a nested TikTok state.
  The native controller now uses the canonical protocol; the bundled server
  retains HTTP and accepts the web aliases for compatibility.
- `StudioLivePage` used a static temple photograph, fabricated timer/visitor
  labels and a hardcoded ready indicator. It did not render the LIVE overlay.
- Audio switches reused one-shot commands; BPM, capacity and Edge voice were
  labels instead of controls. Connection errors were previously swallowed.
- The native extension product cards were fixed examples whose buttons sent
  simulated chat. The actual product library already existed in Concat.
- `LegacyLivePage`, with the existing native TikTok connector, avatar motion,
  AI replies, products, voices and Kedi controls, was present but unreachable
  from the replacement page.

## Inventory and preservation map

| Module | UI / implementation | Service or transport | Before | Restoration / verification |
|---|---|---|---|---|
| Chánh Điện TikTok connection | `studio-live.slint`, `studio_temple.rs` | existing LIVE service, `/ws?role=control` | one-shot HTTP actions; static status | native explicit connect/disconnect, real state and error logs |
| Scene participants | same, `TempleViewerRow` | state/join/leave/skin/gift messages | placeholder count/list | real participants and demo markers; scene count is not represented as the number of real TikTok viewers |
| Simulation | same, `temple_live.rs` | chat / like / gift / fill / bots / clear | incomplete preview/state bridge | canonical WS commands; simulation identified in UI; actual Rust payloads exercised against bundled server |
| Edge TTS / music / percussion | same | settings / mo / bell / chant / say / stop | switches and static voice/BPM labels | real controls, debounced settings, server errors shown in log; no automatic retry of writes |
| LIVE preview | `studio_temple.rs` preview host | existing `/live/overlay.html` | static image and fabricated overlay | persistent unprivileged WebView2 of original dynamic overlay; checks actual 1080×1920 canvas |
| OBS / external studio | same | configured origin + `/live/overlay.html`, `/live/studio.html` | fixed port repeated in handlers | one configured origin, copy/open remain explicit user actions |
| Avatar / character creation | `PromaxAvatarPicker`, `studio-promax-avatars.slint`, `studio_opentalking.rs`, `studio_promax_avatars.rs` | existing avatar APIs, motion and lipsync services | fake activation in temple tab; real modules elsewhere | real picker and original creation route retained, skin controls use service protocol |
| Product library / cart | `StudioProductsPanel`, `studio_products.rs`, `concat-studio/products.rs` | existing persistent product repository | fixed sample cards in temple | real library CRUD/select/save and explicit overlay pin / TTS |
| Scripts | temple script input + original `StudioScripts` / `ScriptLibrary` route | existing script service, LIVE say/stop | simulated-chat buttons | original script editor/library reachable, actual Edge TTS for temple text |
| Native TikTok / AI / LiveBrain | `LegacyLivePage`, `studio_live.rs`, `studio_kedi_live.rs`, `concat-studio/live_brain.rs` | existing validated native runtime and subscriptions | real component unreachable; hardcoded ready label | original controls exposed in expansion tab; engine state bound to actual globals |
| Native speech / sample voice / video output | existing LIVE, OpenTalking, director and lipsync modules | existing playback and providers | retained native implementations | preserved through the native LIVE expansion and original avatar page |
| Film / Factory / Editor / downloads / publish / settings / MCP | existing `StudioRoute`, native services and app | unchanged endpoints and executors | existing Concat modules | LIVE-specific container branch; other routes keep their prior layout |

The temple service and the existing native TikTok/AI runtime are distinct
existing owners. The UI does not claim the native engine is processing temple
comments merely because the temple WebSocket is connected. The native engine's
original validated connector and controls remain accessible. The bundled service
advertises actual bot state in snapshots and events; the native toggle follows
that acknowledgement instead of assuming a queued request succeeded.

## Layout and state

- Scoped native dark palette in `temple-chrome.slint`; no editor/global theme
  override. The LIVE route has a dedicated full-height container, bounded
  controls scrolling and an independently fitted preview.
- Three control columns when at least 840 logical pixels remain for controls;
  two columns from 540 pixels; one below that. Header and extension tabs wrap.
- Preview follows 9:16 and fits available height. Below 1050 logical pixels of
  module width, a popout provides the same original overlay instead of squeezing
  controls below usable widths.
- `StudioLive` remains the owner of inputs, settings, selected tab and history.
  Opening tabs cannot recreate the control client. The worker and preview are
  persistent for the application session, and hide during native dialogs,
  navigation overlays, minimize and editor navigation.
- Log rows update incrementally, with a 500-row / 1800-character-per-row bound.
- Reconnect uses 1–30 second backoff. Queued or uncertain writes are dropped,
  never replayed on reconnect. Opening the page does not start TikTok or a bot.
- Preview audio starts muted and is enabled by an explicit action. Existing
  OBS / popout browser sources remain independent; their audio is controlled
  by the operator.

## Configuration and build

`CONCAT_TEMPLE_URL` optionally selects the LIVE service origin. Default:
`http://127.0.0.1:3088`. All routes and the WS URL derive from this one value.
Local HTTP or HTTPS is accepted; remote services require HTTPS. Credentials,
paths, query strings and fragments in the configured origin are rejected.

Windows builds use the SDK directories already installed in `.deps`, as
documented by `scripts/windows-studio.ps1`. A raw `cargo check` without those
environment variables incorrectly reports missing FFmpeg/Vcpkg.

`scripts/stage-temple-runtime.ps1` stages the actual runtime/assets, locked Node
dependencies and Node executable/license into `resources/` beside Concat. The
normal Windows build invokes this step. Prepare dependencies with `npm ci
--ignore-scripts` in `integrations/temple-live/live`. Node is sourced from
`.deps/temple-live-node` (the installed Node executable and its license).
Node của Chánh Điện được đặt riêng tại `resources/temple-live/node`, tránh thay
đổi runtime Node của AI/MCP hiện có. Cả nhánh build release và Windows package
đều stage component này. Build Windows mặc định một job và kiểm tra còn 4 GB
trống; người dùng vẫn có thể đặt `CARGO_BUILD_JOBS` khi máy đủ tài nguyên.

On opening LIVE, `temple_runtime.rs` reuses a healthy service or starts the
bundled loopback service. It never connects TikTok automatically. The service
survives closing Concat so OBS can keep running. Settings/pinned product and
elapsed-time origin persist separately from source. Optional overrides:
`CONCAT_TEMPLE_SERVER`, `CONCAT_TEMPLE_NODE`, `CONCAT_TEMPLE_DATA_DIR`.

TTS keeps the original Edge provider, adds cancellation, a 15-second deadline
per attempt, at most one retry for transient errors, stream/client cleanup and
a bounded private audio cache. No provider credentials or development accounts
were copied. Disconnecting TikTok invalidates pending connections and stale
event handlers.

## Verification evidence

Evidence and logs are written under `.runtime/temple-ui/`. Results below come
from protocol tests, the native client, Concat checks and visual inspection.
The headless renderer is only evidence for native controls;
the real dynamic preview is verified separately in a running Windows window.

Actual local overlay checked before UI tests: HTTP 200, canvas and stage
1080×1920, connected overlay socket, and sound still waiting for user activation.
No live broadcast or TikTok account connection was started by that check.

Physical OBS capture / Virtual Camera and TikTok account connection require
their actual applications and a live account. Route reachability alone is not
treated as evidence for those steps.

## A. Files đã sửa và lý do

| File trong Concat | Thay đổi |
|---|---|
| `src/crates/concat/ui/workspace/studio-live.slint` | Khôi phục ba bảng, header, trạng thái, nhật ký và năm tab; giữ `LegacyLivePage` thật. |
| `src/crates/concat/ui/workspace/temple-chrome.slint` | Bộ điều khiển native nền tối riêng; trường nhập, checkbox, slider, menu chọn và nút có bàn phím. |
| `src/crates/concat/ui/workspace/studio-products.slint` | Thêm cách trình bày tối cho repository sản phẩm có sẵn; giữ CRUD, dirty/uncertain guards và xác nhận xóa. |
| `src/crates/concat/ui/workspace/studio-promax-avatars.slint` | Cho picker có sẵn nhận cách trình bày tối; giữ callback chọn/tải/hoạt ảnh. |
| `src/crates/concat/ui/workspace/studio-pane.slint` | Container riêng cho LIVE; nối đường dẫn quản lý avatar/kịch bản. |
| `src/crates/concat/ui/workspace/pane.slint` | Cho nhật ký cuộn xuống cuối qua API của PaneScroll. |
| `src/crates/concat/ui/start.slint` | Dùng viewport thật, bỏ lớp cuộn lồng của LIVE, quản lý ẩn preview khi đổi trang. |
| `src/crates/concat/ui/app.slint` | Export model người xem; ẩn native child khi dialog/editor che trang. |
| `src/crates/concat/src/studio_temple.rs` | Controller thật: một WS, model sự kiện, settings, clipboard/open, host preview WebView2, IPC âm thanh tối thiểu. |
| `src/crates/concat/src/studio_live.rs` | Bỏ bridge HTTP Chánh Điện cũ; giữ toàn bộ executor native LIVE/AI/avatar/video. |
| `src/crates/concat/src/lib.rs` | Giữ controller sống suốt phiên ứng dụng. |
| `src/crates/concat/Cargo.toml` | Bật Windows Shell API để mở URL trên worker. |
| `src/crates/concat-studio/src/temple_live.rs` | Schema lệnh gốc, kết nối WS có giới hạn và backoff; ba test giao thức/socket. |
| `src/crates/concat-studio/src/temple_runtime.rs` | Khởi động/reuse runtime đi kèm; xử lý đường dẫn Windows; không tự kết nối tài khoản. |
| `src/crates/concat-studio/src/lib.rs` | Export hai adapter mới. |
| `src/crates/concat-studio/src/download_browser.rs` | Sửa hai phép kiểm tra host chứa chuỗi, được phát hiện bởi test cũ; chỉ nhận domain/subdomain TikTok và Douyin thật. |
| `src/crates/concat-studio/examples/temple_control_contract.rs` | Xuất payload từ chính hàm native để test Node nhận đúng lệnh. |
| `src/crates/concat-studio/examples/temple_runtime_smoke.rs` | Kiểm tra start/reuse bằng runtime thật ở cổng kiểm thử. |
| `integrations/temple-live/live/server.mjs` | Runtime gốc đi kèm; bổ sung lệnh còn thiếu, lưu settings, trạng thái bot, đánh dấu mô phỏng và hủy kết nối cũ. |
| `integrations/temple-live/live/tts-service.mjs` | Edge TTS có deadline, cleanup, retry giới hạn và hủy khi dừng. |
| `integrations/temple-live/live/overlay.html` | Giữ canvas/animation gốc; thêm ghim sản phẩm, dừng hàng đợi giọng, xác nhận bật âm. |
| `integrations/temple-live/live/studio.html` | Sửa giao thức của web studio gốc, responsive, dữ liệu realtime, bỏ sản phẩm/avatar giả. |
| `integrations/temple-live/live/control.html` | Giữ bảng điều khiển gốc và giao thức cũ để tương thích. |
| `integrations/temple-live/live/package.json`, `package-lock.json` | Giữ dependency khóa phiên bản gốc; thêm lệnh test. |
| `integrations/temple-live/live/test/runtime.test.mjs`, `tts.test.mjs`, `obs-smoke.mjs` | Kiểm thử server thật, lifecycle TTS và OBS portable thật. |
| `integrations/temple-live/assets/*`, `LICENSE`, `README.md`, `.gitignore` | Giữ nguyên tranh, sprite, nhạc và giấy phép MIT của component gốc; loại dữ liệu tài khoản/cache khỏi source. |
| `scripts/stage-temple-runtime.ps1` | Đóng runtime/assets/dependencies và Node riêng vào `resources/temple-live`; không ghi đè Node của module khác. |
| `scripts/windows-studio.ps1`, `windows-package.ps1` | Stage runtime khi build debug/release và khi đóng gói. |
| `scripts/temple-ui-regression.py` | Kiểm thử thao tác UIA trong cửa sổ Concat thật, chỉ mô phỏng ở server riêng 3199. |
| `tools/live-studio-preview/{Cargo.toml,Cargo.lock,preview.slint,src/main.rs}` | Harness render native; chế độ cửa sổ dùng chính controller production. |
| `tools/studio-preview/preview.slint` | Theo container LIVE mới. |
| `.ai-style-rules.md` | Ghi rõ ngoại lệ chỉ dùng WebView cho overlay động gốc; các điều khiển vẫn native. |

156 file tracked bị xóa trước đó đã được khôi phục nguyên bản, không tái tạo nội dung giả.

## B. Giao diện

- Khôi phục nền navy, tiêu đề vàng, trạng thái nhỏ và các nút trên thanh đầu.
- Ba bảng ngang khi đủ chỗ; hai/một cột và cuộn nội dung khi cửa sổ hẹp.
- Trường username cùng hàng với Kết nối/Ngắt; nhãn và nút được wrap thay vì đè nhau.
- Preview dùng overlay thật, đúng stage/canvas 1080×1920; thay đổi chiều cao dựa trên việc header một hay hai hàng.
- Nhật ký mặc định nằm dưới ba bảng; các tab mở component thật.
- Theme tối chỉ áp dụng Chánh Điện. Các trang khác và `LegacyLivePage` giữ theme của ứng dụng.

## C. Chức năng được giữ lại

Các luồng WS/HTTP, bình luận, quà, tim, presets, bot, settings, Edge voice,
nhạc/mõ/chuông, say/stop và ghim sản phẩm đã được kiểm tra qua runtime riêng.
UIA đã thao tác trực tiếp các nút native; không coi callback tồn tại là test đạt.

Smoke ứng dụng đầy đủ đã chạy luồng native LIVE enable/start/comment/pause/
resume/takeover/stop trong fixture, và cầu nối đăng nhập AI ba provider.
Các trang Làm phim AI, Kedi Factory, AI Control, Danh sách video và quay lại LIVE
đã được mở/chụp ảnh thật. Đây là kiểm tra điều hướng/giao diện; không chứng minh
đã sinh video hoặc đăng bài qua tài khoản thật.

## D. Tích hợp mở rộng

Avatar dùng `PromaxAvatarPicker` và route tạo nhân vật hiện có. Sản phẩm dùng
`StudioProducts`/repository hiện có, không có danh mục mẫu. Kịch bản giữ thư viện
Concat và gửi say/stop tới Edge TTS thật. Tab Điều phối LIVE & LiveBrain mở lại
`LegacyLivePage` cùng các service AI/voice/video đang có. Nhãn engine lấy từ
runtime gốc, không hiển thị sẵn sàng khi chưa chạy.

Trong hồ sơ QA chưa đăng nhập, API avatar/sản phẩm vẫn yêu cầu xác thực. Không
chép token từ hồ sơ phát triển và không thay kết quả API bằng dữ liệu mẫu.

## E. Kiểm thử

Artifacts: `.runtime/temple-ui/`.

- `cargo check -p concat --lib`: đạt trước khi kiểm tra desktop.
- `cargo build -p concat --bin concat`: build ứng dụng đầy đủ đã đạt; log bản giao cuối nằm trong `cargo-build-delivery.log`.
- `cargo test -p concat-studio --lib temple_live`: 3/3 đạt; socket fixture thật, không tự gửi lệnh khi kết nối.
- `cargo test -p concat-studio --lib`: 322 đạt, 0 lỗi, 10 ignored (cần dịch vụ/provider ngoài). Lượt đầu phát hiện test lọc hostname TikTok lỗi; đã sửa điều kiện host và chạy lại toàn bộ.
- `cargo test -p concat-api --profile quick`: 22/22 đạt với PATH DLL FFmpeg/ONNX/Sherpa; gồm tạo/sửa/lưu/mở project, frame, export, giới hạn đường dẫn. Lượt đầu thiếu DLL ở test process; cấu hình PATH đúng rồi chạy lại, không sửa API để né lỗi.
- `cargo run -p concat-studio --example temple_control_contract`: payload được dùng nguyên bản trong test Node.
- `npm test` tại `integrations/temple-live/live`: 9/9 đạt, không skip; gồm HTTP, WS, state, assets, persistence, payload native, dữ liệu TikTok hiện hành và lifecycle TTS.
- `node --test scripts/native-build-resource-regression.test.mjs`: 3/3 đạt; các script PowerShell thay đổi đều parse không lỗi.
- Edge TTS online: tạo file MP3 36.576 byte, HTTP 200, voice Nam Minh; không sử dụng âm thanh giả.
- Runtime smoke: cold start ở cổng 3198 đạt; lần gọi thứ hai reuse; Node vẫn chạy sau khi tiến trình kiểm tra kết thúc.
- Desktop UIA: nhập text, chat/like, quà 1+30+199 = 230, presets 15/36/64/100, clear, bot on/off nhận acknowledgement, đổi nhạc, bật AudioContext rồi tắt âm, chuyển đủ năm tab và giữ input.
- Cửa sổ đã chụp: 1920×1080, 1632×1000, 1440×900, 1366×768, 960×900. Kích thước tính theo pixel vật lý của cửa sổ; hồ sơ QA đặt scale 1.
- Sau nhiều lần đổi tab/route, tiến trình native chỉ giữ một kết nối điều khiển tới 3199.
- Ngắt server QA thật: native hiện “Đang nối lại”, nút gửi bị khóa, giữ tên đã nhập. Khởi động lại server: tự kết nối tốt, nút gửi bật lại, tên giữ nguyên, số người 0 và chỉ một socket điều khiển; không phát lại lệnh cũ. Xem `desktop-final/reconnect-{offline,online}.json`.
- OBS 32.2.2 portable riêng: Browser Source thật 1080×1920, 30 FPS; source nhận thay đổi cảnh; Virtual Camera start/stop đạt. Streaming=false, recording=false. `obs-evidence/obs-results.json` và hai ảnh PNG là output thực của OBS.

OBS portable theo [tài liệu OBS](https://obsproject.com/kb/portable-mode);
kiểm thử RPC theo [giao thức chính thức obs-websocket](https://github.com/obsproject/obs-websocket/blob/master/docs/generated/protocol.md).
Cấu hình OBS của người dùng không được thay đổi; port kiểm thử đã đóng khi OBS QA thoát.

Server chính 3088 đã chuyển sang runtime đã stage trong Concat sau khi xác nhận
phòng cũ idle, username trống và số người bằng 0. Snapshot trước chuyển nằm ở
`legacy-state.json`; kiểm tra sau chuyển xác nhận giữ nguyên startedAt, voice và
moBpm. Overlay đang mở tự nối lại WS thành công, vẫn dùng URL cũ. Thư mục nguồn
`D:\chanh-dien-online` không bị sửa. Đây là chuyển runtime cục bộ, không khởi
động tài khoản hoặc buổi phát.

## F. So sánh ảnh

Trước sửa: ảnh A do bạn cung cấp và checkpoint/patch trước sửa. Đối chiếu thiết kế:
ảnh B do bạn cung cấp. Ảnh sau cuối nằm tại `desktop-final/native-1920x1080.png` và các kích
thước tương ứng; tab thật có ảnh `desktop-final/native-tab-*.png`. Không sử dụng ảnh
headless để chứng minh preview động.

Khác biệt còn lại so với B: giữ sidebar/topbar native Concat; thêm hàng tab mở
rộng, nút clear/chuông; cỡ chữ theo DPI Windows. Khi thiếu chỗ, bảng xuống hàng
và cuộn; cửa sổ rất hẹp mở preview qua cửa sổ phát riêng. Canvas, tranh chánh
điện, ba thầy, đồng hồ và danh sách người lấy từ overlay gốc.

## G. Hạng mục cần quyền truy cập thực tế / giới hạn

Ảnh desktop cuối ở DPI hệ thống được hoãn theo lựa chọn “Chụp ảnh sau” của người dùng; các ảnh UIA ở phần F vẫn là ảnh thực đã kiểm tra trước đó.

- Kênh đầu tiên `@senkai896` không LIVE tại thời điểm kiểm tra; sau đó đã xác minh thành công với `@gocanyen.live`: trong lượt quan sát 20 giây nhận 2 bình luận, 3 sự kiện quà và 4 sự kiện tim thật. Phòng `7703337019411270421`, số người thật tăng từ 25 lên 29. Đây là đọc sự kiện công khai, không gửi bình luận hoặc khởi động phát sóng.
- Chưa xác minh CRUD avatar/sản phẩm, sinh video/model AI, đăng/lên lịch bằng tài khoản thật trong hồ sơ đã đăng nhập; UI và service hiện hữu vẫn được giữ.
- OBS Browser Source/Virtual Camera đã kiểm tra trực tiếp; chưa xác minh ứng dụng TikTok LIVE Studio nhận Virtual Camera hoặc một buổi phát công khai.
- OBS Browser Source đã đo được âm thanh: peak 0.4823457897, 330 sự kiện audio meter. Ảnh nguồn thực tế xác nhận không còn nút nhắc bật âm thanh. Chưa đo âm thanh ở đầu nhận phát sóng.
- Hồ sơ QA ghi một cảnh báo GPU `Surface::configure` lúc khởi tạo cửa sổ trước khi mở LIVE; ứng dụng tiếp tục chạy. Không coi việc UIA đạt là bằng chứng driver GPU không có vấn đề.
- OBS portable báo một allocation còn lại lúc thoát cả ở baseline cảnh trống và sau test Browser Source. Không quy lỗi này cho module Chánh Điện, cũng không tuyên bố đã chứng minh không có memory leak dài hạn.

### Bản giao cuối và sửa nút âm thanh

`run-concat.bat` đã trỏ đến bản qua kiểm tra version/smoke, SHA256 `54ddb0f36f63c16f4a2aa21c79a8a1c7e89603d7a72db99b9e8c7bd7d345e0ef`. Receipt ở `.runtime/kedi-current-build.json`; có receipt trước để quay lại.

Đã thêm adapter `tiktok-events.mjs` để đọc cả payload hiện hành (`content`, `id`, `displayId`, `giftDetails`) và payload cũ. Kết nối công khai tắt truy vấn metadata quà mở rộng và room info tùy chọn để tránh yêu cầu gói trả phí không cần thiết cho nhận sự kiện.

Nút “Bấm để bật âm thanh” nay ẩn khi AudioContext thực sự chạy và không xuất hiện lại sau lần kích hoạt thành công. OBS tự kích hoạt trên trang overlay; preview vẫn theo lựa chọn nghe thử. Âm lượng giữ nguyên. Kiểm tra OBS thực tế tại `.runtime/temple-ui/obs-audio-evidence/obs-results.json` và `obs-browser-source.png` xác nhận có tín hiệu âm thanh và không còn nút nhắc che hình. Nguồn Browser Source đang mở cần làm mới trang để nhận mã mới.

Các server đang phục vụ nguồn của người dùng tại 3088 và 3197 được giữ hoạt động; không đóng OBS của người dùng. OBS portable riêng phục vụ kiểm thử đã thoát và tắt cổng điều khiển kiểm thử.
