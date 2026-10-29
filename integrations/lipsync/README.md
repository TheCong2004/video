# Concat GPU lip-sync — buffered MuseTalk 1.5

Status: integration source + offline tests. **Not deployed or GPU-benchmarked.**
The desktop EXE must be rebuilt after this change. No ArtCraft/Promax service,
no hosted AI subscription, no automatic paid GPU purchase.

## What this does (and does not do)

- Uses the **same synthesized reply audio** to generate the lips; keeps original
  body/background. MuseTalk itself does not synthesize gestures or clone voices.
- Resident model + one cached avatar. Downloads and prepares complete utterances
  before playing. Network/render latency delays the reply instead of streaming
  half-ready pictures. Not zero-latency/token-level streaming.
- Native video is paced by the audio-device sample clock and a first-picture
  barrier; speech replies never loop. Idle motion resumes at the end. Interrupt
  cancels delivery and actually stops queued audio.
- Source: one clear frontal consenting adult, MP4, 1–10 seconds, ≤25 MB.
  Audio ≤15 seconds. Output fits 1280×720 or 720×1280, no source upscaling;
  face model resolution is **256×256**, not “1080p generated face”. Blending may
  change mouth detail or jitter. Source watermark/blur is not removed.
- A whole answer may pause before starting; measure this on the rented GPU.
  Do not claim 3060 real-time based on V100 benchmarks.

Sources checked 2026-10-20:
[official MuseTalk](https://github.com/TMElyralab/MuseTalk),
[pinned inference utilities](https://github.com/TMElyralab/MuseTalk/blob/0a89dec45a0192b824e3cf4daf96c239440c5ed8/scripts/realtime_inference.py),
[license and dependencies](https://github.com/TMElyralab/MuseTalk/blob/0a89dec45a0192b824e3cf4daf96c239440c5ed8/LICENSE).
Upstream permits commercial model use, but review the licenses of each dependency
and obtain permission to use the person's likeness/video/voice. Wav2Lip is not
selected because its official weights are non-commercial.

## GPU server setup (operator-run, Linux + NVIDIA Container Toolkit)

Start with the intended **RTX 3060 12 GB**, not the 8 GB variant. This is a test
target, not a measured guarantee. Keep persistent disk for model files/cache;
budget CPU RAM for decoded source frames too. Do not rent long-term before the
benchmark below. Docker recipe has not been built on this Windows/no-CUDA host.

From this directory copied to your server:

```sh
docker build -t concat-lipsync:dev .
docker volume create concat-musetalk-models
# Explicitly downloads models from official sources; inspect licenses first.
docker run --rm -v concat-musetalk-models:/opt/MuseTalk/models \
  concat-lipsync:dev python /opt/concat-lipsync/download_models.py --accept-licenses
```

Create a private environment file with a **random alphanumeric** 48-character
`CONCAT_LIPSYNC_TOKEN` (e.g. generate with Python `secrets.token_hex(24)`).
Do not paste it in Git, command screenshots, or chat. Start with host networking;
the Python endpoint binds **127.0.0.1 only** (never expose unauthenticated HTTP):

```sh
docker run --rm --gpus all --network host --env-file /secure/lipsync.env \
  -v concat-musetalk-models:/opt/MuseTalk/models \
  concat-lipsync:dev
```

Normal startup is offline and fails if CUDA/weights are absent. Look for
`LIPSYNC_READY`. A startup message is not proof that face preparation/inference
will succeed; do the actual video/audio test below.

On Windows, establish an SSH tunnel (replace the placeholders):

```powershell
ssh -N -L 8766:127.0.0.1:8766 USER@GPU_HOST
```

In the **new Concat build**: Cài đặt Studio → Máy chủ nhân vật.
Use `http://127.0.0.1:8766` + your secret, then “Kết nối máy chủ”.
This setup explicitly permits uploading the selected avatar video and TTS audio
to your own server when answering. Once readiness is verified, LIVE uses lip sync
automatically; there is no separate on/off toggle in LIVE. Entering LIVE retries
readiness only when user-configured credentials exist, never submits a render or
invents credentials. An unavailable/unconfigured server is shown as such, not as
lip sync success. Choose a short MP4 avatar and listen before real LIVE.
Credentials remain in RAM for this app session, not saved to a plaintext project.
The GPU caches the last avatar's prepared face/body frames in its container until
replacement/container removal; input speech and reply work files are temporary.
Use your own trusted private server, not an anonymous shared endpoint.
Remote direct origins require HTTPS + a reverse proxy with upload/read timeouts
≥180s, request limit41MB, no request-body logging. No API redirects or auto POST
retries. One operator token/one GPU job at a time; not a multi-tenant service.

## Verification gate before real LIVE

1. Confirm `nvidia-smi` reports the real GPU/VRAM; record driver, model receipt,
   image digest, source resolution and warm/cold timings.
2. Use a clear 5–10s source and three different short Vietnamese sentences.
   Confirm the generated mouth changes per sentence (not the idle source).
3. Measure `LIPSYNC_RENDER frames=… seconds=… audio_seconds=…`. Render time / audio
   duration <1 after warmup is a throughput goal, not yet established. Measure
   user-visible first-response delay separately, including TTS/upload/download.
4. Watch lips vs consonants on the actual TikTok capture for at least 30min;
   measure A/V skew and dropped-frame counts. Do not advertise flawless realtime
   before this test. If too slow, keep utterance buffering or change GPU.
5. During render: stop/change avatar, disable GPU, interrupt speech, disconnect
   network. No old reply should replace the new selection; no voice-only fallback
   should be mislabeled as synced. GPU failures should show an error, not success.

Offline checks (no downloads/CUDA, fixtures are not inference evidence):

```sh
python -B -m unittest discover -s integrations/lipsync -p 'test_*.py'
cd src
cargo test -p concat-studio --profile quick lipsync
cargo test -p concat-core speech_clock
```

## Native director and existing SLive sample voices

Video nhân vật → Tạo video now contains the character-motion preparation panel.
LIVE has no technical setup block. The preparation panel has three optional slots:
reply, greeting and product introduction. These are **user-assigned clips**, not
pose extraction or newly generated gestures. The selected avatar remains the idle
loop. With the director and MuseTalk enabled, reply keywords choose a source clip,
the new speech drives its mouth, then the native player returns to idle. Source
audio is never mixed into playback. Only paths/enable state persist in
`live-director/motion-profile.json`; removing a mapping never deletes the video.

The sample-voice client reuses `integrations/slive/upstream/tts_server/app.py`
(VieNeu) and the existing `tts/f5api.py` API contract. It does not implement a new
TTS engine. Run that service separately from MuseTalk; do not import/start it on
the Windows UI thread. Model installation/startup and a real GPU run remain
operator steps, not verified by native/HTTP fixture tests.

The original SLive voice service has no access-token enforcement. **Never expose
its raw port publicly.** Bind it to server loopback and tunnel it, or put it behind
an authenticated TLS reverse proxy. Example tunnel when the remote voice service
listens on 8767 (configure its launch port to match):

```powershell
ssh -N -L 8766:127.0.0.1:8766 -L 8767:127.0.0.1:8767 USER@GPU_HOST
```

Use `http://127.0.0.1:8767` in **Cài đặt Studio → Dịch vụ giọng mẫu**. Numeric
loopback/SSH permits an empty token: the upstream service does not enforce one.
Public HTTPS still requires an alphanumeric 32–256-character token, which the TLS
proxy must actually verify. Never expose the raw upstream publicly. Tokens stay
in RAM. Operator deployment can supply `CONCAT_VOICE_URL` and `CONCAT_VOICE_TOKEN`
without putting them in the LIVE controls. Verify health reports `S-Live Voice Pro`.
A ready server with an empty voice list can now enroll its first sample.
Native decoding extracts the first 3–8
seconds as mono PCM16 16 kHz. Use a clean solo voice; no denoising claim is made.
The upstream server stores the voice profile/reference; disconnecting Concat does
not delete it. Use the server's voice management to remove it. Enrollment is never
retried automatically: if a response is lost, check server voices before resending.

The main **Giọng nói** control now has **Có sẵn / Giọng mẫu** choices. Under
Giọng mẫu, only sample actions, optional naming and status are visible. **Lấy giọng
từ video nhân vật** uses the exact currently selected LIVE motion source;
**Tải giọng lên** opens a separate audio/video picker. Both call the same bounded
enrollment adapter. Selecting a file through these explicit upload actions
authorizes submission; there is no separate ownership-confirmation checkbox.
The inline notice explains that 3–8 seconds are sent and stored by the configured
voice service. Selecting an avatar or opening the page never uploads audio.
An upload connects on demand, checks real engine readiness, auto-names unnamed
samples without overwriting profiles, enrolls once, then selects that exact voice.
**Giọng đã lưu** loads the existing catalog without technical connection fields.
Choosing **Có sẵn** uses the built-in route but retains the sample connection in
RAM for switching back. Explicitly changing the service in Settings disconnects it.
Without a running service, upload gives a clear error; it does not install models,
rent a GPU, claim cloning is ready, or silently use a different voice. A selected
sample mode without an enrolled/selected voice cannot fall back to Edge speech.
Older local face-preparation exports are silent. If the selected avatar file has
no audio stream, the sample action explicitly opens a picker for the original
speaking video/audio. It does not guess another file or recover erased audio.
Cancelling this picker uploads nothing; the idle video remains unchanged.

No automatic Edge fallback while sample mode is selected. Disconnect restores
the existing local/catalog voice route. Single-flight replies prevent overlapping
GPU requests/audio; interrupt invalidates pending delivery. Each reply is still
fully buffered, so this is not a claim of continuous low-latency generation.

Before renting a GPU, use the isolated operator smoke command below with an MP4
containing audio/video, 1–15 seconds, ≤60 MB and ≤1280 pixels per side. The test
control is not exposed in the selling-LIVE UI. It exercises the same synchronized
playback without calling a model, posting chat or deleting the fixture. It cannot
prove mouth quality, voice similarity or GPU speed; test real services separately.

For a reproducible native offline check after building (plays the fixture's audio
briefly, in isolated settings, then exits):

```powershell
.\scripts\live-director-smoke.ps1 -FixturePath 'D:\path\short-with-audio.mp4'
```

Require `LIVE_SYNCED_SMOKE=PASS`, not startup exit code alone. This checks native
video delivery, two completed replies, a real UI interruption during playback,
recovery and three returns to idle. Use a moving fixture with at least one second
of audio. The script runs the built EXE directly without copying it and isolates
settings; it is not inference or a subjective lip-quality measurement. Test logs
stay under `.runtime`.

Native speech preparation waits up to five seconds for the output device on its
worker thread. Cancellation is checked before decoding and throughout this wait;
an interrupted or superseded reply must not queue later when the device becomes
ready. This addresses cold-start `audio output device is not ready` failures,
not broken/missing devices. There is no automatic replay of a submitted buffer.

Rollback: disable the motion profile in video preparation, select a built-in voice
and change/disconnect the GPU service in Studio settings. Existing idle
clips and original files remain intact; server-stored voice profiles require
separate deletion. No ArtCraft or Promax process is required.

The current MuseTalk worker caches one prepared source. Switching gesture slots
can therefore incur cold avatar preparation again; idle/reply boundary poses may
also differ. This build does not promise seamless gesture transitions. Benchmark
each assigned clip and role switch before real LIVE, not just repeated speech
on the same warm avatar.

## Noise/echo: separate from lip generation

Muting TikTok Studio's speaker/system-audio source removes **all captured desktop
audio**, including TTS; that alone does not prove the avatar video is noisy.
Concat's idle video decoder has no audio decoder. To isolate: Windows volume
mixer → mute **only Concat** briefly while leaving TikTok capture unchanged.
If noise remains, inspect the other apps/monitor-return routes. If noise stops,
record Concat's isolated output and compare TTS-on vs idle. Never capture both
app audio and a duplicate desktop route or play the monitored LIVE back into the
same captured device. No sound settings are changed automatically by this patch.
