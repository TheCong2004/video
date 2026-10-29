# Kedi local operator runbook

## Verify and replay without internet, GPU or accounts

From the Concat repo root:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/kedi-live-verify.ps1
```

This runs serial Studio tests, compiles examples and replays the canonical comment/product fixtures. Evidence is written under `.runtime/kedi-live-verification/`. The replay explicitly labels speech completion `simulated`. The accelerated 7200-step soak test is not a two-hour GPU/media soak. Add `-CleanBuild` after all desired desktop checks to remove the verified Cargo `src/target` output.

## Native panel

Set `KEDI_LIVE_ENABLED=true` in the process that launches Kedi, then open the existing LIVE page. The Kedi panel offers start/pause/resume, manual takeover, STOP, product introduction, comment simulation and manual speech. Flags for real speech, commerce, gestures and platform writes remain off unless explicitly enabled. Mock product answers state that their source is mock.

Takeover stops automatic responses; use Continue to resume. STOP is terminal for that session; New Session creates a new ID. Viewer comments cannot invoke operator commands. The legacy automatic reply mode is excluded while the Kedi owner is active, preventing two automatic speech writers. Existing manual connector controls retain their existing explicit UI behavior.

Native checkpoints are scoped to tenant/store under the normal Concat application-data `kedi-live` folder. Checkpoints contain status metadata, no raw comments, tokens, audio or reply text. Restoring a nonterminal checkpoint enters RECOVERING with automatic speech disabled. A human must explicitly Start after preflight. An unconfirmed speech request is never replayed after restart.

## Real local TTS/avatar preview

Explicit process settings:

```text
KEDI_LIVE_ENABLED=true
KEDI_OT_ADAPTER_ENABLED=true
KEDI_TENANT_ID=<configured-tenant>
KEDI_STORE_ID=<configured-store>
KEDI_ASSET_ROOT=<approved-local-clip-root>
KEDI_AVATAR_MANIFEST=<avatar-json-file>
KEDI_VOICE_ID=<configured-existing-voice>
KEDI_MOTION_ENABLED=false
KEDI_PLATFORM_WRITE_ENABLED=false
```

Prepared clips are 1–10 second local MP4 files accepted by the existing Concat decoder. The avatar manifest must include a nonempty consent reference, commercial permission, future rights expiry, an idle looping clip and SHA-256 pins. This is a reference to an operator-reviewed consent record, not proof that a legal review occurred. Existing avatar/voice enrollment and local lipsync services are reused; no proprietary DEEPCORE binary is imported.

Export an asset manifest after authoring `idle.mp4` and optional `smile.mp4`, `present_product.mp4`, `hand_detail.mp4`, `attentive.mp4` under one approved root:

```powershell
$env:KEDI_FFPROBE = '<approved ffprobe.exe>'
# Run in concat/src; output is JSON on stdout. Rights acknowledgement is explicit.
cargo run -p concat-studio --example kedi_scene_manifest -- '<asset-root>' '<consent-record-ref>' '<expiry-RFC3339>' --commercial-approved
```

Run `scripts/kedi-live-preflight.ps1 -AssetRoot ... -AvatarManifest ... -Ffprobe ...`. Exit 2 means at least one gate is not satisfied; inspect the JSON output. A missing NVIDIA utility is not a successful GPU smoke test.

## Commerce and analytics

Only set `KEDI_COMMERCE_ENABLED=true` with a scoped staging token in the local process environment. Configure `KEDI_COMMERCE_ORIGIN`, `KEDI_COMMERCE_TOKEN`, `KEDI_FEATURED_SKU`, tenant and store. The client requires HTTPS outside numeric loopback, disables redirects and bounds response size and timeouts. It checks returned scope/source/schema before facts enter the brain.

A separate worker refreshes snapshots and sends minimal `viewer_intent` signals to the gateway's `/v1/live/events`; the gateway may relay to Twenty. This is not a separately verified Twenty connection. Analytics has a bounded in-memory queue and drops failed deliveries; it cannot block speech. Repeated snapshot failures back off. Changing authoritative facts requires a new revision; expiry produces a neutral unknown answer. Shipping is not guessed: quotes can be carried/validated in canonical snapshots, but unsupported/unscoped live shipping queries remain a request for verification. No order, refund or stock reservation is created from chat.

## Local MCP tools

From `concat/src`, start `cargo run -p concat-studio --example kedi_live_control -- --demo --operator`. This is a standalone local mock controller over stdio, not an exposed HTTP service and not an attachment to a running desktop process. It implements MCP initialization, tools/list and tools/call for status, local session controls, featuring, safe introductions and development simulation. Without `--operator`, writes are denied. There is no platform pin or send-comment tool. Do not attach untrusted agents with operator privileges.

## Rollback and release gates

Unset the Kedi flags and restart to restore the pre-existing controls. No existing engine or datastore was migrated. Preserve authorized product/session records during rollback. Rehearse local STOP and manifest/price-revision failures before a staging pilot.

Public release still requires: real configured GPU/avatar/voice A/V smoke and latency/skew evidence; authorized platform/store staging credentials and official API capability review; a real-time two-hour media soak; operator STOP/rollback exercise on the target machine; and consent/source/weights/voice/codec licensing signoff. These checks cannot be inferred from mock, replay or compilation results. Platform writes remain unavailable until a permitted provider and confirmed ACK flow exist.
