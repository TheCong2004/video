# AI Control architecture

Status: partial implementation, 2026-10-28.

```text
Native AI Control page ─┐
Existing editing MCP ──┴─> single workflow owner
                           ├─ registry / scopes / approval digests
                           ├─ durable DAG and dispatch receipts
                           └─ actual executors
                               ├─ native media probe
                               ├─ saved-video library
                               ├─ existing AI connection client
                               ├─ existing Factory renderer
                               ├─ actual artifact probe + SHA256
                               └─ UI-thread strict EditPlan commit
```

The core lives in `concat-studio/src/ai_control.rs`; the native adapter is `concat/src/studio_ai_control.rs`. The native page uses existing Slint Studio controls. UI and MCP share a single in-process workflow owner and the same persisted receipts. Existing media engines and editor history remain authoritative.

Workflow inputs, declared dependencies and registered tools are validated before acceptance. Only dependencies may supply output references. A dispatch receipt is saved before an executor runs. An interrupted Running step reloads as Unknown. Unknown writes are never automatically replayed. Read-only/replay-safe retries are bounded. Atomic saves and a Windows exclusive owner lock prevent two runtime owners from writing the same store.

Full Auto executes within operator-granted scopes. Supervised requires approval for writes; Manual stages work for explicit approval. Approval hashes bind the exact resolved input. Edits must pass the existing strict EditPlan compiler and scope/revision check. A manual project change makes an older plan stale rather than overwriting it.

The store is under the application's `ai-control` data directory. Task outputs get fresh attempt directories; attached paths are explicitly authorized. Model output cannot select arbitrary filesystem roots. Network permission and MCP Control permission originate in native UI. New MCP methods cannot approve, grant permissions or change apply policy. Long work stays off the UI thread.

Pause/cancel asks the actual executor to stop and waits for its receipt; it does not fabricate completion. A remote model request that cannot be interrupted is awaited before dispatch stops. Receipt ambiguity is exposed, not retried as a new paid call.

## Codex runtime boundary

The first Codex adapter checks `app-server` initialization and `account/read` in a separate runtime home. It clears inherited provider credentials and never borrows the development Codex login. Server requests are refused during this probe, and no model turn is started. A healthy protocol or authenticated account does not enable execution.

Protocol details were checked against installed CLI 0.164.0-alpha.11.1 generated schemas and the [official app-server documentation](https://learn.chatgpt.com/docs/app-server). An execution adapter still needs bounded dynamic-tool dispatch, approval/cancellation handling, turn accounting and a runtime-owned login flow before it can run workflows.

## Remaining design work

The implementation does not yet expose every Concat business operation. General provider planning, live project lifecycle/export, external MCP clients, per-step deadline enforcement and protected manual edits remain open. Per-step JSON correction now invalidates dependent checkpoints and previous approvals; unknown outcomes must be reconciled first. These are tracked in `AI_IMPLEMENTATION_PLAN.md`; no placeholder capability is advertised for missing executors.
