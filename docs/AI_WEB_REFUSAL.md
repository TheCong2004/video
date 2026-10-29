# Web model refusals — 2026-10-29

The user supplied Gemini's actual answer: “Tôi không thể trợ giúp về điều đó,
vì tôi chỉ là một mô hình ngôn ngữ.” This is a refusal, not a campaign plan.
The saved turn `plan-3f842f59c0e52e1b959e14828a4c2986` had dispatched once,
observed one bound assistant reply with `streaming:true`, and then timed out.
No raw answer was saved, so the user's pasted answer supplies the missing evidence.

`control-web.mjs` now recognizes a bounded, stable refusal in the exact submitted
turn's final message (or a completed copyable reply). It raises `MODEL_REFUSAL`,
records `model_refusal` as the receipt reason and displays a provider-specific
Vietnamese message. A refusal is never parsed into executable plan steps. The
existing post-dispatch failure guard still prevents automatic resubmission.
An unrelated reply or thoughts-only refusal is insufficient evidence.

The response reader now ignores hidden/disabled stop controls and busy flags
inside thoughts panels. Previously those signals could hold `streaming:true`
after final content arrived. This mechanism is covered by DOM fixtures; the
actual failed page's DOM was not captured, so its precise stale signal is not
claimed as proven. Trace metadata now includes final-content and completion
counts, without logging response bodies or credentials.

Changed runtime: `tools/ai-edit-mcp/control-web.mjs`.
Tests: `tools/ai-edit-mcp/control-web-browser.test.mjs`.
Five focused browser tests passed, including the user's exact refusal, unrelated
turn rejection, stale thoughts/hidden stop controls and active streaming.
The broader Control reader/DOM regression passed 36/36 in 115.6 seconds, covering
ChatGPT, Gemini and Claude. These are intercepted local fixtures, not model calls.
Logs: `.runtime/gemini-refusal-focused.log` and
`.runtime/gemini-refusal-regression.log`. This runtime update needs no native
rebuild; no existing failed request was replayed and no real model call was sent.
