# Authentication / backend / key-management validation — 2026-10-15

This is a scoped implementation report, not a full four-editor-port claim.
Existing worktree changes were preserved.

## Latest follow-up: native-owned local AI (supersedes earlier gateway blocker)

Architecture chosen with the user: Promax cloud handles accounts/workflows;
Concat starts and owns a bundled loopback OmniRoute for AI Keys and inference.
Public OmniRoute hosting is NOT required for desktop AI. Cloud AI flags in root
wrangler config are disabled; no cloud deployment was made.

- The local supervisor starts off-thread only after verified native login or
  session restoration. Identity profiles use verified Supabase issuer/subject
  rather than ephemeral cloud memory-store user IDs. Account changes isolate
  provider storage; same-identity re-login preserves it.
- Windows DPAPI protects runtime secrets. Each runtime holds a profile file
  lock, picks its own loopback port, requires authenticated dashboard cookies,
  and is contained by a kill-on-close Windows Job. Sign-out/shutdown stop only
  the owned process. No Promax bearer token is forwarded locally.
- Native Keys, model/chat/test, LIVE text replies and structured editing use
  this local client. Existing cloud workflows/auth remain cloud routes. No
  submitted mutation is retried after an ambiguous result.
- Discovered and fixed a pre-existing UI/logic mismatch: Ask required a native
  confirmation flag but the rewritten UI never set it. The explicit quota-aware
  confirmation is restored. Diagnostics now inspect the owned AI client, not
  an unrelated process at fixed port 20128.
- Security verification initially FAILED: the prebuilt ArtCraft runtime returned
  200 for anonymous provider listing. Its shared auth requirement was disabled
  and duplicated across 110 Webpack bundles. Reproducible staging now hardens
  every copy of that known module and disables CLI-token bypass. Unknown builds
  fail closed; packaging verifies hardening before bundling.
- After hardening, real runtime integration PASSED: anonymous management is
  rejected; a local authenticated request reaches chat validation with zero
  configured providers/no paid inference; fixture provider add/list/disable
  survives child restart with stable ID; delete succeeds; Drop closes its port.
  Only an isolated random test profile and invalid fixture key were used.
- Adapter suite: **92 passed, 0 failed, 2 ignored**. The optional real-runtime
  group was then explicitly run: **7 passed, 0 failed**. The other ignored test
  needs a configured OpenTalking engine and was not claimed as verified.
- UI/auth/hardening structural suites: **23 passed, 0 failed**. Obsolete layout
  assertions were updated for the actual callbacks, multiline Slint formatting
  and bounded 100-row LIVE history; the Ask confirmation regression was fixed
  in product code, not removed from the tests.

Remaining limits: live Promax still has development/memory/test-principal
configuration. Production durable strict auth must be provisioned before public
release. No real-account OAuth or paid-provider inference was verified, and
OpenTalking rendering still needs its local engine/models. This follow-up does
not establish full feature parity of all four editor ports.

### Latest artifact and delivery checks

- `dist/windows/Concat-20261015-185500-494/Concat.exe` (development profile).
- SHA256: `C15D6C2107A0A1F064A2249033EBDD9A51FBB7FE5599AD0501F716F284AC8D91`.
- Final native build succeeded (17m07s). Automatic scoped cleanup reclaimed
  **3.79 GB** of regenerable compile/test artifacts, retaining dependencies,
  current reusable caches, sources, models, user projects and runnable EXE.
- Packaged DLL-loader `--version` and isolated native `--smoke` both exited 0.
- All **7 local-AI tests also passed against the PACKAGED OmniRoute folder**,
  with development PATH removed, in 53.10s. This includes real runtime CRUD,
  persistence, anonymous rejection and owned-child shutdown; not real inference.
- Package runtime inventory: **15,309 files, 700 MB, zero .env/database/log
  files**. Auth hardening was verified again in the packaged folder.
- Keep the entire package folder together. Do not copy just the executable;
  no installer/automatic overwrite of an already installed app was attempted.

### Current delivery self-check (supersedes historical score below)

Overall **3.8/5**: Accuracy 4 (native/runtime tests pass, real OAuth/inference
unverified); Completeness 3 (local architecture delivered, live cloud still
not production-hardened); Clarity 4 (scope and limits explicit, long historical
report retained for evidence); Actionability 4 (runnable package, needs real
user login/key verification); Conciseness 4 (short handoff; detail kept here).
Highest-impact follow-ups: verify a real native login and one configured-model
response, then provision durable strict cloud auth before public distribution.
The user should view this as a verified local-runtime package, not a claim that
the entire cloud deployment or full four-project migration is finished.

## Implemented

- Native first-run login gate for launch screen, desktop and phone editors.
- Actual Supabase email/password grant and signup, followed by verified Promax
  exchange. Confirmation-only signup does not unlock Studio. Native session
  restore verifies the cached principal with Promax; Windows DPAPI stores it.
- Google PKCE can open the system browser without a Nexora dependency; choosing
  a Nexora profile remains optional. OAuth redirect allowlisting remains an
  identity-provider deployment requirement.
- Add Key shows its real form without scrolling past the large canvas. Result
  status is visible. Submission uses the existing authenticated provider API,
  clears the visible secret, and does not report success without a receipt.
- Gateway catalogue uses Promax's model API instead of opening a hardcoded local
  dashboard. No hardcoded administrator key is shown or copied.
- Diagnostics use the configured Promax origin, not hardcoded port 43100.
- Server exception filter recognizes AI upstream failures and returns sanitized
  status/code instead of a generic 500. This server patch is **not deployed**.
- Windows packaging includes required native DLLs; startup smoke uses isolated
  application data on D:. This is a development-profile validation package,
  not an optimized production release.

## Completed checks

- `windows-studio.ps1 -TestAdapters`: **85 passed, 1 ignored, 0 failed**.
- `node --test scripts/studio-auth-key-regression.test.mjs`: **3 passed**.
- Backend TypeScript build and test compilation passed. Focused
  `ai-exception-filter.test.js` / `omniroute-management.test.js`: **3 passed**.
- Public identity-provider settings: email/Google/signup enabled, email
  confirmation required. No account was created and no credentials were used.
- Legacy launcher layout suite: **11 passed, 5 failed** after existing UI
  rewrites (project chooser, script-route shape, settings diagnostics, AI route,
  LIVE selector shape). It is not an all-green regression result.
- Native build: succeeded, 9m24s; development profile with window debug symbols
  disabled (native dependencies retain their existing profile settings).
- Artifact: `dist/windows/Concat-20261015-172243-521/Concat.exe`.
  SHA-256: `F35A7F9506FF411D94F0BF4CE30F5CC8802FB0C603E435B381C1FA3CAB9525E9`.
- Packaged `--version` DLL-loader check passed with only Windows system PATH.
- Packaged `--smoke` native GPU/engine/window-model startup passed with isolated
  D: app-data directories. This does not test interactive login/key creation.
- Cleanup of duplicate package `dist/windows/Concat-20261015-172159-428` was
  blocked by the execution policy; it remains on disk. The final package is
  the later `172243-521` directory. Reusable build caches were preserved.

## Live cloud findings / remaining verification

### Follow-up verification

- Read deployed version `0acf0b40-0eae-4c07-9262-20f343e51ebb` bindings via
  Wrangler: `NODE_ENV=development`, `PROMAX_STORAGE_MODE=memory`, no production
  database/gateway/management-key binding. Secret values were not printed.
- Server source now selects strict PostgreSQL settings for root Cloudflare
  deployment. Explicit local Wrangler development remains separate.
- Worker entry rejects incomplete public configuration with sanitized 503
  diagnostics before starting Nest. It no longer defaults cloud storage to
  memory or reflects unexpected exception messages to clients.
- Email-only bootstrap is denied in strict development as well as production;
  isolated NODE_ENV=test fixtures remain available and cannot pass the public
  Worker configuration guard.
- Full backend build/test compilation passed. Final complete runner:
  **357 tests, 356 passed, 0 failed, 1 skipped**. The test runner explicitly
  sets NODE_ENV=test. PostgreSQL durability was tested against the existing
  workspace-only `_test` database; its server was started for verification and
  stopped afterward. No production database was changed.
- OmniRoute source confirms management routes require dashboard auth or a key
  with manage/admin scope. Cloud AI configuration now requires a server-side
  upstream credential; it is never bundled into the desktop executable.
- These server fixes are **not deployed**: there is still no production
  PostgreSQL connection or secured public OmniRoute upstream available in the
  checked environment. Provisioning new hosting/public exposure requires an
  explicit infrastructure decision. The earlier EXE remains the current native
  package; no native source was changed in this follow-up.

Read-only probes confirmed the deployed health route returns 200 but the
provider list returns 500. Worker logs identify `AiModuleError`, which the old
filter incorrectly serialized as INTERNAL_ERROR. The checked-in gateway default
is desktop loopback; no gateway URL is configured there, and the queried worker
secret list was empty. A secured reachable HTTPS OmniRoute upstream must be
provided/configured; the exact deployed upstream was not independently read.

Unauthenticated `/api/v1/auth/me` returned a test principal. Provision strict
authentication, durable production storage and required secrets before public
distribution. Do not blindly redeploy the current development/memory config.

Real-user Google/email login, live key creation and AI inference are **not yet
verified end to end**. No cloud deployment or public tunnel was created.
OpenTalking still needs its local render runtime and model assets; worker
health does not make those engines available.

## Delivery self-check

Using the `agent-self-evaluation` rubric, overall **3.6/5**:

| Axis | Score | Evidence / improvement |
| --- | --- | --- |
| Accuracy | 4 | Build/startup and focused tests pass; no claim of real-account E2E. Verify OAuth callback with the user next. |
| Completeness | 2 | Public cloud gateway/auth configuration and live key save remain unresolved. Provision and securely deploy before claiming completion. |
| Clarity | 4 | App fixes and deployment blockers are separated; OAuth configuration is still an external prerequisite. |
| Actionability | 4 | Runnable package and smoke script provided; gateway HTTPS URL still requires the user's input. |
| Conciseness | 4 | Short handoff plus this evidence report; test counts are not proof of cloud readiness. |

Priority: configure a secured HTTPS gateway, provision strict/durable backend
auth, then verify one real login/key-save flow. The user would correctly view
this as a tested local package with cloud work remaining, not a finished public
release. Completeness cannot be raised honestly by UI changes or mocked tests.
# 2026-10-16 local connection follow-up

Current scope: Nexora integration, bundled TikTok connector, encrypted provider
storage and real AI Keys API dispatch. This is not evidence of full four-editor
feature parity or a production-ready cloud deployment.

- `windows-studio.ps1 -CheckOnly`: passed (native/Slint/auth/Nexora compilation).
- `windows-studio.ps1 -TestAdapters`: 100 passed, 4 opt-in integration tests
  ignored. Includes DPAPI migration/read-back, corrupt-archive preservation,
  configured Nexora token and real HTTP AI request/receipt fixture tests.
- `studio-auth-key-regression.test.mjs`: 8 passed, including Nexora shared
  configuration, packaged LIVE dependency wiring and no canned AI success.
- `local-ai-hardening.test.mjs`: 2 passed against all staged compiled auth copies.
- `local_ai::tests::bundled_runtime_round_trip`: passed with the actual staged
  runtime; no model inference, real authenticated provider CRUD and restart.
- `nexora::tests::running_nexora_health_and_profiles_are_real`: passed against
  the installed running Nexora; read-only health/profile requests, no browser
  launch, cookies copied, or profile stop.

Known failures retained rather than assertions deleted:
`studio-launcher-layout.test.mjs` has 14 passes / 2 failures: OpenTalking native
generation/project capture and LIVE TTS direct-call structural assertion after
the separate robust-TTS change. The missing Keys refresh button was restored;
the combined auth/key/launcher checks now have 22 passes / 2 failures. These
need reconciliation and interactive regression testing before full parity can
be claimed. No real provider quota, Google OAuth account, public TikTok room,
Nexora profile lifecycle, or native login visual parity was validated here.

Old unscoped plaintext provider JSON is encrypted into a DPAPI archive; it is
not automatically assigned to a verified user's local AI profile. Re-add old
keys explicitly when needed. Unknown outcomes of submitted mutations/inference
are not automatically replayed. New storage uses OmniRoute's encryption key
which is itself DPAPI-protected by Concat.

## Verified Windows package, 2026-10-16

Package: `dist/windows/Concat-20261016-123406-820` (development profile).
Final native build passed; EXE SHA256:
`67CC0C691D9EE0C73AD5B4CA8719605CD050962CD3151BB2F92BFC0EBE119E5B`.

- Packaged DLL loader `--version`: passed with system-only PATH.
- `windows-package-smoke.ps1`: native startup passed with isolated APPDATA,
  LOCALAPPDATA, TEMP/TMP and system-only PATH; no real account used.
- Packaged connector handshake: passed with system-only PATH, 18.46 seconds
  cold start, clean shutdown, no room joined or message posted. The former
  12-second timeout failed a cold source-runtime check; the final timeout is
  bounded at 45 seconds on a dedicated worker, not the UI thread.
- Packaged OmniRoute authenticated CRUD/restart/encryption integration: passed
  with system-only PATH in 91.10 seconds; no paid inference.
- Package inventory: 18,896 files, 1,308,367,664 bytes; zero .env/database/log
  files. User keys/profile stores are outside the distributable.
- Build cleanup reclaimed 3.85 GB of regenerable artifacts after final build.

Installer: `dist/windows/ConcatStudio-Setup-x64-20261016-124745.exe`,
242,480,172 bytes, SHA256
`23D330C13DCAFD830E30223D921ED8799275F0E4968146EA957D508FA8DBA43E`.
NSIS 3.11 completed successfully with `/WX` (warnings are errors). The first
attempt exposed an UninstallString quoting warning; that attempt was stopped,
its partial EXE removed, the script corrected and rebuilt without warnings.
The installer is unsigned. Actual install/uninstall was not exercised against
the user's existing installation; portable-package startup and service tests
do not claim that separate end-to-end installer check. Installer compression
scratch stays on D and exact generated manifest/empty directory cleanup is
automatic on future successful runs. No app/user/Nexora data is removed.

## Scoped self-evaluation (supersedes the historical score above)

Accuracy 4/5: native startup and packaged service tests pass; real-account AI
and OAuth remain untested. Completeness 3/5: requested connection paths are
wired, but old keys need explicit re-entry and the two unrelated OpenTalking /
LIVE checks remain unresolved. Clarity 4/5: runtime readiness, provider response
and quota claims are separated; the historical report still needs careful
reading. Actionability 4/5: a tested portable package and installer workflow
exist; production cloud auth remains an external release prerequisite.
Conciseness 4/5: the handoff can link this report instead of repeating logs.
Overall 3.8/5. The user should regard this as a scoped tested update, not a
finished full port or a production release. Highest-impact next checks are
real-account login/AI inference, then OpenTalking/LIVE generation regressions.

## Live Google failure follow-up, 2026-10-16

With the user's explicit approval, the lightweight `concat-studio` example
`auth-diagnostic` opened a fresh PKCE Google sign-in through the production
adapter. Callback receipt succeeded; Promax exchange returned HTTP 400. No
callback URL/code/token was printed or replayed, and no model was invoked.
This supersedes the earlier untested-real-account diagnostic status, but is
not a successful real-account login.

The actual Worker/Express/controller fixture reproduced HTTP 400 with
`body === undefined`, before remote identity verification. `src/worker.mjs`
used a bare EventEmitter as its synthetic socket; Express's body parser treated
it as already finished. The source fix uses a real PassThrough stream, frames
the buffered bytes with their actual Content-Length, removes stale chunked
framing, and marks the request complete. The same test then passes exchange
and authenticated `/auth/me`, while anonymous/invalid identities remain 401
and malformed JSON remains 400. Twelve focused Worker/config/WebSocket tests
pass. No production deployment was performed.

Concat source now removes the login-screen `signed-in = true` bypass and
separates identity rejection, Promax HTTP rejection, invalid session receipt
and network errors. Adapter tests: 102 pass, 4 opt-in ignored. UI/auth structural
checks: 9 pass. The existing EXE/installer has not been rebuilt for this follow-up.
The new diagnostic example can be built independently without Slint/media.

Cloudflare root `wrangler secret list` returned no configured secrets. Current
source production settings require strict auth/PostgreSQL, DATABASE_URL and
PROMAX_AUTH_SECRET; deploying blindly would make the public service unavailable.
Do not weaken those guards or republish the permissive memory/test principal.
Production provisioning/deployment and a fresh approved real login are still
needed before claiming the public authentication issue is resolved.

Follow-up verification: native `windows-studio.ps1 -CheckOnly` passed (9m04s);
`git diff --check` passed on the changed auth/Worker files. The production URL
was not deployed or mutated. Local `.env` inspection printed presence/scope only:
no DATABASE_URL and no production auth secret were found. No secrets were read
into the report or chat. The running user's Concat process was left untouched.

Scoped review: accuracy 4/5 (fresh Google callback + real HTTP400 reproduced and
the actual Worker/Express path verified locally; deployed fix still untested),
completeness 3/5 (public deployment blocked by production database/secrets),
clarity 4/5 (identity/password/Promax stages are distinct; internal labels still
need a fresh dev binary to appear), actionability 3/5 (source and standalone
diagnostic are usable, but database provisioning needs user direction),
conciseness 4/5 (short handoff plus evidence here). Overall 3.6/5. No critical
score <=2; deliver the verified source change with the deployment blocker, not
a false successful-login claim. The user would still correctly consider live
sign-in unfinished. Next improvements: provision strict durable Cloudflare
bindings, then deploy and verify one fresh user-approved Google login.

