# Windows paths in model JSON — 2026-10-29

The user's later Gemini screenshot shows a complete campaign envelope with the
matching request ID, but a raw Windows path containing single backslashes in its
`providers.assets` value. The corresponding receipt
`plan-bb03baf44f6ed5995bcb590d8f9f65e0` confirms a bound final response with
`streaming:false`, `completeCount:1`, `finalContentCount:1`, `parsed:false`, then
timeout. This is a different failure from the earlier model refusal.

The transport now extracts bounded, absolute drive-letter Windows paths from JSON
string values in the original native prompt. For a reply bound to the exact
submitted user turn, only a complete string value that exactly matches one of
those known paths may have its JSON escaping restored. Properly escaped paths
and all unrelated text remain unchanged. This also preserves literal `\t`/`\n`
in known paths rather than turning them into tabs/newlines. No generic JSON repair,
invented path, evaluation, nonce relaxation, Page authority change or template
provider substitution is introduced. Native plan validation still runs afterward.
The outgoing request also explicitly asks the model to preserve JSON path escaping.

Changed files:

- `tools/ai-edit-mcp/control-web.mjs`
- `tools/ai-edit-mcp/control-web.test.mjs`
- `tools/ai-edit-mcp/control-web-browser.test.mjs`

Tests include the screenshot's filename shape with Vietnamese text, unchanged
ordinary newline escapes, already-valid JSON, ambiguous valid JSON path escapes,
unknown paths, wrong request ID, incomplete JSON and missing DOM turn binding.
The intercepted Gemini DOM test returns the malformed path, reads the corrected
nine-video result and asserts exactly one Send. Two focused tests passed. An
initial fixture had incorrectly encoded its own Vietnamese path; it was corrected
and the focused fixture passed. Final Control parser/DOM regression passed 38/38
in 104 seconds across Gemini, ChatGPT and Claude using intercepted local fixtures.

Logs: `.runtime/gemini-windows-json-focused.log` and
`.runtime/gemini-windows-json-regression-final.log`. No real model request was sent
or replayed during verification. The runtime update applies to the next request;
no native build is needed. Existing failed receipts are left unchanged.
