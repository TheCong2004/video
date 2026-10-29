# AI web login button bridge — 2026-10-29

The secondary Control window exposed `AiControl.web-login` but omitted it from
the callbacks forwarded to the main application's existing AI owner. Clicking
the Campaign Manager login button therefore did not invoke the browser launcher.

`studio_control_window.rs` now registers that forwarding callback through the
shared proxy binder. The browser login worker in `studio_ai_control.rs` publishes
ready/completion/error updates through `upgrade_in_event_loop`, including clearing
the busy state when the login window closes or fails.

Debug native smoke checks now invoke the actual proxy binder using separate
hidden UI component instances. For ChatGPT, Gemini, and Claude they verify that
the selected provider reaches the owner callback and that busy/status changes
return to the secondary window. The probe substitutes only the owner callback;
it does not open a browser, authenticate an account, or call a model.

The existing `control-web.test.mjs` and `control-web-browser.test.mjs` suite passed
all 33 tests, including pre-login refusal, rejected OAuth, and response recovery.
These fixture tests do not demonstrate successful login to a real AI account.

Changed source: `src/crates/concat/src/studio_control_window.rs`,
`studio_ai_control.rs`, and `lib.rs`.

## Published verification

The final native build succeeded. The isolated native smoke check logged
`AI web login bridge: 3 providers dispatched and status mirrored`, followed by
`smoke: ok`, and exited with code 0. The version check also exited with code 0.

- Launcher revision: `AI web login button bridge fixed / 2026-10-29`
- Executable: `src/target/debug/concat-ai-control-efd4ac6f371b.exe`
- SHA256: `efd4ac6f371b6e5f4a3643b4061bc8a4d2b06bddb76f059ed3cd577f14f77b21`
- Evidence: `.runtime/web-login-bridge-{native-build,js-tests,version,smoke}.log`
- `run-concat.bat` selects the new verified build; the previous verified build is
  retained through the prior receipt for rollback.
