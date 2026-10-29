# Concat: create gesture clips before LIVE

This is an **offline production feature**, not real-time full-body generation.
It generates reply/greeting/product gesture clips with EchoMimicV3-Flash, verifies
their duration, strips original audio and assigns the complete pack to Concat's
existing persisted motion profile. LIVE selects a role and MuseTalk applies the
new reply audio. Existing clips are not re-labelled as generated gestures.

## In Concat

Video nhân vật -> Tạo video -> Chuẩn bị cử chỉ nhân vật:

1. Select an image you have permission to animate (PNG/JPEG, minimum 128x128).
2. Optionally select driving audio, max 5 MiB. Otherwise Concat uses the selected
   voice, or the already-connected LIVE sample voice, for three short samples.
3. Enter **2-10 seconds per clip**. This is the current MuseTalk source limit;
   the value reaches upstream `--video_length`, not just history text.
4. Confirm image/voice rights. No endpoint, token or model fields are shown here.
5. Press **Tạo 3 clip cử chỉ bằng AI và gán cho LIVE** once.

The operator sets the private endpoint and session-only token under **Cài đặt
Studio -> Dịch vụ tạo cử chỉ**. Moving these controls does not provision a server
or mark the service ready: generation still verifies readiness before any media
is sent. Missing setup or an unavailable service gives a plain-language message.

The health check runs before media/TTS submission. There is no automatic retry.
All three outputs must pass media checks before replacing the saved profile.
If a request fails, the old profile stays selected and this attempt's local
partial files are removed. Generated originals are stored under Concat's data
directory, `gesture-generated/<unique-pack>/`; source images/audio are not saved
there. Tokens and selected input bytes remain in process memory only.

The regular **silent video** creator also has a separate 1-120 second field.
Spoken videos keep the full speech duration; this field does not silently cut a
sentence. History reports probed artifact duration instead of a fixed 30 seconds.

## Operator installation on the rented Linux GPU

No model or environment is downloaded by the Concat UI. Install the official
[EchoMimicV3](https://github.com/antgroup/echomimic_v3) environment and the
[Flash weights](https://huggingface.co/BadToBest/EchoMimicV3-Flash) first, following
their upstream instructions and license/dependency terms. Use a dedicated
environment, not the MuseTalk/VieNeu Python environment.

CLI contract checked against upstream commit
`7e89489ca51c0d008fc1963ec6c03fc5bd0b9397`; pin that checkout for initial testing.

The checkout needs `infer_flash.py`, `config/config.yaml` and its requirements.
Set the model root to a directory containing:

```
Wan2.1-Fun-V1.1-1.3B-InP/
chinese-wav2vec2-base/
transformer/diffusion_pytorch_model.safetensors
```

Install FFmpeg/FFprobe and Pillow in that environment. The adapter uses the
upstream Flash CLI (8 steps, 512-bound resolution, 25 fps). It requires installed
weights and CUDA at startup; the model is loaded by the CLI for each offline
clip. A ready runtime is **not proof that the first real GPU render will succeed**.
Model/GPU memory compatibility and output quality still need a real render test.

Launch from the installed environment, with this repository available:

```sh
export CONCAT_GPU_MODE=motion
export CONCAT_ECHO_ROOT=/workspace/echomimic_v3
export CONCAT_ECHO_MODELS=/workspace/echo-models
export CONCAT_LIPSYNC_DATA=/workspace/concat-motion-work
export CONCAT_LIPSYNC_PORT=8768
# Supply your own random alphanumeric 32-256 character token in the environment.
# Do not paste it into logs or commit it. The variable is CONCAT_LIPSYNC_TOKEN.
python /workspace/concat/integrations/lipsync/server.py
```

Keep the service bound to loopback. Use an authenticated SSH tunnel, for example
`ssh -L 8768:127.0.0.1:8768 <your-server-alias>`, then Concat connects to
`http://127.0.0.1:8768`. For a remote endpoint, HTTPS is required. Browser requests,
redirected credentials, arbitrary input URLs/paths and simultaneous jobs are
rejected. The server uses bounded temporary directories and clears uploaded media
after each request. Do not run gesture generation and a separate MuseTalk/LLM
job concurrently on a VRAM-limited GPU; prepare clips before starting LIVE.
On a 12 GiB GPU, stop/unload the resident MuseTalk or LLM process before the
offline generation pass, not merely the LIVE stream. After the pack is ready,
the Echo CLI exits and releases its model memory; then start MuseTalk for LIVE.

The frame count is rounded **up** to Wan's 4n+1 requirement, with audio padded to
cover it, then the silent output is trimmed/probed to the exact requested time.
Final output must be <=25 MiB and within two 25-fps frames of that duration.

## Verification without a GPU

```
python -m unittest discover -s integrations/lipsync -p "test*.py"
python -m unittest discover -s integrations/motion -p "test*.py"
cargo test --manifest-path src/Cargo.toml -p concat-studio motion_generation --lib
node --test scripts/motion-generation-regression.test.mjs
```

These validate transport/security/seconds/assignment contracts, not generated
faces, hands, lip accuracy, RTX 3060/3090 memory use, or production latency.
