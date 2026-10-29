# Embedded Hotclip white viewport

## Reproduction and root cause

On the user's Windows desktop, the installed Concat displayed a completely
white Hotclip child even after React mounted. A separate isolated Electron
diagnostic confirmed six real buttons, the mounted import screen, dark computed
body background, and no reported renderer exceptions. Windows accessibility also
exposed those controls in the white child. The old native smoke only checked
owned process/IPC and the host's visibility flag, not displayed pixels.

Concat changed the HWND visibility directly with SetWindowPos. Electron's own
widget lifecycle never received showInactive/hide. An embedded-only experiment
disabling hardware acceleration did NOT fix the white viewport and was reverted.

## Fix

- Retain synchronous Win32 clipping/hiding and owned-PID validation.
- Queue only fixed show/hide lifecycle commands after validated attachment, over
  the existing private nonce-authenticated socket. Do not block the UI on I/O.
- Coalesce visibility updates in the worker; shutdown takes precedence.
- Electron invokes showInactive (no focus stealing) and invalidates its content
  on show; it invokes hide on hide. The presenter preserves host geometry and
  measures the foreign-parent coordinate translation instead of assuming screen
  position/DPI. Unit tests cover repeated show and positive/negative origins.
  The initial window background matches the dark UI to avoid a white flash.
  Standalone Hotclip's lifecycle remains unchanged.
- No rendering-security settings were relaxed; no new command/eval API exists.

## Verification in this repair

- Reproduced the white viewport in the real installed Concat using Computer Use.
- Compiled the lightweight existing webview preview with the SAME production
  studio_hotclip module, real concat-studio process manager and Hotclip runtime.
- Observed the actual embedded import UI render, then the real source/transcribe
  screen render. This is a native-window screenshot, not capturePage alone.
- Hotclip protocol/geometry tests: 4 pass. TypeScript typecheck/build pass.
- Concat Hotclip/NexPulse source regressions: 7 pass, including new lifecycle test.

## Final verification and installation

- Full native build passed in 20m47s, using the existing build target with one
  job, no window debug symbols and no incremental window cache.
- Actual main Concat EXE was launched in an isolated profile. A native screenshot
  showed the real Hotclip source/transcribe UI rather than white. Maximizing the
  initially oversized QA window showed the complete right edge, project/settings
  buttons and language selector. A native system menu remained open correctly.
- Final Hotclip native smoke passed autostart hidden, real preload/media IPC,
  bounds, host menu hide, resize, minimize/restore, route hide and owned shutdown:
  .runtime/hotclip-qa-0c939fbdb7074c4a88a137f8f3732293.
- NexPulse native regression passed:
  .runtime/nexpulse-qa-3f9cc60e9f5148659310afc3c5dad45a.
- Voice and offline A/V synchronization regression passed:
  .runtime/director-smoke-ed862f6ad45643859cf556523c2184c0.
- Separate real Electron media import/export smoke passed on the final Hotclip
  build; no AI model/provider was used or installed.
- User saved/closed and allowed stopping old Concat PID8792. It had already
  exited when checked. Only owned QA processes were closed/stopped during tests.
- Tested EXE moved over src/target/debug/concat.exe after verifying old baseline,
  app closed, and matching hashes for all five native runtime DLLs. No EXE backup.
  Installed size 237747200 bytes, SHA256:
  D662BCEF4DD559D536193B20CE81811542DAF4A420F0B1D4C6987B8D1C41BCD0.

Limitations: the Computer Use mouse helper reported stale/tiny geometry after
maximization, so not every manual route-click could be exercised reliably. Do
not claim complete mouse E2E coverage: automated native route/menu/minimize
regressions and actual visible pixels were checked separately. Other monitor
scales and multi-monitor dragging were not tested on hardware. Original cover
quality failure from the preceding integration task is outside this repair.

The QA profile .runtime/hotclip-visible-qa-20261021 is isolated from user projects;
keep its auto-saved project if the user interacted with the visible test window.
Temporary geometry logging was removed from final source/runtime; the old QA
JSON contains dimensions only. No cache cleanup or policy-blocked deletion was
attempted. Existing user source changes/projects/media and GPU configuration kept.
