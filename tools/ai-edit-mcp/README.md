# Concat AI Edit: connected chat browser and MCP

## AI Control workspace (current source; desktop build pending)

Open **AI Control → Phòng điều hành AI** for the dedicated task workspace. It
shows real persisted steps as numbered blocks, with dependencies, readable states,
pause/resume and results. Raw tool JSON is under **Chi tiết công cụ**.

Choose **Dùng AI trên web**, then ChatGPT/Gemini/Claude and explicitly allow AI
access. Control opens its own browser profile, waits for manual login if needed,
types one correlated request, clicks Send once and reads the matching structured
reply. `ai.plan`, `ai.script` and `ai.edit` use this transport; validated native
executors apply project changes. This is not an arbitrary website click agent.
API connections remain an alternative. Existing connected-editor web chat below
is preserved and uses a separate owned profile.

The provider is fixed after a model dispatch. A waiting workflow that has never
dispatched AI can change sources through revision-checked input updates, which
invalidate old approvals. An uncertain send is not automatically replayed.
Tests use intercepted local browser pages, not a live account or model inference.

**Chat web tự động → chọn ChatGPT/Gemini/Claude → Mở chat & kết nối project**
launches an owned Chrome window (Edge fallback only if Chrome is absent) with a dedicated persistent profile per browser/provider. Log
in there once and type in the chat. On your Send click, the browser adapter
attaches current timeline IDs/timing and the native schema, then reads the
correlated JSON response and submits it to Concat. It does not install global
MCP settings or require an API key. The same native scope/schema checks and
undoable commit apply. Disconnect or close Concat to revoke the bridge.

The browser adapter uses the public page DOM, not a provider's official MCP
integration. Login, quota and page changes can prevent it from working. It
never bypasses login/CAPTCHA and does not read cookies or private endpoints.
Only the dedicated window is controlled. Arbitrary existing browser tabs are
not attached. Timeline context does not contain video frames/audio.

The current ChatGPT guest/mobile textarea and `data-message-role` response
markup are supported alongside the desktop editor. Connected status is emitted
only after a DOM readiness + native context round trip. Informational questions
use a message response and never create an edit. A page navigation rehydrates
only the pending request ID/scope; it never replays the prompt or edit.

MCP stdio adapter for the currently open Concat window. Windows local-user
DPAPI connection; no model key or cloud login is copied into Concat.

For a separate MCP client choose **Nâng cao: dùng MCP client riêng → Bật MCP local**, then
save the generated config and add the `mcpServers.concat` entry to a compatible
local MCP client. Keep Concat open. The prompt is entered in that AI client,
not duplicated in Concat. After disabling/re-enabling MCP, save the new per-run
configuration again.

Tools: `concat_get_context`, `concat_get_status`, `concat_propose_edit`.
There is no arbitrary file read, shell, save, export or separate approve tool.
When MCP is enabled, Concat turns on auto-apply by default. The native editor
validates every proposal against the live project revision and strict edit
schema before applying it as one undoable change. Turn off **AI tự áp dụng sau
kiểm tra** in Concat to stage drafts for preview/approval instead. Undo reverses
the whole applied edit.

This stdio server connects compatible local MCP clients; it is not a remote
HTTPS MCP endpoint for ChatGPT web. External clients use their own account/model
permissions and quota; MCP does not supply a free model. ChatGPT Desktop local
plugin support depends on the ChatGPT client/account configuration; if that
client does not accept local MCP tools, use a supported local stdio client or
the dedicated browser adapter above.

Development: Node >=22 for the browser adapter, `pnpm install --frozen-lockfile --ignore-scripts`,
`pnpm test`; `pnpm test:browser` verifies the real installed browser/CDP against fulfilled local
page fixtures for all three adapters without a provider call. The
`browser-native-smoke` Rust example verifies the browser response automatically
commits a native trim+title exactly once and one Undo restores the project.
`.npmrc` uses hoisted dependencies so the Windows package can copy
real module directories without junctions or the pnpm virtual store.

See the bundled `AI_EDIT_EXTERNAL.md` for usage, tests and limitations.
