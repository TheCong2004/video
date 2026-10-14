# AI port source inventory

Recorded 2026-10-14. Source clones are read-only; no setup/install scripts,
model downloads or source test suites were executed. Target baseline was clean.

| Repository | Local path | Inspected revision | License |
|---|---|---|---|
| Concat target | `D:/capcutpolot/concat` | `067f1e268e59c34e507d808e40349ccc3cb76b81` follow-up baseline | AGPL-3.0-or-later + existing plugin exception |
| OpenChatCut | `D:/capcutpolot/OpenChatCut` | `2e6f4a29b784200cfb32b859b16f2561592bc3e8` | AGPL-3.0-or-later |
| WeftCut | `D:/capcutpolot/WeftCut` | `a542d6d241897c1a8a230e332948e337039dc322` | MIT |
| Cartcut | `D:/capcutpolot/cartcut` | `6dd7591899d13da090c4588d2d27f50a25fbc19b` | Root MIT; package does not declare it |
| Velorn | `D:/capcutpolot/velorn` | `b3c02adbd5c0779feb8abf34978299e9502cc0e7` | GPL-3.0-only |

## OpenChatCut

- Agent contract/policy: `src/agent/tool-schema`, `tool-activation`,
  `execution-policy`; server `agent-runs/tool-policy`, `executor`,
  `acceptance-loop`, durable store/recovery/settle/SSE modules.
- External editing: `external-edit-session`, `external-proposal-apply`,
  `proposal-commit`, draft editor, project/version/proposal persistence.
- Important behavior: revision-bound drafts; cancellation and stale checks
  around persistence; capability/session-scoped claim/result ledgers; uncertain
  paid generation/export outcomes must not be resubmitted blindly.
- Transcript: local/native/cloud providers, word identity, source-time mapping,
  cut padding, caption projection. Silence RMS candidates are not authoritative
  destructive VAD. Beats use detected onsets; scenes use bounded FFmpeg work;
  semantic search needs versioned embeddings/source hashes, not just filenames.

## WeftCut

- Current mutation authority is `main/state/actor`, not the retired native
  agent session (ADR0065). Read `main/mcp`, actor commands/results/history,
  mutations/model/validate/IDs/canonical, agent activity and renderer stores.
- Dry run executes sequential scratch mutations; locks/checkpoints/session
  ownership/activity need independent enforcement, not a UI-only flag.
- Source dry-run ID generator can advance; target instead isolates the exact
  native mint. Connection cleanup releases only its own lease.

## Cartcut

- Registry/bridge imports real command families. Editing depends on clip,
  geometry, audio, merge, hierarchy, speed and track operations, transactions,
  read/context handlers, text metrics, presets/keyframes/transitions/effects.
- `apply_edit_plan`: cuts -> motion -> transitions -> captions/titles -> effects.
  It consumes a plan; it is not itself an LLM creative planner.
- Units differ: source timeline ms -> native seconds; source neutral scale 10
  -> native 1; opacity 0..100 -> 0..1; keyframes absolute -> clip-relative.
- Contact sheets use the real compositor and cleanup decoder scopes. Raw
  footage thumbnails are not equivalent.
- Do not copy weak source no-op, batch-continue or MCP sender checks. Target
  must reject missing references and abort the entire proposal on any refusal.

## Velorn

- Guided creators delegate execution to `GenerateWorkspace.jsx`, workflow
  registry/config/JSON, dependency/setup checks and ComfyUI integration.
- `flowAiSchema` and `flowAiRuntime`: typed ports/topological ordering, prompt
  rewrites, queue/poll/download/import; auto-import/assets/project/file IPC and
  sidecars supply provenance and cache behavior.
- Guided short-film brief copy/paste is not fully automated LLM generation.
- Gaps not to inherit: global interrupt cancelling another job, unsupported
  cancellation, estimated progress presented as measured, download URL counted
  as verified import, swallowed project-save failures, mtime-only deduplication.

## Target reuse

`concat-project::Editor` owns native commands/mint/history. `concat-host::Session`
owns the open document/save/flattening. GUI `Studio` owns the live Session and
after-change updates. `concat-api::Api` separately owns headless Sessions and
refuses Window-owned projects: GUI MCP therefore needs a live owner bridge,
not another `Api::open`. Studio provider credentials stay server-side.
