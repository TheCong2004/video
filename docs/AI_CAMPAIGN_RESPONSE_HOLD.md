# Stop repeated content generation on unread replies — 2026-10-29

The reported nine-video campaign continued claiming other videos after a web
response became unknown. A completed receipt also contained the provider message
“Sorry, something went wrong. Please try your request again.” Native script
deserialization classified that as retryable, generating another content attempt.
The screenshot additionally shows different envelopes with one correlation ID;
the reader correctly refuses to guess between conflicting results.

Immediate intervention used the existing worker `--control ... pause` command on
`campaign-100d646ed36cd607481339defdd1d5a3`. Its saved state was verified paused
with zero active leases. Only its exact idle worker PID 7356, executable path and
campaign command line were then verified and stopped to allow replacing its binary.
No Chrome process, project, generated video or campaign receipt was deleted.

Changes:

- Native malformed script JSON now requires review, rather than automatic retry.
- Settlement of unknown/input failures pauses the whole campaign.
- Claims refuse further content/media work while a previous content/media job
  remains unknown or needs review, including after pressing Start again.
- The web reader recognizes completed, bound conflicting response envelopes and
  provider error text, reports specific errors, and never sends another prompt.

Files: `campaign/{engine,store,tests}.rs` in `concat-studio`, and
`tools/ai-edit-mcp/{control-web.mjs,control-web.test.mjs,control-web-browser.test.mjs}`.
The browser regression passed 41/41 using local intercepted pages. The initial
campaign regression passed 20 tests (1 existing live test ignored); final tests
also exercise trying Start after a held response. Logs use
`.runtime/campaign-response-hold-regression.log` and
`.runtime/campaign-stop-loop-final-tests.log`.

Final campaign tests passed 20/20 with 1 existing live test ignored, including
pressing Start again while a response still requires review. A separate IDE
`cargo check -p concat` held the debug artifact lock. Our queued debug builds were
stopped; that separate check was preserved. This change executes in the background
worker, so no GUI rebuild is required. The worker was built with the existing
`quick` profile in 52 seconds and its CLI version/protocol check passed.

Published worker SHA256:
`d609dbb6a5a3c6ff3ea43ba6e60e46a899428aa5d023e5b42962d34c10b30ce3`.
The verified GUI remains `concat-ai-control-94799dc674d5.exe`. Its launcher receipt
was atomically updated with the worker hash and separate worker revision/time.
The previous worker is retained at
`dist/kedi-test/concat-campaign-worker-059f2d8e81b2.exe`; its matching receipt is
`.runtime/campaign-stop-loop-worker.previous.json`. Rollback requires stopping an
idle worker, restoring both that binary to `src/target/debug/concat-campaign-worker.exe`
and that receipt to `.runtime/kedi-current-build.json`. No rollback was executed.
Build log: `.runtime/campaign-stop-loop-worker-quick-build.log`.

The nine-video campaign remains paused; its failed/unknown requests are not replayed.
