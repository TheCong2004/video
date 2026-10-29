# AI Control implementation plan

Updated 2026-10-28. The master prompt remains the acceptance scope; the following is delivery tracking, not a reduction of that scope.

| Phase | Implemented | Remaining acceptance |
|---|---|---|
| 0 Audit | Crate/service audit, generated callback/API/MCP inventory, key live-bridge gap | Runtime trace and evidence for every callback |
| 1 Shared control | Registry, real native executors, UI/MCP shared owner, existing editor commit reuse | All project, timeline, audio, caption, effect, Film and export operations |
| 2 MCP | Additive capability/status/submit with output schemas and contract checks; original tools preserved | Full business-tool coverage and native execution checks for every adapter |
| 3 Orchestrator | Persisted DAG, receipts, dependencies, scopes, approval, bounded retries, crash Unknown | General AI planner, deadlines, richer artifact recovery and accounting |
| 4 Native UI | Command/script/assets/mode, workflow monitor, history, favorites, JSON step correction/checkpoint invalidation, real callbacks | Project selector, templates, richer progress and provider chooser |
| 5 Providers/Codex | Existing AI connection service reused; isolated Codex protocol health adapter | Runtime login, model/tool turn loop, external MCP client and explicit browser fallback |
| 6 Human control | Existing revision-bound edit validation, real executor pause/cancel, approvals | Protected scene fingerprints, manual diff/replan/continuation |
| 7 Acceptance | Core/protocol tests, MCP/browser regressions, native adapter compile, real FFmpeg and persisted render integration | Full native build and real A–D end-to-end runs |

Next: finish the full native build and verify native behavior; then extend live project and export services through their actual owner. Duration enforcement and safe step-input correction are now implemented and checked at their current boundaries. Do not add fake tools to improve matrix coverage. Native sources are frozen during the expensive full Windows build; verify the resulting executable and record exact evidence.

Existing project/import/edit/undo/redo/save/reopen/export services now pass a real-media headless API integration. They remain separate from the GUI control gap. The next owner adapter should use `Shell` on the UI thread and the live `Session`, share the existing `Exporter`, and take an immutable export request from that same session. Do not open a second editable project copy to bypass the missing production `LiveSessionBridge`.

Remaining orchestrator work also includes an exhaustive intent/plan check: the current bounded keyword planner is not suitable for arbitrary multi-module production commands. Unsupported intent must be rejected or reported as a missing capability; a registered plan's success must never be presented as proof that an unrelated natural-language request was fulfilled. Current per-step receipts are current-attempt state with a short activity trace; append-only receipt history still needs implementation.
