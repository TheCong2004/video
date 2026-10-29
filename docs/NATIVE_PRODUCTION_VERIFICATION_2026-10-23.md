# Native production verification — 2026-10-23

Status: source changes implemented; not a production-ready EXE handoff.

## What changed

- Bundled `concat.native_production.v1` graph maps seven central commands to
  existing research-metadata, script, image edit, 9:16 expansion, video, voice,
  and local assembly implementations. Promax alone schedules the commands.
- Nexora native relay retains cloud credentials, while Concat receives only
  device identity and existing bounded transport envelopes. No Concat login UI.
- Graph/profile/source reservations, durable command receipts and unknown-outcome
  quarantine prevent cross-Page execution and blind duplicate submissions.
- Generated video is probed and decoded before import into the matching project.
- Configured Cloudflare HTTP API traffic now shares the Node/Nest origin used by
  WebSockets. Redirects, retries and scheduler fallback are disabled.
- Missing transport-origin configuration has its own safe native/UI diagnostic.

## Verified evidence

- 13 Node contract/regression tests pass. The seven-node graph is accepted
  unchanged by the actual backend graph schema; graph digest matches backend.
- 9 backend websocket/runtime-routing checks pass (including four affinity tests).
- `concat-studio` Rust test binary: 199 pass, 0 fail, 6 ignored, serial execution.
  An earlier full run had one loopback fixture fail; its isolated rerun and the
  subsequent full serial run passed. This does not prove provider E2E operation.
- Native seven-command fixture checks ordering, wrong graph/profile rejection,
  exactly three browser dispatches, receipts and duplicate rejection. Provider
  output there is a fixture, not a real Grok video.

## Deployment blockers / not verified

- Live backend websocket upgrade returned HTTP 503
  `TRANSPORT_ORIGIN_NOT_CONFIGURED`. Production needs an actual single Node/Nest
  runtime and `PROMAX_WS_ORIGIN`, with the same durable DB and auth configuration.
  A Cloudflare deploy alone cannot repair a missing upstream service.
- Running Nexora profiles currently did not advertise the required authenticated
  Grok extension capabilities. A browser window alone is not worker readiness.
- Latest Concat desktop code/Slint build and real provider-to-import E2E have not
  passed. Do not label the current installed EXE as containing this repair.
- Nexora release build is in progress; later native diagnostic changes still
  require recompilation/verification before installation.
- Current apps remain user-owned and running. No process was terminated to
  replace their binaries. No deployment or public tunnel was created here.
- No old build cache was deleted while active app/build processes hold it.
  D: had about 3 GB free at the last check; reuse targets, avoid duplicate builds.

Next: obtain the existing Node/Nest HTTPS origin (or an explicit hosting choice),
validate and configure it; finish current-source app builds while the apps are
closed; then run one user-approved end-to-end job in the selected Grok profile.

## Follow-up: Grok startup belongs to Create, not configuration checking

- Native Create is available after selecting a Page, without demanding an
  already-open Grok tab. Its existing confirmation snapshots/validates inputs,
  then prepares Grok in that exact profile. Configuration checking returns
  before any browser launch, tab creation or workflow mutation.
- Running profiles reuse any existing HTTPS `grok.com`/subdomain tab. If none
  exists, the existing Nexora local page API creates exactly one Imagine tab;
  it never navigates or closes unrelated tabs. A stopped profile uses Nexora's
  existing cold-start policy, followed by a fresh tab inventory.
- Profile/PID/generation and creation receipts are verified. An uncertain
  mutation is not automatically repeated. UI busy/pending guards reject a
  second Create while preparing/submitted.
- Grok worker readiness and Promax preflight still precede content generation.
  Completing this UX change does not repair the separate production 503.
- 3 new Rust HTTP-fixture tests cover existing-tab reuse across repeated calls,
  missing-tab creation and stopped-profile startup. Full core suite: 202 pass,
  0 fail, 6 ignored. Updated Node regression set: 14 pass, 0 fail.
- Read-only check of the installed Nexora confirmed the local page inventory
  API is available. No real tab was opened by diagnostics: startup remains tied
  to the user's Create confirmation.
- Full current-source Rust/Slint check passed: `windows-studio.ps1 -CheckOnly`,
  exit 0, 14m 11s. No newly built Concat binary has been installed or substituted
  for the running dev app; this check validates source, not a GUI/provider E2E.
- A definitively rejected native submission clears only its matching local
  reservation's pending flag. An unknown outcome remains blocked. This avoids
  leaving Create permanently disabled after a known preflight rejection.
