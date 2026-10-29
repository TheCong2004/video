# Release evidence and unverified gates

This records local evidence, not a commercial or platform authorization signoff.

| Gate | Local evidence | Public-release status |
|---|---|---|
| Source license inventory | Concat root `LICENSE`, `LICENSE-EXCEPTIONS.md`; workspace manifest declares AGPL-3.0-or-later with plugin exception. Existing `THIRD_PARTY_NOTICES.md` describes Slint/FFmpeg packaging. | Distribution review still required for the shipped artifact. |
| OpenTalking source | Adjacent checkout `artcraft/engines/opentalking/LICENSE` contains Apache License 2.0. | This does not authorize all model weights, training media or voices. |
| FFmpeg codecs | Local verification used the existing `.deps/ffmpeg/*-gpl-shared-*` tools; synthetic fixture was created and probed. | Verify codec/build obligations for the actual distributed binary. |
| GUI build dependency | Exact locked rust-skia 0.153.3 prebuilt artifact, downloaded into workspace only; SHA-256 `A19ADBCB3C8E9D691CE4E9350FCC3E3CE066A0793263D5358185EFBCEAE46F30`. | GUI dependency only; no AI model downloaded. Follow existing third-party packaging review. |
| Identity/voice rights | Manifest requires consent reference, commercial permission and expiry. Synthetic test fixture represents no person. | Actual talent/voice consent, scope, revocation and disclosure must be reviewed by the operator. |
| GPU/runtime | Local preflight could not find `nvidia-smi`; no inference model was launched. | NOT VERIFIED; do not label this a GPU pilot. |
| Media | Existing native A/V path compiles, audio-clock guards are wired, synthetic clip decodes. | Actual avatar, device/OBS output, frame pacing, first audible latency and A/V skew still need target hardware measurement. |
| Platform/store | Local read-only mapping and gateway mock contracts pass. New code sends no real platform account write and exposes no write transport. | Authorized staging API/connector policy and storefront behavior must be tested before use. |
| Soak/recovery | Accelerated 7200-step state soak, bounded queues/analytics, STOP/takeover/generation/restart tests. | A real-time two-hour GPU/media soak, crash/output recovery and operator rollback drill remain required. |

The local operator implementation can be reviewed and exercised with the offline verifier/runbook. Public publishing, account writes, actual commerce transactions and final commercial distribution are not claimed as completed.
