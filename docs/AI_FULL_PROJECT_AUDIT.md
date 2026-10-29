# Concat AI Control audit

Date: 2026-10-28. Status: implementation in progress; this is not a full-automation certification.

## Architecture verified in source

The product is a native Rust/Slint desktop editor. The workspace has 19 crates: concat, concat-agent, concat-android, concat-api, concat-cli, concat-core, concat-effects, concat-export, concat-host, concat-media, concat-perf, concat-project, concat-render, concat-server, concat-shorts, concat-speech, concat-studio, concat-text and concat-vision. The pinned Rust toolchain is 1.93.1. Media processing uses FFmpeg and native rendering/speech services; Windows builds also require ONNX/Sherpa and Skia SDKs.

`concat-project` owns editor commands, revisions and undo. `concat-host::Session` coordinates the live project and services. `concat-api` dispatches typed requests; `concat-server::Hub` serializes API access and enforces transport/write-root rules. Native Slint pages bind Rust callbacks in the main application. Files, project documents, service configuration and specialized production ledgers supply persistence; there was no general durable cross-module workflow engine.

## Control surfaces

See [the generated capability matrix](AI_CAPABILITY_MATRIX.md) and [machine inventory](ai-control/inventory.json) for source locations and callback candidates. The generator is `scripts/audit_ai_capabilities.py`. Matching an `on_*` name is not proof that a UI action reaches that implementation; dynamic bindings and similarly named callbacks need individual runtime tracing.

| Area | Existing implementation | Automation finding |
|---|---|---|
| Project/timeline | Session, Editor, Commands, prepared drafts and undo | Strict revision-bound EditPlan bridge is usable; general project lifecycle is not yet exposed to Control |
| Media | Native probe/import and catalogue | Control now reuses probe for operator-attached files; import into the live timeline remains separate |
| Factory | Existing download, product and FFmpeg/TTS render adapters | Control invokes the real render adapter and validates its actual output |
| Film/storyboards | Native storyboard, scene and production modules | Preserved; not registered as Control capabilities yet |
| Speech/subtitles | Existing speech, Factory narration and captions | Factory render can use operator audio or online speech with permission; not a general speech-control API |
| Export/preview | Shared exporter, native rendering and API endpoints | Factory artifact validation is implemented; full live-editor export orchestration remains open |
| Library | Saved-video metadata and files | Real bounded metadata listing is registered |
| Live/OpenTalking | Live session, scene, speech/motion adapters | Preserved; no general Control executor claimed |
| External/browser | Existing dedicated browser profiles and connection adapters | Reused only through existing services; no new account or credential owner |

## Existing MCP and important gap

`tools/ai-edit-mcp/server.mjs` previously exposed context, status and propose-edit. The DPAPI-protected local connection, bounded loopback protocol and native approval policy remain in place. Three additive Control tools discover executors, inspect workflow status and submit work. Control requires a separate UI opt-in and cannot remotely grant network access or approve its own write.

`concat-api::LiveSessionBridge` defines live document/apply/save/checkpoint/lease methods, but the audit did not find a production GUI installation of that bridge. API sessions must therefore not be described as guaranteed control of the open GUI project. The new timeline adapter instead calls the existing native EditPlan path on the UI thread.

## Gaps requiring implementation and runtime checks

The current planner handles library listing, a bounded render workflow and a revision-bound edit workflow. It is not a general agent planner. Missing coverage includes live project creation/import/export orchestration, all Film stages, general subtitle/audio/effects controls, provider routing, external MCP client registration, Codex model turns, protected-scene continuation and the four full acceptance scenarios from the master prompt. Authentication, paid usage and production publishing are not silently substituted or simulated.
