# Campaign image generation without an upload — 2026-10-29

Campaigns with no reference image or local media now generate their first image
from the AI-written image prompt. A selected Nexora Grok profile is still needed.
An uploaded reference continues to use the existing reference-image workflow;
existing local media continues to use the local edit workflow.

The existing extension executor already handles `grok.image.edit` without
`sourceArtifact`: it validates the prompt, captures baseline media, skips upload,
submits generation, and correlates fresh output. Concat now exposes that branch
through `ProductionPlan::from_prompt`. No blank image, sample product image or
placeholder upload is substituted. Only the first request omits the source;
vertical expansion and video generation consume the exact saved prior artifact.

`Providers::uses_grok` selects generation for either a reference image or empty
local media. Validation, preflight, budget estimation, media reservation, profile
serialization locks, execution and restored campaign form state use this same
predicate. Thus absent uploads cannot bypass media budgets or become empty renders.
No existing uncertain generation is automatically replayed.

Changed files:

- `src/crates/concat-studio/src/production.rs`
- `src/crates/concat-studio/src/campaign/{model,engine,store,worker,tests}.rs`
- `src/crates/concat/src/studio_campaign.rs`
- `src/crates/concat-control-ui/ui/campaign.slint`

Verification: campaign tests 19 passed, 1 existing live test ignored; production
tests 21 passed. The new HTTP fixture test checks prompt-only dispatch, exact
image chaining into expansion/video, saved receipts and absence of a fabricated
source image. A campaign test verifies the missing-profile message, generation
cost estimate and media budget reservation. The original reference-image HTTP
test also passes. These tests use a local fixture server; a real Grok generation
has not been run for this change. Logs:
`.runtime/campaign-auto-image-tests.log` and
`.runtime/production-auto-image-tests.log`.

Native build passed in 2m41s; worker build passed in 40s. `--version` and
`--smoke` exited 0, including the real three-provider callback bridge probe.
Launcher target: `src/target/debug/concat-ai-control-94799dc674d5.exe`.
GUI SHA256: `94799dc674d58938918e78ebd037d46516dd47d36dce7592b82f5ce27f2d1f50`.
Worker SHA256: `059f2d8e81b231f255982d18b6baeabded9cba841130771eec6a24eab1ed1cf7`.
Build logs: `.runtime/campaign-auto-image-{native,worker}-build.log`; startup
checks: `.runtime/campaign-auto-image-{version,smoke}.log`.
Existing application windows were not closed automatically.
