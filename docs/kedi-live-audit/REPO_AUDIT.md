# Kedi Live Commerce — repository audit (P0)

**Audit date:** 2026-10-27 (Asia/Bangkok)  
**Target:** `D:\capcutpolot\concat` (the active Concat-based Kedi desktop repository identified by its existing Kedi UI, LIVE, OpenTalking, and avatar code)  
**Base:** branch `main`, SHA `c64c1517516226a2c7a221260be972b74d0cd1aa`, remote `https://github.com/TheCong2004/video.git`.  
**Worktree:** already dirty before this task: 195 modified/untracked paths. Existing changes include LIVE UI, OpenTalking, avatar, speech clock, packaging and regression tests. Those changes were preserved. The separate sibling `artcraft` checkout contains OpenTalking and is also dirty; this task reads it but does not modify it.  
**Instructions:** no root `AGENTS.md` or CONTRIBUTING.md found at the time of audit. This implementation pack is retained at `docs/kedi-live-codex/`.

## What is present

- Concat is a Rust Cargo workspace with a native Slint desktop app; this checkout is not a Tauri app. The app is `src/crates/concat`; shared application services are `src/crates/concat-studio`; live media services are in `concat-host`/`concat-media`.
- Existing LIVE features include a validated JSONL TikTok connector protocol and owned child-process supervisor (`concat-studio/src/live.rs`, `live_runtime.rs`), a comment/reply UI (`concat/src/studio_live.rs`), a product playlist bridge (`concat-studio/src/products.rs`), local speech/OpenTalking access (`concat-studio/src/opentalking.rs` and `concat/src/studio_opentalking.rs`), plus selected motion clips and lip-sync playback (`concat-studio/src/live_director.rs`, `concat/src/studio_live_director.rs`, `studio_lipsync.rs`). Their current UI/media behavior was not replaced.
- The `artcraft` sibling checkout at base SHA `204cef9f24ac454b511ccde8a6a4bd4e53650bec` includes `engines/opentalking/apps/api/routes/sessions.py` and `services/session_service.py`. Source inspection found `POST /sessions/{session_id}/speak` only accepts `mode=replace`, supports matching `command_id`/`Idempotency-Key`, and submits work to a session service; HTTP acceptance is not playback completion. `/interrupt` queues an interrupt and returns `status=interrupted`; it is not proof that old media has left playout. `/speak_audio` uploads audio for STT, then sends recognized text through LLM/TTS/avatar; it is not raw audio injection. Session events are exposed at `/sessions/{session_id}/events`. P2 lifecycle/playout behavior remains unverified in a running engine.
- Existing Concat OpenTalking client is a local loopback client for avatar/voice/speech capabilities. The existing LIVE flow uses configured speech and local playback/lip-sync; it is not yet connected to the session REST lifecycle above.
- No Pancake/Fufuni/Twenty gateway integration or product snapshot authority was found in the active Rust app sources. Existing product playlist support is not treated as authoritative commerce data.
- No new runtime server, API routes, model, or network dependency was added in P1. No real platform write, livestream publishing, store connection, or account action was run.

## Baseline

Command: `cargo test -p concat-studio` from `concat/src`  
Result before P1: exit 1; 212 passed, 2 failed, 6 ignored. Failures were `production::tests::reserved_central_commands_are_ordered_and_each_dispatch_is_single_stage` (`NEXORA_REQUEST_FAILED`) and `production::tests::native_seven_stage_executor_consumes_central_commands_and_local_artifacts_once` (`LEASE_RELEASE_UNCONFIRMED_UNKNOWN_OUTCOME`). These are outside the new LiveBrain module and were observed before its implementation. Treat the branch as having a pre-existing failing baseline.

Repository-wide `cargo fmt --all -- --check` reports formatting differences across already-dirty legacy files (including `concat/src/ai_editing.rs` and other unrelated modules). New Rust files were formatted directly with `rustfmt --edition 2024` to avoid rewriting user changes.

## P0 decision

Extend the native `concat-studio` crate. Its Rust/Serde dependencies and current LIVE services are the closest existing seam; a separate Python sidecar would add a duplicate local service and deployment burden for P1. Keep Concat's editor, media pipeline, LIVE connector, and existing OpenTalking client intact. P1 is a deterministic, offline Rust module that can later be reached through an explicit adapter. All four Kedi flags are OFF unless explicitly set to `1`, `true`, or `on`; simulator execution never contacts a platform or engine.

## Boundaries and current limitations

- P0 inspected local source but did not start OpenTalking, TikTok, OBS, or a GPU model. Runtime capabilities, event delivery, publishing outputs, model weights, and actual playout ACKs remain unverified.
- P1 mock product facts are sample data and explicitly marked `mock`; they are not Pancake/Fufuni facts and cannot be used for real sales claims.
- The scheduler's duration is an estimate for queue capacity, not an audio/video clock. Real speech lifecycle, interrupt acknowledgment, playout guard, and fallback output belong to P2.
- Clone identity/voice consent, commercial licenses, and model weight redistribution need review before use/publishing.

See `INTEGRATION_MATRIX.md` and `ADR_0001_RUNTIME_BOUNDARY.md` for path mapping and decisions.
