# First vertical slice acceptance

This checklist is intentionally not marked complete. Automated native fixture
coverage does not prove a working real-provider GUI session.

## Automated checks (D: output and temporary files)

From `D:/capcutpolot/concat`:

```powershell
.\scripts\windows-studio.ps1 -TestAI
.\scripts\windows-studio.ps1 -TestAdapters
.\scripts\windows-studio.ps1 -CheckOnly
```

`-TestAI` covers 121 project, 6 plan and 6 host Session tests. The native render
test creates its own MP4, composites a timed title over it, checks three times,
exports MP4, saves/reopens and reverses the whole edit by undo. It does not skip
successfully when encoding/rendering fails. Fixtures live under the D: temporary
root and close their encoder/idle readers before cleanup on Windows.

`-TestAdapters` currently gives 77 passed, 1 ignored external live smoke. No
paid provider invocation occurs in these tests. Backend edit-plan tests use
mocked catalogue/completion fetch and cover refusal, malformed/stale receipts,
abort, capability gate, actual usage and no uncertain retry.

## Native GUI acceptance still required

1. Run dev with `.\scripts\windows-studio.ps1` (not a release/package build).
2. Open a disposable project and import real footage. Keep a copy outside the
   project if needed; do not use a user's only copy as the test fixture.
3. In a native editor pane menu choose **AI Editing**, or add that pane. It is
   inside the actual editor, not a separate webview or launch-screen editor.
4. Connect Promax through Studio settings with the user's own login. No browser
   cookie extraction or invented token. Load configured models. Capability
   listing alone is not proof that a key can invoke a model.
5. User has authorized exactly one short real request. Use the configured
   structured-capable model and ask for one small edit such as a two-second
   title. Do not automatically choose an expensive substitute or retry unknown
   outcomes. Report Unsupported honestly if metadata does not establish support.
6. Confirm live timeline and undo history are unchanged while planning.
7. Read the actual native diff, choose a time within the changed span and render
   before/draft. Confirm composited title/media pixels match the proposal.
8. Approve once; confirm timeline changes, monitor updates and undo reverses
   all operations as one step. Redo restores the same state.
9. Save, close/reopen and compare native timeline and picture. Export and play
   the actual resulting file, including audio/titles/effects where applicable.
10. Generate another **mock** proposal or use an isolated fixture to test stale
    edit->undo, project switch, cancel, duplicate click and rejected/no-op plans.
    No additional real paid request is authorized by the current consent.

Evidence should identify app/source revision, fixture path, operations, provider
outcome/actual usage when supplied, observed timeline/undo/save/export and
screenshots without credentials. Only then mark the corresponding GUI features
E2E_VERIFIED. The present machine has no pywinauto/pytest installation, so no
desktop automation run has been represented as passed.

## Current external readiness

Local Promax health200 was verified after the user approved starting it. Native
Concat session storage for the local origin was absent in both standard and
debug portable locations, and there was no explicit pilot token. User login is
therefore needed for the authorized real call. No real inference was sent.
