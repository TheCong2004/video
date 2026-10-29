# Hotclip desktop section

The large **HOTCLIP → Hotclip** sidebar entry hosts the original Hotclip Electron
window, not `dev:web` (which uses a mock API). Hotclip starts hidden with normal
Concat startup, keeps running when another section is selected, and becomes a
bounded native child when selected. The native editor and native shorts tool are
unchanged; Hotclip projects and Concat timelines remain separate.

## Runtime

The prepared checkout is `../hotclip` relative to the Concat project. Discovery
walks bounded executable ancestors for a `hotclip` folder with the expected
package name, compiled main/preload/renderer, and Electron executable. Deployment
may set `CONCAT_HOTCLIP_ROOT` to an absolute prepared runtime directory. It does
not search arbitrary EXEs in PATH, invoke a PowerShell window, install packages,
download models or start inference during ordinary Concat launch.

Preparation in the Hotclip checkout uses its lockfile:

```powershell
pnpm install --frozen-lockfile
node node_modules/electron/install.js
pnpm build
```

The current integration uses the original checkout without copying its large
dependencies. Keep the Hotclip folder next to Concat. Standalone Hotclip launch
remains supported and retains its own projects. Concat-owned Hotclip uses
`LOCALAPPDATA/app.concat.editor/hotclip`; portable Concat uses its portable area.
It does not copy browser cookies, Concat credentials or provider keys. Configure
Hotclip's own AI service/models within Hotclip when needed.

## Lifecycle and boundary

Concat spawns the exact prepared Electron runtime without a shell, with a small
environment allowlist. A private control socket listens only on numeric loopback
and an OS-assigned ephemeral port, protected by a fresh 128-bit nonce. It is not
an HTTP/media/project API. Electron supplies its loaded native HWND; Concat checks
the nonce, owned process PID and actual Win32 HWND ownership before reparenting.
The real preload bridge must exist before readiness is reported. Normal Electron
stdio is not used for lifecycle control because GUI Windows stdin was observed
to terminate prematurely in the first test. Accepted sockets explicitly return
to blocking mode on the worker thread: Windows inherits nonblocking listener
mode, which otherwise drops the later renderer-readiness notification.

The child hides on route changes, host sheets/menus, minimize or editor entry.
Frame bounds are clamped to the host client area and updated using DPI scale.
Shutdown requests normal Electron quit and flushes queued project persistence;
existing inference/export abort handlers run. A Windows kill-on-close Job owns
the child tree and supplies bounded shutdown/crash cleanup, never process-name
or port-wide killing. An active export is cancelled when Concat exits; wait for
exports to finish if you want to retain their current result.

Windows native embedding is the supported platform for this integration. Initial
setup failures are shown honestly with **Mở lại Hotclip**, not a fake ready UI.

## Verification commands

```powershell
# Hotclip checkout: real Electron/preload IPC, source probe, short export and re-probe.
node tools/concat-embed-smoke.mjs D:/capcutpolot/concat/.runtime/live-director-av-fixture.mp4
# Concat: actual launcher, autostart, HWND, bounds, menus, resize, minimize and exit.
.\scripts\hotclip-native-smoke.ps1
node --test scripts/hotclip-integration-regression.test.mjs
```

Smoke uses isolated user data and a synthetic source; no ASR models, paid AI or
social posting is invoked. It does not measure highlight quality, transcription
accuracy or GPU inference speed. Any full Hotclip model workflow still needs its
normal model/provider configuration.

## Verified delivery, 2026-10-21

- Windows Concat build passed. Real native launcher smoke passed autostart while
  hidden, actual preload/media IPC, owned child bounds, real menu callback/hide,
  resize, minimize/restore, route changes and owned process absence after exit.
  Receipt: `.runtime/hotclip-qa-41327fe38682445ea4b3db49f57a1e8b`.
- Separate Electron deep smoke passed actual source probe, short MP4 export and
  output probe through the original renderer/preload/main IPC, then normal quit.
  Native startup QA probes media only; export QA runs separately, so thumbnail/
  export work cannot incorrectly count against normal startup readiness timeout.
- NexPulse actual native smoke and voice/A-V director smoke passed on the same
  final executable. Receipts: `.runtime/nexpulse-qa-bf68f194be15422198d5a14ca704bfe7`
  and `.runtime/director-smoke-eeb7380226ec45c7804f0087c7a1fc06`.
- 70 Concat Node regressions passed. 144 Studio Rust tests passed,6 ignored;
  the explicit real Electron process/handshake/quit integration test also passed.
- Hotclip typecheck/build and all 3 new host-protocol tests passed. Full Hotclip
  Vitest:1282 passed,1 failed,1 skipped. The independently reproduced failure is
  `cover-quality.integration.test.ts`, expecting at least two black/white frame
  rejections but observing one. The cover algorithm/test were not changed by
  this integration; do not interpret the test suite as wholly green.
- Installed `src/target/debug/concat.exe`,237729792 bytes; SHA256:
  `159687A1B247BBE1DC957DDE2086C198D07ABAE56A696803A0674D0D0B0A8EB3`.
  Installed hash matches the tested candidate; retained runtime DLLs match.
- All native checks use isolated data. No user project was modified, model
  weights downloaded, paid AI called, social content posted, or Git changes pushed.
