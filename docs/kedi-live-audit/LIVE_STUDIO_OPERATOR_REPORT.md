# Live Studio operator integration — 2026-10-30

Baseline: `main`, `69caa109d156b3bfb97a6740bcfee822911f0227`.
Existing uncommitted Campaign Manager changes were retained.

## Scope and owners

The existing Rust/Slint LiveBrain, scheduler, product session, speech player,
checkpoint worker, scene preflight and comment connector remain the owners.
There is no new web frontend, media server, copied account session or publisher.

| Spec surface | Actual implementation |
| --- | --- |
| Session | `studio-live.slint`: enable local simulation or configured local avatar, lifecycle and connector controls |
| Avatar/Motion | Existing avatar catalogs, Promax picker, motion player and three actual director roles; manifest scenes retain their existing whitelist |
| Script | Manual speech and product introduction through the existing serialized runtime queue |
| Products | Current tenant/store-scoped snapshot preview, explicit JSON import, revision check and existing policy gate |
| Comments | Existing connector event model and separately labelled local simulation |
| Audio/Output | Existing voice audition, lipsync health, decoder frames, subtitles and clean output window for OBS/LIVE Studio |
| Health | Actual runtime status: queue, speech lifecycle, scene, failures, dropped events, planning percentile and confirmed pin state |

Files: `src/crates/concat/ui/workspace/studio-live.slint`, `ui/start.slint`,
`src/crates/concat/src/studio_kedi_live.rs`, `src/lib.rs`,
`src/crates/concat-studio/src/kedi_live.rs`, and its integration tests.
The old operator page remains reachable through “Điều khiển cũ”.

## Gates and rollback

The page is discoverable without enabling runtime features. Runtime remains off
until the operator selects a mode or explicitly configures a process feature flag.
“Bật thử local” enables simulation only: no GPU, speech synthesis, account calls,
commerce fetch or platform writes. Its mock product is labelled as mock.
“Bật avatar / giọng local” requires explicit tenant/store and avatar manifest/root
configuration and completes the existing rights/hash/codec preflight before Start.
One mode is selected for the lifetime of the existing runtime owner.

Platform-write permission is never granted by the UI activation button. LiveBrain
Start is a local session start, not a TikTok broadcast start. The clean output
window must be captured by separately configured broadcast software. Facebook,
TikTok pin/send, RTMP/WHIP publishing and commerce transactions are not added here.

Product preview uses the same validity filter as speech; expired or future facts
are hidden. Import is bounded to 64 KB, off the UI thread and scoped by tenant,
store and product revision. A changed session revision rejects a stale import.
An imported file is not evidence of a real Pancake/Fufuni connection. Selecting a
featured SKU never marks it as pinned on a platform.

STOP/Pause/Takeover invalidate runtime generations and stop the speech clock while
leaving the independent video/idle path intact. Unknown writes are never replayed.
The launcher retains the previous verified executable for `-PreviousBuild`.

## Validation artifacts

- `.runtime/live-studio-runtime-tests.log`: runtime suite.
- `.runtime/live-studio-preview-build.log`: software renderer using the actual Slint page.
- `tools/live-studio-preview`: disconnected UI render harness; its images are
  layout evidence, not backend or platform E2E evidence.
- Native `--smoke` with `-LiveStudioSmoke`: isolated-profile callbacks for
  enable/start/simulate/pause/resume/takeover/STOP; no real platform writes.

The complete `concat-studio` test command passed: 319 unit tests and 21
integration tests passed; 11 explicitly external-service tests were ignored.
Early broad runs exposed intermittent WouldBlock/transport failures in the
Windows HTTP fixtures. Accepted fixture sockets now explicitly use blocking
reads with the existing three-second deadline. No production retry or dispatch
behavior was changed. The final complete run has zero failures.

All seven tabs rendered at 360px/dark and 960px/light (14 images); native pointer
events navigated the tabs. An additional running-state layout image is explicitly
labelled as a UI fixture, not a live runtime result. Images were inspected at
360px and 960px. Navigation no longer recurses during width measurement and the
content uses its own scrolling viewport below the operator controls.

Launcher tests: 18 passed, including refusal to run Live Studio smoke against
the existing user profile. Initialization/checkpoint I/O and product-file reads
run off the UI thread, with a single-flight initialization guard.

The published desktop build and native operator smoke receipt are recorded below
after the full desktop build completes.

## Remaining production gates

Real avatar/GPU inference, continuous output/FPS/A-V skew, first audible latency,
two-hour real-time soak, approved platform connector/API and one real storefront
must still be exercised with configured services and authorized access. No such
measurement or production approval is inferred from local tests or render images.
This machine reports Intel Iris Xe, no `nvidia-smi`, no configured approved
avatar manifest/root for this session, and the local lip-sync health endpoint was
not verified. These findings block claiming a real GPU/commerce production pilot.
