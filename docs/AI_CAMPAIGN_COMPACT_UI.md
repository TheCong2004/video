# Campaign window: optional publishing (2026-10-30)

The secondary AI window no longer exposes Hide AI or Show main app buttons.
Its campaign login, actions, progress and results share one scrolling area, so
the campaign setup card no longer reserves space above the results viewport.
Empty status banners are omitted.

The Nexora selector chooses the browser profile used to generate media through
Grok. It does not choose a Facebook Page. Publishing remains optional and defaults
to off for a new campaign. Without publishing, the form ignores leftover Page
inputs and does not read Facebook credentials to create the plan. Page fields,
Facebook connection controls, publishing calendar and reconciliation actions are
only shown when publishing is enabled. Existing Page authorization checks remain
in place when publishing is enabled; multiple authorized Pages do not require
multiple Grok browser profiles.

Validation: `cargo test -p concat-studio --lib campaign::tests --profile quick
--offline`: 21 passed. The added case checks generation without a Page, refuses
publishing with no Page, and accepts two Pages with one Grok profile. These are
local tests; no generation or Facebook publication was dispatched.

Desktop build log: `.runtime/campaign-compact-native-build.log`.
Build completed successfully; version and smoke processes both exited 0.
Published executable: `src/target/debug/concat-ai-control-82ac06062ee8.exe`.
The launcher selects this hash-bound build and preserves the existing campaign
worker receipt. Launcher selection tests: 16 passed. The prior UI build remains
available through `-PreviousBuild`. No interactive desktop layout test was run.
