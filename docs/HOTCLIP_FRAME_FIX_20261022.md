# Hotclip embedded frame repair — 2026-10-22

Native Concat now compares the actual child HWND rectangle to its requested
physical client-area bounds on every layout tick, and repairs drift even when
the cached request is unchanged. Electron no longer writes embedded geometry;
embedded windows are non-movable/non-resizable and their drag regions are disabled.
Standalone Hotclip keeps its normal window controls.

## Verification and deployment

- Hotclip lifecycle/config tests: RED observed before fix, then 5/5 passed.
- Hotclip TypeScript checks and production bundle build passed.
- Concat Hotclip integration regressions: 5/5 passed.
- Web embedding and launcher regressions: 19/19 passed.
- Shared-code native preview: actual rectangle and deliberate drift recovery passed.
- Main EXE build passed (26m 47s). Three Lux dialog Button properties were
  corrected from `text` to the existing component's `label` API to unblock build.
- Main candidate native smoke passed before deployment:
  `.runtime/hotclip-qa-e2e829559dcb4b3984b83fb1d4d5c502`.
- The first post-deployment run recovered native geometry but timed out at stage 3
  (resize). This intermittent test failure is retained, not counted as a pass:
  `.runtime/hotclip-qa-2356fb59391e41d58a606aedc58e365f`.
- Post-deployment rerun passed real Electron media IPC, actual child bounds,
  deliberately displaced child recovery, menu hiding, resize, minimize/restore,
  route hiding and owned-child shutdown:
  `.runtime/hotclip-qa-555a6aa255244ad0863be355392a1316`.

User explicitly authorized termination of Concat PID 20724. Its executable path
was verified before termination. The tested candidate was moved (not duplicated)
over `src/target/debug/concat.exe`; runtime DLL hashes matched before deployment.
Installed size: 241,912,832 bytes. SHA-256:
`BA71045938711553A5BC41E662AA5F3CA59A9AC37AB39C442C57EFD1C5669369`.
No models, projects or media were removed. No inference accuracy or visual
screenshot acceptance is claimed by these native geometry checks.

## Self-evaluation

Overall: 4.0/5. Deliver with the resize-test instability disclosed.

| Axis | Score | Evidence / remaining improvement |
|---|---:|---|
| Accuracy | 4 | Main build, hash and native geometry verified; no new screenshot acceptance. |
| Completeness | 4 | Main EXE deployed and lifecycle passed; resize smoke failed once before passing. |
| Clarity | 4 | Distinguishes source/helper/main verification; keep user handoff shorter than this log. |
| Actionability | 4 | Existing RunBuilt path contains tested EXE; user still needs visual confirmation on their desktop. |
| Conciseness | 4 | Short handoff planned; build progress updates were necessarily repetitive. |

Highest-impact follow-up: investigate the intermittent resize-stage timeout if
it recurs, with observed size/rectangle telemetry. Self-check: the user can now
test the installed fix, but a claim of perfect stability would exceed the evidence.
