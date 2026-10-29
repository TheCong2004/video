# Nexora / Grok batch execution

Implemented 2026-10-29. This extends the existing native Control owner; it does
not create another project, account, browser or queue owner.

`nexora.pages` reads the current encrypted Nexora settings and actual profile
catalogue, overlaying existing channel presets. `ai.plan` receives this catalogue
as untrusted context automatically. Browser profiles are not proof of Facebook
Page authority. No browser cookie or cloud token is returned to the model.

`production.generate` uses the existing `ProductionClient` and profile-scoped
recovery fence: exact profile -> Grok image edit -> 9:16 expansion -> video.
It opens/reuses Grok through the existing manager and starts the existing sidecar
if required. The source must be a user-attached anchor. The output root is the
owned Control workflow directory. A profile without a Concat preset receives
only a job snapshot; no duplicate Page or permanent preset is created.

`batch.expand` accepts 1–100 jobs and expands them transactionally into existing
Control steps, before dispatching a child. Each job contains a distinct brief,
target duration and either actual attached `paths` or an attached `anchor` with
actual `profile_id`, `image_prompt`, `expand_prompt`, `video_prompt`. Missing
scripts use `ai.script` automatically, followed by optional Grok footage,
`factory.render` (existing speech/subtitle/edit adapter), and `artifact.validate`.
`same_story: true` permits shared briefs/scripts for explicitly requested serial
content; it does not assemble the outputs into one feature-length film.

The existing graph store is bounded to 512 steps to fit 100 four-stage jobs.
Model-authored plans retain their independent 16-step bound. Batch expansion
validates asset scope and profile existence before admitting children. Numeric
counts directly preceding “video” in the request must match the job count.
Exact normalized duplicate briefs and supplied scripts are rejected by default;
semantic originality of generated content is not guaranteed by a string check.
Each job appears with its video number and stage. Execution is sequential through
the existing driver; 100 requested videos does not mean 100 simultaneous workers.
An unknown Grok result pauses the batch before subsequent jobs consume more AI;
it does not replay the uncertain generation request.

The independent Control window exposes **Mở Nexora** and **Đọc hồ sơ Nexora**.
Both call existing Xưởng phim AI/browser callbacks and use the same settings.
No new download destination, authentication or separate browser lifecycle was added.

## Verified and remaining

Read-only live checks confirmed Nexora protocol v1 READY and two real profiles.
No profile was opened and no account or generation job was operated in that check.
The batch test persists and executes a 100-job / 400-child graph using protocol
fixtures, verifying dependency references and settlement. It does not create
100 live Grok videos. Existing Grok transport tests use controlled HTTP fixtures.

The existing SocialPublisher registry in this batch feature has no publisher
executor. Campaign Automation now adds a separate official Meta Graph/Reels
adapter and a persistent Page-token connection flow. That adapter has
private-origin contract tests; no live Page connection or public post was
verified. A production campaign still requires an operator-configured Nexora
profile, permitted assets, an authorized Meta app/Page and authenticated
end-to-end tests. Text-only image creation, multi-shot generation and arbitrary
automation of every existing UI callback remain outside these adapters. The
Grok campaign adapter requires an anchor and the existing vertical pipeline.
See `AI_CAMPAIGN_AUTOMATION.md` for the campaign audit and acceptance boundary.

## Changed files

- `src/crates/concat-studio/src/control_batch.rs`: typed batch compiler and tests.
- `src/crates/concat-studio/src/ai_control.rs`: bounded 512-step durable graphs.
- `src/crates/concat-studio/src/control_plan.rs`: atomic batch expansion seam.
- `src/crates/concat-studio/src/control_display.rs`: video/stage labels.
- `src/crates/concat-studio/src/lib.rs`: module export.
- `src/crates/concat/src/studio_control_production.rs`: real Nexora/Grok adapters.
- `src/crates/concat/src/studio_ai_control.rs`: registry, preflight, permissions,
  batch dispatch/settlement, real planning context and unknown-result pause.
- `src/crates/concat/src/studio_control_window.rs`: existing Nexora callbacks.
- `src/crates/concat/src/lib.rs`: native adapter module.
- `src/crates/concat-control-ui/ui/control.slint`: Nexora connection controls.
- `scripts/audit_ai_capabilities.py`, `docs/AI_CAPABILITY_MATRIX.md`,
  `docs/ai-control/inventory.json`: updated executable inventory.
- `docs/AI_PROGRESS.md`, `docs/AI_TEST_RESULTS.md`: evidence and remaining work.

Verified build: `src/target/debug/concat-ai-control-b1e2fbb15152.exe`.
Restart through the normal launcher to load it. No actual 100-video campaign
or social publishing was run as part of verification.
