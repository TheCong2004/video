# Grok login confirmed but worker offline — 2026-10-30

The user confirmed Grok was open and signed in in Nexora profile “á”. Actual
registry metadata still reported only its legacy OFFLINE worker. The configured
extension existed and Chrome's extension arguments referenced it. The canonical
Playwright media runtime on loopback port 9223 was offline.

Two startup defects were found and fixed:

- The campaign previously created the runtime only in Media, after content work.
  It now retains a runtime guard for the entire campaign worker and requires a
  real ready/extension/auth/capability report before claiming content jobs.
- The runtime passed Rust's Windows verbatim `\\?\` script path to Node. The
  bundled Node reproduced `EISDIR`; the normal path passed `node --check` with
  exit 0. Startup now reuses `local_ai::node_path`, the existing path normalizer.

An explicitly started diagnostic runtime became healthy. A canonical `grok.health`
lease was acquired and released through Nexora, without generation or publishing.
The registry then reported an actual READY worker with `extension_ready:true`,
`grok_logged_in:true`, and image edit/expand/video generation capabilities.
No readiness fields or capabilities were fabricated.

Automatic approval review rejected a combined publication/diagnostic-runtime-stop
command with only `blocked by policy`. It did not execute. The diagnostic runtime
was kept alive; no alternative attempt to stop it was made. Worker publication
was completed separately and the campaign reused the healthy service, a supported
runtime path. Fresh native spawning after the path fix is not claimed as a full
end-to-end test; the actual service, health lease and reuse were verified.

The next campaign attempt exposed a preexisting exact-duplicate check that included
the current recovered script itself. The campaign was paused and the verified
idle campaign worker stopped, leaving the diagnostic runtime untouched. The check
now excludes the current video, while a real duplicate in another video still
fails. A regression executes the actual Content stage with locked scripts and no
AI connection for both cases.

During that attempt, video 4 had dispatched a new model request before pausing.
Its outcome was retained as unknown. The user explicitly chose “Bỏ lượt cũ, tạo
mới video 4”. A fenced operator-only discard now checks the exact request ID,
video revision, paused/no-lease state and absence of existing script/artifacts.
It keeps the old receipt and audit and permits a new dispatch with a new revision.
This is an explicit user-directed discard, not an automatic replay.

The user's local automation request also allows switching this non-publishing,
paused campaign to FullAuto, preserving all content/provider/quality constraints.
This control refuses publishing campaigns. A versioned plan revision and approval
are required internally; no social posting is enabled.

Tests: 23 campaign tests passed, 1 existing live test ignored. Logs:
`.runtime/campaign-grok-resume-final-tests.log`,
`.runtime/campaign-grok-resume-worker-build.log`, and worker stdout/stderr logs.
Final publication and runtime status are appended after verification.

The corrected worker initially reached Media but failed before dispatch with
`LEASE_CORRELATION_INVALID`. The real worker identifier is
`playwright-profile:<profile-id>`, which the generic identifier validator rejected
because of the colon. The validator now accepts exactly that canonical prefix
with the same selected profile, in addition to existing simple IDs; other profile
and path characters remain invalid. An actual HTTP fixture with the canonical
worker ID passes all three dispatch paths.

The failed production manifest had state FAILED, zero stages and no pending
submission. Its retained lease/job/profile matched the registry and was explicitly
released (`RELEASED`). This proves no image request was dispatched. A fenced local
repair CLI permits retry only for this exact pre-dispatch failure, paused state,
matching video revision/profile/output directory, zero artifacts and exactly one
matching manifest. It preserves the script and receipt and does not generalize
unknown-outcome retry.

The first parallel production fixture run had 3 transport/preflight failures;
the serial rerun passed all 22 tests. Logs:
`.runtime/grok-canonical-worker-tests.log`,
`.runtime/grok-canonical-worker-serial-tests.log`, and
`.runtime/grok-canonical-worker-build.log`.

The user explicitly discarded video 4's interrupted request. Its receipt remains
retained; a fenced operator action cleared only that job's uncertain state and
incremented its revision. The local-only FullAuto plan was versioned/approved with
publishing still disabled. Three recovered scripts remain locked.

Final published worker SHA256:
`3cfe60d6311c768a9dcc19626a641e985868eff41e1aca9ad03dda9aaf30a1de`.
The verified no-dispatch media failure was repaired through its restricted CLI
path, preserving the failed manifest. Worker PID 3008 was started via the normal
worker entry point. Video 1 passed its semantic check and entered Media. The new
production manifest `b739e8bf-d854-43c6-9ae9-6cead693f83b` records RUNNING at
GENERATING_IMAGE with a persisted pending submission. No output artifact had yet
been confirmed at the last inspection. This is a real generation attempt, not a
fixture, and must not be submitted again while its outcome is pending.
