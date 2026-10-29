# ADR-0002 — Literal sales speech and actual playout evidence

Status: implemented locally, GPU pilot not yet measured.

The checked-out OpenTalking fork's `apps/api/routes/sessions.py` accepts replace-only text speech with command idempotency. Its `opentalking/pipeline/session/runner.py` and `pipeline/speak/synthesis_runner.py` also show that a `speak` request may enter the engine's LLM conversation pipeline. `speech.started` can be emitted before TTS generation; HTTP queued and engine events are not proof of first audible audio. `speak_audio` remains STT input, not raw TTS ingestion.

Kedi commerce speech therefore uses its existing literal TTS + native lip-sync path: `studio_live_director::synthesize` → `studio_lipsync::play` / `studio_live::play_tts`. The new owner creates deterministic verified speech, and first-audio observation uses the existing `SpeechClock.seconds()`. Cancellation is checked after synthesis, during the A/V clock wait and at completion. STOP/takeover and product revision changes invalidate the generation without waiting for a provider response. Existing idle playback owns fallback media; the controller never exports a timeline per frame or opens a new publisher.

`OpenTalkingLiveAdapter` remains useful for bounded REST contract tests and dialogue sessions, but advertises `supports_literal_speech=false`. It must not be substituted for literal sales speech without an audited literal-ingest capability. Raw SSE events must not be relabeled as audible playout evidence.

The native feature is off by default. `KEDI_LIVE_ENABLED=true` enables the operator panel in mock mode. `KEDI_OT_ADAPTER_ENABLED=true` additionally permits the existing real TTS/avatar path, subject to manifest consent/expiry/hash/media preflight. `KEDI_MOTION_ENABLED` separately enables prepared gestures; otherwise idle is selected. No model weights are automatically installed by this feature.
