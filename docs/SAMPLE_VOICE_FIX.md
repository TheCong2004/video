# Sample voice: LIVE workflow and verified limits

The LIVE voice control is **Có sẵn / Giọng mẫu**. Sample mode exposes only:

- **Giọng đã lưu**: load the configured service's catalog without uploading.
- **Lấy giọng từ video nhân vật**: use the exact selected motion source; if its
  preparation removed audio, explicitly choose the original speaking file.
- **Tải giọng lên · audio / video**: choose a separate reference file.
- An optional name, status, and the existing preview action.

There is no technical connection form or separate rights-confirmation button on
LIVE. The explicit sample action is the upload instruction. The inline notice
still says the reference is sent and stored by the configured service.

Uploads extract 3–8 seconds of mono PCM16 at 16 kHz, verify actual VieNeu engine
readiness, choose a non-colliding default name if blank, register exactly once,
and select that exact voice. A ready service with no existing voices is valid.
Choosing sample mode without an enrolled/selected voice fails clearly rather
than speaking in a different built-in voice. Switching to built-in speech keeps
the sample service/profile in memory for switching back.

Operator setup is in **Cài đặt Studio → Dịch vụ giọng mẫu**. Numeric loopback/SSH
may omit credentials because upstream VieNeu does not implement authentication.
Public HTTPS still requires a token validated by an actual TLS reverse proxy.
Do not expose the unauthenticated upstream publicly. Operator deployment may set
`CONCAT_VOICE_URL` and `CONCAT_VOICE_TOKEN`; credentials are never logged or saved.
Neither opening LIVE nor selecting sample mode uploads audio or installs a model.

## Diagnosis on 2026-10-21

The previous interface gated uploads on connection, a nonempty name, and a
separate confirmation. The adapter also required a dummy token on localhost and
rejected a ready service with an empty voice list. These gates are fixed.

No listener was present on local port 8767. The Concat avatar Python environment
contains the face-preparation dependencies but not the `vieneu` package. The user
chose a future rented GPU server, so no local model installation/download or paid
server rental was performed. UI/transport fixes cannot substitute for that engine.

## Verification

- 67 source/controller regression checks passed.
- 143 `concat-studio` tests passed; 6 explicitly ignored tests remain ignored.
- All 8 voice tests passed: real local HTTP fixtures exercise health, empty
  catalog, no-token loopback, TLS token requirement, name collision, multipart
  enrollment, exact enrolled-voice speech, malformed WAV and wrong engine errors.
- The opt-in `CONCAT_VOICE_SELECTION_SMOKE` exercises actual native callbacks,
  sample-mode no-fallback, unselected voice rejection and mode switching without
  uploading a reference or invoking a model. The isolated director smoke enables
  it alongside the existing audio/video completion test.
- Final Windows build passed. Candidate native voice-selection and synchronized
  A/V smoke passed, as did actual NexPulse load/resize/minimize/route checks.
  The tested executable replaced `src/target/debug/concat.exe`; hash identity
  after installation was verified. No extra retained Concat EXE copy was kept.

HTTP fixtures do not prove voice similarity, GPU latency, mouth quality or a
working rented deployment. These require a real GPU service and a user sample.
When the server is available, verify engine readiness, upload one clean reference,
preview the registered voice, restart/reload its catalog, then test a LIVE reply
with MuseTalk before using it in a sales broadcast.
