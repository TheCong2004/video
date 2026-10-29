# Recover retained content replies — 2026-10-30

The user supplied the exact response for video 2, then responses for videos 1/3.
The latter contain an unfinished JSON string followed by a complete replacement
envelope with the same request ID. The reader now restarts at bounded envelope
boundaries only when the normal scanner found no valid JSON. Complete conflicting
envelopes remain rejected; request correlation and native validation are retained.
No response is evaluated as code and no prompt is sent during recovery.

`CampaignStore::recover_content` validates a paused campaign, the video revision,
absence of a live lease, the exact retained dispatch ID plus `-content`, required
script fields, size/duration and duplicate narration. It preserves the supplied
script, sets `script_locked`, and clears the review error without submitting AI.
The next content stage still validates the locked script and performs configured
originality checks; it does not ask AI to write that script again.

All three supplied replies were recovered into campaign
`campaign-100d646ed36cd607481339defdd1d5a3`, videos 1/2/3. All nine jobs are pending;
three have locked scripts, six have no script yet. No unknown jobs remain.
The snapshot before recovery is `.runtime/campaign-before-content-recovery.json`.
The adopted envelopes are `.runtime/recovered-video-0000{1,2,3}.json`.

The previous media source was `postertuyendung.png` with no Grok profile. The user
selected Nexora profile “á” (`9ea5045f-4e4f-48ee-ab27-28632bac4954`). A narrowly
scoped media-source rebind preserves scripts only while no media/render/publish
has happened and no leases exist. It appends a plan version, removes the old
poster source, and requires a new approval. This source-only version was approved
using the user's explicit profile selection and existing generation request;
publishing remains disabled. The pre-rebind snapshot is
`.runtime/campaign-before-media-rebind.json`.

Nexora was started via its existing configuration owner. Opening Grok in the
selected profile returned `UnknownOutcome`, so it was not repeated. Read-only
metadata confirms that profile's Chrome PID 26292 opened at 00:04:34, but the
worker still reports `grok_ready:false`. The user was asked what that window
shows and to sign in directly if needed. The campaign was not started.

Verification: 21 campaign tests passed, 1 existing live test ignored; 42 parser/DOM
regressions passed, plus the added truncated-envelope DOM test passed with exactly
one Send. Actual recovery CLI calls succeeded for all three videos. Logs:
`.runtime/campaign-recover-content-final-tests.log`,
`.runtime/campaign-truncated-response-regression.log`, and
`.runtime/campaign-truncated-dom-focused.log`.

Worker rebuilt and published with SHA256
`dd5a8d563621524537eb5b80bd4004602295b032c3c4d5b001fca30a66e207a2`.
The active GUI receipt (`concat-ai-control-f780f05a3756.exe`) was preserved and its
worker hash atomically updated. Launcher receipt verification passed. Previous
worker/receipt are retained as
`dist/kedi-test/concat-campaign-worker-d609dbb6a5a3.exe` and
`.runtime/campaign-recover-content-worker.previous.json`.
