# Production connection via Nexora — 2026-10-23

User interaction: create a Page/profile in Nexora → select it in Concat → use
the Nexora native connection to Promax. No separate Concat login, registration,
test account, Google OAuth or test-mode button is involved.

`ProductionStudio` now reads the existing Nexora profile catalogue. The list
is not populated from Concat's independent `ContentPageStore`; that store is
only an optional preset overlay matched by an explicit `browser_profile_id`.
Nexora IDs, names and running state are retained. Removed profiles disappear;
duplicate preset links are rejected. Browser CDP tabs (`/pages`) are not the
catalogue shown in Nexora's profile manager.

`studio_session::production_client()` creates a delegated workflow client.
The unrelated legacy product-catalogue client retains its former serialized
session owner; it cannot silently change the production connection.

Canonical local bridge request schema:
`docs/contracts/nexora-production-bridge-v1.schema.json`.
Provider: `donutbrowser/src-tauri/src/promax_local_connection.rs` and the new
native-only loopback handler in `api_server.rs`.
Consumer: `concat-studio/src/browser.rs` / `workflows.rs`.

The native bridge rejects browser-origin requests and arbitrary methods,
paths/backends. It rechecks that the selected profile exists and matches the
production snapshot. It never returns a cloud token and never retries a
production mutation. Local native applications share Nexora's existing
loopback trust boundary; the integration marker is not a cloud credential.

Nexora derives a purpose- and origin-bound installation proof from its existing
encrypted local API key. Promax allocates an isolated, persistent installation
workspace using the existing federated identity/device/session storage. Only
member-level, route-limited workflow operations are available. This is an
installation connection, not human account registration or a fabricated test
tenant. The public exchange has bounded per-process admission; configure edge
admission/quotas before granting shared billable provider access.

Deployment gate: `PROMAX_NEXORA_LOCAL_CONNECT_ENABLED=true` on the HTTP and
transport origins. Default is off. This change does not turn off production
strict auth. BE was deployed with operator approval as version
`b7d53754-e558-4608-bb46-9ae0a96a54a2`; health returned 200, anonymous workflows
401, and invalid installation proof 400. Nexora release build and NSIS packaging
succeeded; the installed native EXE was updated with matching SHA-256 and opened
as PID 29964. Concat EXE build is in progress.
See BE `docs/NEXORA_LOCAL_CONNECTION.md`.

Verification so far:

- Concat service library: 181 passed, 6 existing ignored helpers/tests.
- Native UI routing regressions: 30 passed, including stale catalogue replies
  and explicit workflow preference (no first-row fallback).
- Nexora native `cargo check --lib --locked --offline`: passed.
- BE TypeScript source/tests compile; installation/guard tests and a real local
  HTTP connect→auth/me→workflow definitions→deny admin→rate limit→disable check
  pass. No real provider generation or paid API calls were made.
- Direct read-only check against the running Nexora: protocol version 1,
  `READY`, two real profiles returned with the expected catalogue fields.

- Full BE regression: 373 passed, zero failed, one skipped; existing isolated
  local PostgreSQL was started to run the durability test. Worker body framing
  regression also passed separately.

Live installed-Nexora verification: READY and two actual profiles returned.
The native production bridge reached the deployed Worker, but session exchange
failed. Worker tail confirmed `relation "device_registrations" does not exist`
in `PostgresAuthStore.findDeviceById`. Request ID:
`0e130502-23d8-43f9-9987-d269e21d70a2`. This is a production database schema
blocker, not a need for user login. No production DATABASE_URL/admin credential
is configured in this local environment; no production migration has been run.
Do not claim a successful cloud session from health checks or local test results.

Outstanding: finish Concat EXE build, obtain secure production database access,
inspect/apply the existing forward migrations safely, and repeat the native
multi-process connection check.
Listing a Page or returning a workflow is not worker readiness. The larger
production port still needs end-to-end dispatch/artifact/import verification;
local image upload remains deliberately blocked until a real transfer path
is connected. Do not describe the complete seven-stage production chain as
finished based on these connection tests.
