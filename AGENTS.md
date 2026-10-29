# Concat contributor instructions

- Preserve the native Rust/Slint application and existing Film, Factory, Concat and OpenTalking workflows. Extend existing services before introducing another owner of project or account state.
- Read `.ai-style-rules.md` before changing native UI. Reuse the shared theme and Studio controls; allow wrapping and scrolling at narrow widths.
- Run Rust commands from `src` so `rust-toolchain.toml` selects the pinned toolchain. Windows native builds require the SDK paths documented in `scripts/windows-studio.ps1`.
- Treat UI callbacks and static inventory matches as candidates, not evidence of working automation. Register a Control capability only when an actual executor exists.
- Persist workflow dispatch before executing it. Never automatically replay a write whose outcome is unknown. Bind editor changes to the current project revision and retain normal undo history.
- Reuse the existing AI connection owner. Do not copy development Codex credentials into product runtime storage. Keep network and MCP Control grants under operator control.
- Keep audit, capability and test claims in `docs/AI_*.md` consistent with actual checks. Compile-only adapters are not end-to-end tests.
- Delete only verified, task-owned generated build/test artifacts. Preserve source, models, SDKs, user projects, output videos and the current working build.
