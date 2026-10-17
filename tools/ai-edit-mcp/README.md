# Concat AI Edit MCP

MCP stdio adapter for the currently open Concat window. Windows local-user
DPAPI connection; no model key or cloud login is copied into Concat.

In AI Edit choose **AI bên ngoài → Bật MCP local → Lưu cấu hình MCP**.
Merge the saved `mcpServers.concat` entry into a compatible local client's
configuration (for example Gemini CLI), then reconnect the client. Do not
overwrite unrelated settings. Keep Concat open; after disabling/re-enabling
MCP, save the new per-run configuration again.

Tools: `concat_get_context`, `concat_get_status`, `concat_propose_edit`.
There is no approve, execute, arbitrary file read, shell, save or export tool.
Proposals are compiled by the native editor and require human preview/approval
in Concat. Undo reverses the whole approved draft.

This is not a remote HTTPS MCP service for ChatGPT web. Use the context-file
export/JSON draft import route for web AI. External clients use their own
account/model permissions and quota; MCP does not supply a free model.

Development: Node >=18, `pnpm install --frozen-lockfile --ignore-scripts`,
`pnpm test`. `.npmrc` uses hoisted dependencies so the Windows package can copy
real module directories without junctions or the pnpm virtual store.

See the bundled `AI_EDIT_EXTERNAL.md` for usage, tests and limitations.
