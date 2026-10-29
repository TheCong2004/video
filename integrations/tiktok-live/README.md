# Concat-owned LIVE controller

This controller is maintained in Concat, derived from the staged modified
TikTok-Live-Connector 2.5.0 controller (AGPL-3.0-only; see the staged LICENSE).
Its historical `ArtCraft` symbol names are retained for protocol compatibility;
building and running this override does not use a sibling ArtCraft installation.

The SDK emits `{ info, exception }` for both per-packet processing failures and
transport failures. The old controller unconditionally published `degraded`,
disabling native AI replies even with `isConnected === true`. Error-only actual
loss also had no reconnect path. Concat now preserves healthy transport state,
emits only a sanitized warning, and schedules recovery on an explicitly lost
connection. Reconnect attempts are serialized, bounded and cancelled by stop.
Both `stream_end` and the production SDK's `streamEnd` prevent reconnecting an
ended broadcast. Event deduplication survives recovery.

## Verify and stage

Node 22.13+ is required for the build-time TypeScript transformer. The emitted
runtime remains JavaScript and adds no new runtime package dependencies.

```powershell
node --test integrations/tiktok-live/controller.test.mjs
node scripts/patch-live-controller.mjs
node --check .deps/tiktok-live-connector/dist/artcraftLiveRuntime.js
# Optional: joins a currently public LIVE for 45 seconds; no posting or AI calls.
node scripts/live-readonly-watch.mjs voaioh
```

The patcher replaces only the controller's marked bundle region and updates the
shipped corresponding TypeScript source. It fails closed if the bundle markers
are absent. Both staging and Windows packaging invoke it so restaging cannot
silently remove this fix. The protocol implementation comes from the staged
corresponding source and remains unchanged.

2026-10-19 verification: six controller regressions passed; native supervisor
suite four passed (two opt-in checks skipped). The built JavaScript connected
read-only to `@voaioh` for 45 seconds and received five chat events, then shut
down cleanly. This is a short acceptance check, not a long-term uptime guarantee.
No user comment contents, account tokens or credentials were logged by the probe.

The running app's existing child may have the old controller loaded in memory.
Save and restart Concat: the ordinary Disconnect button only stops the session
and retains that Node process. This controller-only fix does not require
rebuilding the native EXE.
