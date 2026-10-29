# Windows compiler resource failure — 2026-10-23

## Evidence

- Application event 1000 at 17:10:24 reports `rustc.exe` (PID 23852), pinned
  toolchain 1.93.1, terminating with `0xc0000409`. It is not `concat.exe`.
- System Resource-Exhaustion-Detector events at 17:05:08 and 17:10:07 explicitly
  report low virtual memory. The same compiler consumed 26,293,645,312 and
  27,164,798,976 bytes respectively (about 24.5 and 25.3 GiB).
- D: initially had approximately 0.67 GiB free. Main incremental state contained
  5.67 GiB completed and 8.82 GiB interrupted `*-working` snapshots. Logical
  sizes include hard-linked data and must not be equated to physical recovery.

This supports resource exhaustion during compiler execution. The Windows status
name alone does not identify a Concat application buffer bug or a missing DLL.
A prior `cargo check` pass is not a successful native EXE build.

## Changes

- Disable incremental for the generated window crate only. Preserve engine and
  dependency caches, pinned compiler, standard dev target and debug-symbol policy.
- Default launcher Cargo jobs to 1. Refuse compilation below 4 GiB free space;
  RunBuilt remains usable without that compilation-space requirement.
- Add explicit `-PruneMainIncremental` cleanup using existing path, reparse,
  process and exclusive Cargo-lock checks. It removes only own main incremental
  directories, including interrupted sessions; not EXE, libraries or projects.
- `-PruneOwnCache` can also reclaim recent abandoned own `.rcgu.o` intermediates
  while the same no-compiler/lock safeguards hold.

## Verification / current status

- Cleanup completed; 14.50 GiB logical candidates removed, with D: subsequently
  reporting about 10.65 GiB free. Last runnable EXE retained its original time
  and length immediately after cleanup. Deleted caches are regenerable, not
  recoverable through undo.
- Launcher mock integration passed: native DLL resolution, exactly one intended
  Cargo call, and environment restoration.
- Three resource-policy regressions passed; PowerShell syntax parsing passed.
- Full native build with `CARGO_BUILD_JOBS=1`, `CARGO_INCREMENTAL=0` **passed**:
  `scripts/windows-studio.ps1 -Build`, Cargo exit 0, 16m 43s. The native link step
  completed and updated `src/target/debug/concat.exe`. This verifies the EXE
  build, not the provider workflow or GUI interaction end-to-end.
- A temporary monitor is scoped to the owned compiler PID/start time and stops
  only that compiler if free virtual memory drops below 1 GiB. User apps and
  Windows pagefile settings are not altered.
- The monitor finished normally when the owned compiler exited; it did not
  terminate the compiler. Launch without another compilation using
  `scripts/windows-studio.ps1 -RunBuilt`.
