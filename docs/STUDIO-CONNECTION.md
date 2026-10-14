# Native Studio connection — authenticated migration pilot

The native Workflow Promax page now requests the real tenant-scoped workflow
catalogue using GET `/api/v1/artcraft/workflows/definitions`. The separate
production page supports preflight/start/cancel of registered Promax runs, with
explicit confirmation and no automatic retry. AI Keys supports connection
listing, API-key creation, enable/disable/delete, model listing, connection tests
and routed chat. None of these requests run automatically at startup.

Provider administration requires the server's owner/admin permission. Ordinary
product authorization is sufficient for model listing/chat; the server remains
the authority. A connection test is not a generated answer. Routed chat is not
pinned to a connection ID and is labeled accordingly in the UI.
Quota is not fabricated from a successful response. API-key creation currently
covers registered providers requiring no extra provider-specific configuration;
OAuth/custom endpoint configuration is not migrated.

## Current session boundary

Native Google PKCE sign-in is implemented using an explicitly selected Nexora
Chromium profile. Promax verifies the Supabase identity and owns the tenant
session; Concat stores that session with Windows DPAPI. Browser cookies are not
copied. Interactive real-account sign-in and refresh still need verification.
The ArtCraft proxy also requires a verified bearer session.

This pilot accepts a valid **Promax access token for the ArtCraft product scope**
from the launching process environment, `CONCAT_PROMAX_ACCESS_TOKEN`. The optional
non-secret `CONCAT_PROMAX_ORIGIN` selects the Promax origin; the default is
`http://127.0.0.1:43100`. A Supabase/Google/provider API key is not this token.
The environment-token path is a developer pilot. Native login, explicit restore,
serialized refresh and logout are now available in Studio Settings. Using any
native account action disables the environment-token fallback for that app run.

Never send tokens in chat, paste them into a URL, commit them, put them in a
project file, or enter them as command-line arguments. An expired/missing token
shows an auth-required message; 403, 429 and unavailable services have distinct
messages. No raw upstream error or response body is displayed/logged.

For a developer-owned test token, enter it through a masked PowerShell prompt
in your own terminal (not this chat). This example avoids terminal history
containing the token, but process environment memory is not a credential vault:

```powershell
$previousStudioToken = $env:CONCAT_PROMAX_ACCESS_TOKEN
$studioSecureToken = Read-Host 'Promax access token (ArtCraft scope)' -AsSecureString
$studioTokenPointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($studioSecureToken)
try {
    $env:CONCAT_PROMAX_ACCESS_TOKEN = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($studioTokenPointer)
    # Optional: configure a trusted HTTPS origin explicitly before launching.
    # $env:CONCAT_PROMAX_ORIGIN = 'https://your-existing-promax-host.example'
    .\scripts\windows-studio.ps1
} finally {
    $env:CONCAT_PROMAX_ACCESS_TOKEN = $previousStudioToken
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($studioTokenPointer)
    $studioSecureToken.Dispose()
}
```

The token is sent only to the explicitly configured origin. HTTPS is required
except for numeric loopback addresses. Userinfo, paths, query strings, fragments,
port zero and invalid header characters are rejected. Redirects are not followed.
Catalogue reads have an 8-second timeout and a 1 MiB response limit; a malformed response
is not presented as an empty list. Definitions have bounded count/text/graph sizes.

State-changing gateway requests have a 15-second client timeout. A timeout after
submission is an unknown outcome, not proof that nothing happened. No automatic
retry is made. Provider secrets are cleared from the input after submission and
never persisted/logged by Concat; the configured server persists the key.

## Nexora local profile manager

The native production page can list existing Chromium profiles and explicitly
open Grok or stop one selected profile using Nexora's existing local routes.
`DONUT_BROWSER_API_URL` defaults to `http://127.0.0.1:10108`; numeric loopback HTTP
only, no URL credentials or redirects. No browser cookies are read or copied.
Profile identity is rechecked before mutation. Stop sends the confirmed PID and
launch generation so a changed session is not silently terminated.

This does not migrate the worker lease/dispatch, Grok login detection, extension
readiness or local video pipeline. Running a profile is never reported as those
capabilities being ready. Nexora itself must already be running; Concat does not
spawn it as a side effect of loading a page.

## Build / launch on Windows

From the Concat root:

```powershell
.\scripts\windows-studio.ps1 -CheckOnly
.\scripts\windows-studio.ps1 -TestAdapters
.\scripts\windows-studio.ps1
```

If the executable has already been built, open it without invoking Cargo:

```powershell
.\scripts\windows-studio.ps1 -RunBuilt
```

The launcher resolves `sherpa-onnx-c-api.dll` from the actual installed Sherpa
1.13.7 runtime, not an empty `sherpa-dlls` folder. Missing runtime DLLs and import
libraries are different problems: successful compilation alone is not a loader
smoke test. The launcher regression test is
`scripts/windows-studio-launcher.tests.ps1` (uses installed dependencies, mocks
Cargo, checks DLL resolution and environment restoration; does not launch UI).

The helper discovers installed native development dependencies in `.deps` or
respects explicit `FFMPEG_DIR`, `SHERPA_ONNX_LIB_DIR`, `ORT_LIB_LOCATION`. It sets
the runtime DLL search path for this invocation, restores environment values on
exit, and limits Cargo to two jobs unless you explicitly configured otherwise.
It does not download models, start backend processes, modify user data or use
DOCS_RS. It uses Cargo's default debug profile because sherpa-onnx-sys 1.13.7's
build script misidentifies the output directory with the custom quick profile.
  The current isolated adapter tests live in `concat-studio`, not `concat-host`.
  That crate has no media/window/speech dependencies and can use the quick profile
  without the Sherpa custom-profile problem.

Cancellation hides/discards the request result; it does not abort a blocking
socket instantly. A cancelled in-flight call retains its single-flight slot
until completion/timeout, so repeated cancel/refresh cannot spawn parallel calls.

## Verification status

- HTTP fixture tests cover the actual endpoint, bearer header, response envelope,
  empty/malformed/oversize responses and 401/403/429/503/redirect errors.
- Live authenticated user-account catalogue access and manual desktop navigation
  still require a supplied valid test session; no production-account call was
  made automatically during implementation.
- Complete native `scripts/windows-studio.ps1 -CheckOnly`: passed, including the
  actual window Rust bindings and Slint code, without docs-only build flags.
- Standalone workflow adapter fixtures: 8 passed, 0 failed. Locale inventory,
  Rust formatting for changed modules, PowerShell parsing and diff checks pass.
- Actual host crate `scripts/windows-studio.ps1 -TestAdapters`: 13 passed,
  0 failed (5 health fixtures + 8 workflow fixtures); 78 unrelated unit tests
  and 11 unrelated export tests were filtered, not represented as verified here.

### Windows loader fix verification

The user-built debug executable initially exited with STATUS_DLL_NOT_FOUND.
PE dependency inspection confirmed `sherpa-onnx-c-api.dll`; the launcher previously
searched an empty `.deps/sherpa-dlls` directory instead of the installed runtime's
`lib` directory. No third-party DLL download or Rust recompilation was needed.

- `windows-studio-launcher.tests.ps1`: passed DLL-path and environment-restore checks.
- Actual `windows-studio.ps1 -RunBuilt` smoke: process stayed running with a
  nonzero window handle and title `Concat` after 8 seconds. A direct executable
  smoke also remained running after 10 seconds. Only the owned test processes
  were closed; no existing user application was terminated.
- This establishes startup, not workflow sign-in or a full interactive UI test.

Loader-fix self-review: accuracy 4/5 (PE inspection and actual startup verified;
interactive actions untested), completeness 4/5 (reported failure fixed; installer
packaging remains outside this fix), clarity 4/5 (RunBuilt command documented;
developer guide remains English), actionability 4/5 (existing EXE opens without
Cargo; requires the installed runtime folders), conciseness 4/5 (narrow launcher
change and one regression test; accumulated guide is longer than a quickstart).
Overall 4.0/5. Next improvement is a packaged-runtime test independent of `.deps`.
Self-check: the user should agree the startup error is fixed, not that the full
migration is complete. Verdict: deliver the verified local loader fix.

## Delivery review

Self-evaluation: 3.6/5 for this migration increment, not the finished replacement.

| Axis | Score | Evidence / improvement |
|---|---|---|
| Accuracy | 4 | Native type check and adapter fixtures pass; linked desktop/manual UI still unverified |
| Completeness | 3 | Native PKCE/session implementation and real catalogue; interactive login, jobs and artifact automation still pending |
| Clarity | 4 | Distinct error states and explicit read-only notice; developer connection guide is English |
| Actionability | 3 | Windows helper, developer-token pilot and native account UI exist; interactive account verification remains |
| Conciseness | 4 | User-facing labels are short; implementation evidence is kept here rather than in chat |

Highest-impact next work: implement the verified native OAuth/session lifecycle
and run a manual desktop test, then connect workflow execution/artifacts. Do not
compensate by weakening auth, scraping browser sessions or inventing workflow data.
Self-check: the user should regard this as a working adapter increment with a
remaining sign-in boundary, not a complete usable production migration. Verdict:
deliver this increment with those limitations disclosed; no critical safety gap
is being hidden behind a green connectivity badge.
