# TikTok LIVE connection check — 2026-10-17

## Chạy bản sửa

Đóng Concat cũ, mở bản mới rồi vào **Live TikTok**. Nhập `@username` của kênh
đang phát, bấm **Kiểm tra phòng**, sau đó **Kết nối Live**. Kết nối nhận bình luận
không yêu cầu model AI hoặc key. Giọng đọc/nhân vật/AI trả lời là các phần riêng;
không cần bật chúng để kiểm tra kết nối. Khi app đang tự nối lại, dùng **Ngắt kết
nối** nếu muốn kết thúc phiên; không tạo thêm một lượt kết nối trùng.

## Scope

Fix Concat's native LIVE integration against the existing ArtCraft connector.
No cloud deployment, account changes, AI requests or chat posting were performed.

## Changes

- Critical UI root cause: `Weak::upgrade()` in Slint returns `None` on a worker
  thread even when the window is alive. The event monitor used it as a liveness
  check and exited on its first iteration, before asynchronous connect finished.
  It now checks event-loop delivery instead. Verified against the actual pinned
  `i-slint-core` API implementation, not an assumption about network failure.
- Native supervisor preserves bounded uppercase machine error codes, discarding
  raw provider messages. UI explains offline, signature and rate-limit failures.
- Room inspection now waits for the matching JSONL request ID and shows the actual
  room result/error. Previously the UI reported that inspection had started but
  never consumed its result until a separate connection monitor happened to run.
- Native monitoring stays alive across `disconnected`, allowing ArtCraft's own
  bounded reconnect logic to deliver later states and events.
  The UI and supervisor also reserve the active session during reconnect, so a
  second Connect cannot replace it or get stuck waiting on an already-active
  connector. Status refresh compares error codes as well as the state name.
- Development discovery includes Concat's staged `.deps` runtime. Distributed
  resource discovery remains self-contained.
- Shared ArtCraft connector source: preserve concrete error subclass when
  `Error.name` is inherited (e.g. `UserOfflineError`); pass session credentials
  inside the SDK's `session` option instead of ineffective top-level properties.
  Rebuilt its dist and staged the same runtime into Concat. No credentials changed.

## Evidence

- Original staged connector and original ArtCraft runtime initially had identical
  SHA256 `0375FBC5C7607E28282A49B2978B91C80997B8F63D91BF12D2FDC1ADA34C78F7`.
  This was not a case of a different connector being substituted.
- Initial read-only probe of the old screenshot's username returned
  `UserOfflineError`. After the user explicitly restarted `@voaioh`, both the
  library and JSONL runtime reported an actual connected WebSocket session.
- Native Rust test `real_live_connects_through_native_supervisor_without_posting`:
  **1 passed**, real `@voaioh` connection through `LiveRuntimeConfig::discover`,
  native process supervisor and protocol parser; owned child stopped afterward.
- Latest native LIVE targeted suite: **5 passed, 2 ignored** (the two network/
  packaged tests require explicit opt-in). Includes handshake, stop/restart and
  safe error propagation. The real-network test was then run explicitly and
  passed again in 5.55 seconds with the final reconnect guard changes.
- ArtCraft controller/protocol tests: **22 passed**.
- ArtCraft built-runtime process smoke: **passed** (malformed/oversized commands
  and stdin close). Its initial run during compilation hit the original 10-second
  startup limit; a subsequent unmodified rerun passed in about 4 seconds.
- `node --test scripts/live-connection-regression.test.mjs`: **3 passed**.
- Packaged **real App callback/event-loop smoke: passed** with the delayed local
  fixture, verifying the formerly broken cross-thread delivery and disconnect.
- Packaged **real TikTok LIVE -> real App UI -> disconnect: passed** with
  `@voaioh`, using the package's Node and connector, isolated app data and no
  source-tree fallback. No posting, TTS or AI was enabled. Logs are under
  `.runtime/live-ui-smoke-2bef66976630447d8f1c24333e31685d`.
- Updated staged entrypoint SHA256:
  `A7D03BFCC392387AC7A8794E9D2AAE52B278613250A34FB73EF81D743CC3C70E`.

Reproduce read-only network checks (requires an actively broadcasting username):

```powershell
node scripts/live-connect-probe.mjs voaioh
$env:CONCAT_LIVE_TEST_USERNAME='voaioh'
cargo test --manifest-path src/Cargo.toml -p concat-studio --profile quick real_live_connects -- --ignored --nocapture
```

The explicit GUI callback smoke is `scripts/live-ui-smoke.ps1 -ExePath
<complete-package>\Concat.exe`. It uses an isolated local fixture with a delayed
connected frame, the real App callbacks/worker/UI event loop, then disconnects.
It requires a `LIVE_UI_SMOKE=PASS` marker from the new executable; an old
executable merely returning startup exit 0 cannot pass this test. It sends no
TikTok messages, makes no AI request and does not use the user's app data.
Adding `-LiveUsername voaioh` explicitly switches it to the actual packaged
TikTok runtime and the public LIVE connection (the channel must be online).

## Current runnable artifact

- Installer: `dist/windows/ConcatStudio-Setup-x64-20261017-110307.exe`,
  242,480,197 bytes; NSIS completed successfully with CRC checking.
  SHA256 `8724852F9A530597E5B3422639AA4E8880F5345151E7B13E26A2979906CC43F7`.
  Authenticode status: **NotSigned** (no signing certificate used).
- `dist/windows/Concat-20261017-110008-042/Concat.exe`
- SHA256 `5D9F4B1FD293F1CD4CB685760A7EE4BFD65CE6FB14A41FCA3BE7D8A859F7FF1F`
- Standard Windows debug build; startup/DLL-loader smoke passed with only the
  system PATH. The native App callback/event-loop smoke passed on this exact file
  with both a delayed fixture and the actual public LIVE session.
- Build scratch stayed on D. Automatic cleanup removed **4.17 GB** of regenerable
  artifacts, preserving the current executable, latest cache, dependencies and
  user projects.

## Limits / evaluation

Connection correctness: 4/5, supported by a real public LIVE connection through
both the native supervisor and the packaged App callback/UI event-loop path.
Completeness: 3/5, because a full mouse-driven GUI click-through,
automatic reconnect against an actual network outage, TTS/avatar rendering and
posting from an authenticated TikTok session are not covered by this result.
No claim of testing those features or of completing the whole four-project port.

An unrelated AI Edit source-regression run returned 11/13: two assertions no
longer match the current browser/automatic-edit source. Those files have changes
outside the LIVE fix; they were not reverted or marked passing. Earlier UI
render checks produced 20 images, but the separate clipboard click fixture
failed; neither is claimed as full GUI end-to-end validation here.
