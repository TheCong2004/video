# Dependency closure and execution order

```text
untrusted provider structured response
  -> concat-agent EditPlan (bounded, versioned, seconds)
  -> exact PlanningSnapshot of GUI-owned Session
  -> sequential alias/reference validation on native scratch Editor
  -> PreparedDraft (opaque owner identity + mutation revision)
  -> human permission [UI integration pending]
  -> original Session.commit_draft
  -> one native Command::Batch + undo snapshot
  -> Studio.after_change [integration pending]
  -> existing native preview/save/export
```

The scratch project is disposable and read-only to preview consumers. It is
never a second authoritative timeline. Its ID mint includes reservations left
behind by undo. PlanningSnapshot can move to a worker without holding the live
Session throughout inference. Approval checks identity AND revision, including
edit -> undo ABA; no document fingerprint substitution.

## Source closure mapped to target

| Source closure | Native target/reuse | Remaining work |
|---|---|---|
| OpenChatCut schema/policy/executor/proposal/session/store | concat-agent + native Editor/Session | Structured provider tools, permission surface, budget, durable run recovery |
| WeftCut actor/history/checkpoints/lease/activity/MCP | Same editing boundary, native undo, GUI event loop | Named checkpoints, scoped lease, activity, authenticated live-session bridge |
| Cartcut registry/skills/geometry/audio/text/keyframes/compositor | Native Command + concat-render/text/effects/export | Full skill vocabulary, style profiles, composite contact sheets |
| OpenChatCut ASR/VAD/beats/scenes/semantic/reframe | concat-speech/media/vision, native source-time maps | Word alignment, versioned intelligence cache, tested plan producers |
| Velorn workflow/dependencies/Comfy/jobs/assets/sidecars | Existing Promax jobs/artifacts + Concat Media | Capability graph, cancellation/unknown outcomes, checksummed import/assembly |

## Ordered phase gates

1. Safe native draft and atomic ID/history foundation.
2. Structured LLM -> EditPlan -> real draft preview -> approval -> GUI Session
   -> undo/save/reopen/export. Real login gates only this real-provider acceptance;
   independent later phases must continue with native/fixture tests.
3. Safe sessions/checkpoints/permissions/activity/recovery.
4. Complete Cartcut skills and composite contact sheets.
5. Word-level media intelligence and derived plans.
6. Authenticated MCP routed to the same GUI-owned service, no duplicate Session.
7. Generation graph/capabilities/jobs/verified artifacts -> existing Media ->
   approved native timeline batch.
8. Native GUI E2E and prescribed full acceptance scenarios.

Native check/build remains on D. No deletion of source clones, model data,
existing user settings or caches. No standalone browser/Electron editor.
