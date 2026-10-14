# Studio migration: initial source/action inventory

Current native service ownership: health, workflow definitions and local prompt
CRUD now live in `concat/src/crates/concat-studio`. `concat-host` re-exports them
for the window. See `STUDIO-BACKEND-MIGRATION.md` for the expanded user-approved
scope including OpenTalking/LIVE, the removal gate and the implemented/pending
acceptance criteria. This inventory is not a claim that every action is ported.

2026-10-13. This is an initial adapter map, not proof that every view is ported.
Source paths below are relative to `../artcraft`.

## Common client boundary

`frontend/apps/artcraft/app/src/pages/FlowordStudio/src/api/flowordClient.ts`
contains two different transports:

1. `invokeCommand`: ArtCraft Tauri IPC commands, dependent on its desktop process,
   runtime state and local database. Concat cannot call these by copying TSX.
2. `gatewayRequest`: HTTP through `VITE_BACKEND_BASE_URL` (default loopback 30000),
   with a Promax bearer token obtained through `getPromaxAccessToken` and a default
   10-second abort timer. Concat needs its own authenticated session adapter.

Do not reinterpret all IPC commands as Promax HTTP routes. Confirm Rust command
handlers, persistence ownership and authorization before exposing an adapter.

## Action groups verified in the client

| View / domain | Existing operation boundary | Migration work before enabling |
|---|---|---|
| Dashboard | `floword_dashboard_summary_command` | Typed summary/read adapter; do not fabricate counts |
| Local production | `enqueue_floword_workflow`, `get_floword_workflow`, `list_floword_workflows` | Preserve local execution owner and project/job correlation |
| Local retry/cancel | `cancel_floword_workflow`, `retry_floword_step`, `retry_floword_job_from_start`, `skip_floword_research` | Explicit controls; preserve idempotency and backend state transitions |
| Central workflows | GET `/api/promax/artcraft/workflows/definitions`; POST `/workflows/{id}/validate` | Authenticated adapter and preflight explanation before submission |
| Central runs | GET `/api/promax/artcraft/workflow-runs`; POST `/workflows/{id}/runs` | Pagination, immutable originating project, explicit run action |
| Central run controls | GET `/workflow-runs/{id}`; POST `/{id}/cancel`, `/{id}/resume` | Bound retries; no automatic resubmission on reconnect |
| Job centre | `list_pipeline_jobs_paginated_command`, `listPipelineJobEvents` client wrapper | Typed job/event mapping; confirm exact event command handler |
| Scripts | `list_prompt_templates_command`, `upsert_prompt_template_command`, `delete_prompt_template_command` | Ownership, validation, save/delete confirmation; read port first |
| Batch | `validate_bulk_import_command`, `commit_bulk_import_command` | Keep validate and commit separate; show bad rows before mutation |
| Channels | `list_content_pages_command`, `get_content_page_command`, `create_content_page_command`, `update_content_page_command`, `archive_content_page_command` | Auth/profile bindings; no copying cookies or keys into project files |
| Publish targets | `list_content_page_publish_targets_command`, `upsert_content_page_publish_target_command`, `delete_content_page_publish_target_command` | Retain page/profile ownership and validation |
| Publishing | `list_job_publications_command`, `approve_publication_command`, `reject_publication_command`, `schedule_publication_command`, `retry_publication_command`, `post_now_publication_command` | External-write boundary; explicit user action and test-account verification |
| Configuration | `get_floword_system_setting_command`, `update_floword_system_setting_command`, `check_storage_health_command`, `check_system_readiness_command` | Separate read checks from settings mutations; redact secrets |
| Runtime | `get_auxiliary_backend_status`, `get_studio_runtime_snapshot` | Report actual capabilities; do not start everything on launch |
| Browser workers | `list_browser_workers_command`, `listDonutProfiles` client wrapper | Confirm handler names and profile access ownership before connecting |
| Existing CapCut output | `rebuildFlowordCapcutDraft`, `openFlowordCapcutDraft` client wrappers | Keep as optional legacy export; Concat Media/timeline is a separate target |
| Sync | `getStudioSyncStatus`, `setStudioSyncConsent` client wrappers; pull/outbox/conflict operations | Preserve explicit consent; inspect exact handlers and reconciliation first |
| AI providers | `/api/ai/models`, `/api/ai/health`, provider list wrappers | Read capability contracts first; never equate listed model with generation success |

For shorthand HTTP paths, the prefix is `/api/promax/artcraft`. Wrapper names
are deliberately identified as wrappers where the underlying command was not
fully traced yet; they are not invented endpoint contracts.

## Next concrete adapter slice

1. Trace central workflow definitions/list-runs from gateway to Promax handlers.
2. Confirm response fixtures and auth requirements without using live credentials.
3. Implement a read-only Rust list adapter with response limits, cancellation and
   explicit endpoint configuration.
4. Build native list/detail UI using real states and empty/error feedback.
5. Verify account sharing before enabling run/validate calls that require auth.
6. Implement artifact import after a real output contract is confirmed.

### Progress after the read-only catalogue slice

- Definition contract traced through `ArtcraftController.listWorkflowDefinitions`
  and the shared `GraphWorkflowDefinitionSchema`: direct Promax path
  `/api/v1/artcraft/workflows/definitions`, `{ok:true,data:[...]}` envelope.
- Native Rust adapter and workflow catalogue UI implemented against that contract.
- Missing session, forbidden access, rate limit, unavailable service, malformed
  response and empty catalogue are distinct states; no fake success fallback.
- Explicit process-token pilot only: shared Google/ArtCraft/Nexora OAuth session
  lifecycle, refresh and OS-backed token persistence are not implemented.
- Central run listing, submissions and artifact import are still later slices.

## First-slice implementation boundaries

The new native menu is not an HTTP browser and does not embed ArtCraft's React
application. It adds no Node/Python engine, startup service, cloud deployment,
provider call, publication, database copy or credential migration. Its useful
first actions are the existing file import and explicit local transport checks.
