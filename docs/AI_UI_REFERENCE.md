# AI workspace presentation reference

Reference: [Kool-Cool/cozycat-ui](https://github.com/Kool-Cool/cozycat-ui),
commit `abf3a55cdf915fcb6f0238f3c142b45e5bcc3a54` (inspected 2026-10-28).
License: MIT, copyright 2025 Kool-Cool. Full notice is retained in
`THIRD_PARTY_NOTICES.md` and the native adapted component credits the source.

The inspected repository implements accordion variants and CSS/Framer Motion
animations. Its README describes a larger component library than the actual
component tree. There is no AI planning/browser automation engine to import.
The linked Vercel deployment returned HTTP 410 during inspection; this adaptation
uses the pinned source rather than claiming to have reviewed a working live demo.

Applied to the native AI workspace:

- Warm, quiet surfaces and 12–16 px rounded panels, retaining Kedi's bundled
  Vietnamese font, shared buttons, focus behavior and primary brand.
- A bounded 1120 px work area, with a compact request composer beside the actual
  plan on wider windows and stacked panels on narrow windows.
- Real workflow blocks are accordions. Active/waiting steps open automatically;
  completed and queued steps remain compact and can be opened with keyboard or mouse.
- Inputs, task options, history and technical data are foldouts; JSON is not part
  of the default user flow. Action buttons are compact and contextual.
- Native 220 ms height/hover transitions honor both app and system reduced-motion
  preferences. Content height follows the actual body, without the upstream
  fixed maximum height that could clip long content.

`control-chrome.slint` contains the reusable presentation. `studio-ai-control.slint`
composes it around the existing durable workflow callbacks. The runtime's web
adapter, project revision checks, approval policy and data remain unchanged.
There are no example products, invented progress steps or fake execution results
in the product UI. Existing video deletion uses the separate centered modal.

The full Cozy presentation build completed successfully. The latest revision
places the same page in an independent native `ControlWindow` beside the main
application, forwarding callbacks to the existing owner. Its narrow panel scrolls
vertically. Physical monitor coordinates preserve placement at 150% DPI.
The 2026-10-29 review further changes this to request/plan pages: Submit opens
the plan and Back preserves the request. The small crate's build adapter exports
the existing composer and monitor from a generated copy; it does not fork their
source or require rebuilding the large main UI for this navigation change.
See `AI_TEST_RESULTS.md` for current build/hash and separate-window GUI checks;
these are not authenticated provider acceptance tests.

## Verification

- Actual Slint workspace and page compile: `.runtime/ai-control-cozy-review-ui.log`
  and `.runtime/ai-control-cozy-slint-check.log`.
- Native visual review through Computer Use at 1100 px and 520 px using existing
  workflow `control-bbda867c2650232afaf750dcd53bc253`, read-only. Wide view shows
  composer and plan side by side; narrow view stacks them and scrolls to the plan.
  Mouse opens a completed step; Enter closes it. No provider call, permission
  toggle or workflow write was performed by the review harness.
- Review caught and corrected overlapping accordion header/body, centered progress
  fill and pointer interception by the focus scope. Final observations showed the
  corrected behavior. The transient Computer Use app-access timeout succeeded on
  the documented single recovery attempt.
- Prior workspace executable `concat-ai-control-preview.exe` (20:04 build) passed
  actual waited version/smoke checks, exit 0, Intel Iris Xe / DX12. Those checks
  predate the Cozy presentation revision and are not counted as its full app test.
- Cozy build: `.runtime/ai-control-cozy-native-build.log`, exit 0. Latest separate
  window build: `.runtime/ai-control-dual-native-build.log`, exit 0. An already
  running older app must be restarted to use the published new executable.

Completed review executables, PDBs, generated Rust and failed-link object files
were removed after the visual checks; the small review sources and logs remain.
