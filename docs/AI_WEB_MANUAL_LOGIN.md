# Manual AI web sign-in — 2026-10-29

## Reopening an existing manual sign-in window

The login button previously reported `PROFILE_BUSY` for an existing human sign-in
owner rather than reopening its browser. The user reported an invisible chat
window; process metadata showed Node PID 2748 and normal Chrome PID 8568, both
created at 21:14. Later both exited, leaving a stale app-owned lease; ordinary
lease acquisition recovers it without deleting browser session data.

Repeated sign-in now distinguishes an actual manual owner from automated work.
On Windows it reads only the exact lease PID's process metadata and accepts only
`node.exe <this runtime's control-web.mjs> <matching root> <matching provider>
--login`, with no extra arguments. It also rejects a live CDP listener and checks
the lease PID/token again. Only then does it forward a normal `--new-window`
launch to the existing manual browser, preserving its original exclusive lease.
The observer waits for that lease/owner to finish; it never releases another
owner's lock, reads authentication pages, copies credentials or sends prompts.
Unknown and automation owners remain blocked. Runtime changes apply on the next
button press; the GUI binary and worker did not change for this fix.

Tests cover exact-owner validation, matching provider, repeated opening with lease
preservation, unknown/automation rejection and live-CDP rejection. The full runtime
regression passed 68/68 in `.runtime/login-reopen-regression.log` (151 seconds).
After the requested user button press, process metadata showed a new manual
owner Node PID 25960 at 21:36:37 and Chrome PID 12684 at 21:36:39, using the
interactive profile without CDP. The production owner verifier returned true for
that actual Node process. Browser process launch is confirmed; visible-window
confirmation and an authenticated model turn are still pending. Reopening a
concurrently held manual lease was verified with a fixture, not a real account.

## Signed-in browser versus open-window state

The user has now reported successful Gemini sign-in and supplied a screenshot
showing the authenticated Gemini workspace. This is user/screenshot evidence;
Concat has not yet verified an authenticated model turn. The campaign UI's old
"waiting for login" text incorrectly described browser-process lifetime as auth
state. It now says the login window is open and explains the close-to-handoff step.
No click or window close is treated as verified authentication.

The user then closed the window but Kedi remained busy. Read-only process
inspection found app-owned normal Chrome PID 22848 still alive (no CDP), so
waiting for process exit also waits for background apps. Future manual launches
include `--disable-background-mode`, a browser process-lifetime option documented
in [Chromium source](https://chromium.googlesource.com/chromium/chromium/+/master/chrome/common/chrome_switches.cc).
It does not alter authentication checks. The already-running old process is not
terminated; the user must exit that instance normally once. Session data remains
browser-owned. No automatic replay of existing campaign requests was performed.

Focused login/control tests passed 15/15, including the background-mode argument
regression (`.runtime/login-window-status-tests.log`). Native label/message build
verification passed: build 3m40s, `--version` and `--smoke` exit 0, including the
real three-provider login callback bridge probe. Published launcher target:
`src/target/debug/concat-ai-control-e39b3505f1c0.exe`, SHA256
`e39b3505f1c0b90151c0823242fd5a60e091067cc07d9393af799b9a0b57f556`.
Logs use `.runtime/login-window-status-{native-build,version,smoke}.log`.
The worker is unchanged. Real browser exit/handoff remains pending a user retry
with the updated launch arguments. PID 22848 was still alive after verification
and was not forcibly closed.

The previous `--login` branch reused `launchBrowser`, so even manual sign-in
opened an active CDP session. The reported Google rejection is an authentication
failure, not proof of a wrong password or a successful saved session.

The new login branch launches the installed Chrome/Edge as a normal visible
browser process with the existing app-owned profile and provider start URL.
It does not enable remote debugging, attach to pages, inspect authentication
content, automate credentials, spoof browser identity, or change security settings.
Sign-in is performed by the user. Closing that browser emits an unverified
`login_closed` event; the subsequent AI task still checks authentication before
sending a prompt and retains the existing rejection/unknown-outcome handling.

Manual sign-in and controlled tasks now take an exclusive app profile lease.
Controlled tasks cannot attach to the profile while sign-in is open. A legacy
already-open profile that forwards the launch into an existing browser is reported
as busy rather than as a successfully opened sign-in window. Profile/session
data remains browser-owned and is not copied between profiles.

Native error events distinguish an open profile, missing browser, launch failure,
and abnormal close. The ordinary Chrome profile used outside Concat is unchanged.

Google guidance: [supported browsers](https://support.google.com/accounts/answer/7675428?hl=en)
and [browser-based authorization requirements](https://developers.googleblog.com/guidance-to-developers-affected-by-our-effort-to-block-less-secure-browsers-and-applications/).
This implementation does not guarantee that Google or another provider accepts
an account or permits subsequent automated access; rejection remains blocking.

## Checks

`node --test browser-login.test.mjs control-web.test.mjs
control-web-browser.test.mjs web-chat-browser.test.mjs` passed 50/50 tests.
The new tests cover launch flags/URL scope, user-owned browser lifetime, session
file preservation, concurrent profile exclusion, early/reused-process rejection,
spawn failure, stale lease recovery, and fail-closed corrupt lock data.
These are local fixtures, not a successful real-account authentication test.

Sources: `tools/ai-edit-mcp/{browser-login.mjs,browser-login.test.mjs,
browser-profile-lock.mjs,browser-cdp.mjs,control-web.mjs}` and
`src/crates/concat/src/studio_ai_control.rs`.

## Published build

The native build, version check and isolated smoke check all completed with exit
code 0. The smoke log also confirms the three-provider secondary-window callback
bridge. Real Google account sign-in has not been verified by these checks.

- Revision: `Normal browser AI sign-in / 2026-10-29`
- Executable: `src/target/debug/concat-ai-control-11c490e5e2a4.exe`
- SHA256: `11c490e5e2a4bf65f6ad052693c3be7b0122ac95c98fc61b3e5c012b3ed78c86`
- Logs: `.runtime/manual-ai-login-{native-build,regression,version,smoke}.log`

Close the existing rejected Concat browser window and restart Kedi through
`run-concat.bat`. Choose the service and sign in manually in the new normal
browser window, then close that window before starting controlled AI work.

## Legacy browser reuse found on the user's machine

Read-only process metadata confirmed that the rejected Gemini window was still
owned by Chrome PID 20744, created at 20:03, with remote debugging enabled, even
after Kedi had been upgraded. Closing the visible window did not end that legacy
profile process. A new launch into the same Chrome profile was therefore reused
by the old process and reported as busy.

Manual sign-in now uses the fixed app-owned path
`ai-control-web/interactive-v1/<browser>/<provider>`. The same provider's controlled
work switches to that profile after the user opens manual sign-in. Providers whose
working legacy sessions were not migrated retain their previous profile. Session
data is not copied or deleted, and profiles do not rotate on repeated failures.

Sign-in preflight also checks only the local liveness metadata of an existing CDP
listener and refuses reuse before launching. It sends no CDP commands and does
not inspect the authentication page. New tests cover this refusal and the stable,
provider-specific migration path. This runtime-only update does not require a
native rebuild. Real account acceptance remains unverified until the user retries
in the newly created normal browser.

After the runtime migration, read-only process inspection confirmed a new Chrome
root process PID 18300 created at 20:37, parent Node PID 23308, using the
`interactive-v1` profile. Its command line had neither remote-debugging nor
enable-automation flags. The legacy PID 20744 remained separate with CDP enabled;
it was not terminated or reused by the new flow. Windows UI automation was
unavailable (`native pipe is unavailable`), so the user opened the new sign-in
window through Kedi. Account acceptance is awaiting the user's sign-in result.

The isolation regression suite passed 52/52 tests. After adding the legacy-session
compatibility selector, the focused login/control suite passed 15/15 tests. Logs:
`.runtime/manual-login-isolation-regression.log` and
`.runtime/login-profile-migration-tests.log`.
