# Campaign execution progress

The Campaign Manager now presents the durable jobs as a vertical path: content,
media, render, quality, and (only when publishing is enabled) publishing.
Each block shows completed video counts and the current stage state.

- A blue glow and moving light require a job in `Running` with an unexpired
  worker lease. Campaign-level `running` alone never starts an animation.
- Completed stages are green. Blocked stages are amber with their stored error.
  Paused, waiting, cancelled, and expired-worker states have static presentation.
- Animation respects the shared application and OS reduced-motion settings.
  The enable-motion button invokes the existing preferences owner.
- Polling does not toggle the action busy state or clear a failed action's
  persistent error banner. Polling still uses the existing three-second timer.
- Login busy state and motion preferences are mirrored to the secondary window;
  provider selection is restored from the selected campaign's plan.

## Verification on 2026-10-29

`cargo test -p concat-studio --profile quick --offline --lib
campaign::tests::progress_requires_live_worker_and_exposes_blockers_in_the_correct_stage
-- --test-threads=1` passed (1 test). It checks pending jobs, blocked authentication,
running without a lease, a live lease, lease expiry, and pause. It uses isolated
temporary campaign storage and makes no external model or publishing requests.

The native Control UI compiled with `cargo check -p concat-control-ui --offline`
before the final desktop build. This is compile verification, not an authenticated
end-to-end generation or a visual acceptance test of the Windows UI.

`scripts/test-kedi-launcher.ps1` passed all 16 assertions. `-PreviousBuild` uses
the prior verified receipt (with checksum validation), so rebuilding `concat.exe`
does not replace the rollback target. When no previous receipt exists, the legacy
original-executable fallback is retained.

Changed sources: `concat-studio/src/campaign/{progress.rs,mod.rs,tests.rs}`,
`concat-control-ui/ui/{campaign.slint,control.slint}`,
`concat/src/{studio_campaign.rs,studio_control_window.rs}`, and
`scripts/{select-kedi-build.ps1,test-kedi-launcher.ps1}`.

## Published native build

The standard Windows build completed, followed by a cached rebuild that included
the final source edits. Both completed successfully. The publisher verified
`--version` and `--smoke` in the isolated test profile; both exited with code 0.
No authenticated model generation, UI click-through, or production publishing was
performed by these checks.

- Launcher revision: `Campaign stage glow + real progress / 2026-10-29`
- Executable: `src/target/debug/concat-ai-control-2a6e42b5b12a.exe`
- SHA256: `2a6e42b5b12af65f27ff91187321cefae9cc95bee7414d8a214860cb552c88dd`
- Previous verified executable: `concat-ai-control-0c6aea947706.exe`
- Logs: `.runtime/campaign-motion-{native-build,version,smoke}.log`
- Campaign worker remains the previously verified protocol-1 executable; this
  change does not alter the worker protocol or execution authority.

Eleven obsolete generated GUI executable copies (3,412,162,560 bytes) were
identified for cleanup while protecting current, previous, and running builds.
The deletion command was rejected before execution by automatic tool review with
`blocked by policy`; those files were not deleted.
