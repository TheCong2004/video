# Visible Control actions

The `ui.action` executor uses the main application's existing Slint controls.
It does not send desktop-wide mouse/keyboard input or operate other processes.
It supports only `open_factory`, `open_film`, and `film_idea` with a supplied value.
Each step finds one visible, enabled control, scrolls to its current geometry,
shows the target, then dispatches real pointer events. Field changes use the
control's accessibility SetValue action and verify value readback.

The guard appears only during actual main-app UI operations. It blocks pointer,
wheel, keyboard, IME, touch and dropped-file input for the main window ID. It
leaves the AI control window and login browser usable. Stop/takeover hides the
guard immediately and requests the existing runtime pause. Cancellation is
checked again before dispatch. A failed/unknown write is not replayed automatically.
The actor also refuses a write if its guard is unavailable, including compatibility
configurations where the separate control-window timer is not installed.

Existing media render and editor services remain adapters; they are not replaced
with a pretend cursor animation. The actor does not promise that it can operate
every app button or synthesize an animated film. Plan compilation refuses missing
capabilities and shows actual model questions as input needed.

Web reference: `chromex/apps/tobyflow/src/platforms/labs.toby.vn/chat-content-chatgpt.ts`,
especially snapshotConversationState, isStreaming and waitForTextResult. Concat's
independent implementation uses public DOM/CDP reads in its owned browser. It does
not copy credentials, private endpoint hooks or reference-server calls.

Web receipts live in the existing application data root under
`ai-control-web/turns/<request-id>.json`. They contain bounded status diagnostics
and, on success, the correlated result. Native recovery requires matching request,
provider, completed/dispatched state and model. A prior unknown outcome without a
completed matching receipt cannot be replayed or declared successful.

Reproduce headless component verification with `scripts/test-visible-ui-memory.ps1`.
It needs the existing native build dependencies and creates no desktop window.
See `AI_TEST_RESULTS.md` for desktop permission denial and acceptance limits.
