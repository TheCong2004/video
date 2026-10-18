# Native Auto Shorts

Auto Shorts is a separate native Studio route. It does not replace the existing
timeline, renderer, AI Edit, MCP server, local key store or project document.
Opening/editing/exporting local projects requires no application account.

## Use

1. Open **Auto Shorts**, choose a video and desired count/duration/aspect.
2. Optionally import a source-timed SRT/WebVTT file. Analyze, then inspect each
   candidate's source ranges, evidence, score and review notes.
3. Select candidates explicitly. Preview renders the same native edit at a
   smaller exact aspect ratio. **Dựng trên timeline** explicitly approves a new
   timeline in the current project; one Undo reverses the batch. Export writes
   the selected videos, covers, SRT, per-clip metadata and `clips.json`.

Basic defaults are local evidence, 3 clips, 15–60 seconds, 720p portrait. Advanced
mode exposes content weighting and the existing configured AI model. Scores rank
candidates in this source; they are not viral probabilities. All candidates
start unselected and require human review. A lack of qualifying candidates is
an honest empty result, not a reason to invent cuts.

## Evidence and privacy

Local analysis samples up to 600 actual decoded video frames and streams audio
RMS into bounded bins. These measurements indicate motion, scene differences and
loudness, **not** emotion, laughter, face identity or semantic understanding.
Silent video is supported independently of speech.

Installed native Whisper models can supply transcript segments in bounded
120-second windows. No model is downloaded by this workflow. Imported SRT/VTT
timings are validated against source duration and overlap. Neither imported nor
Whisper segment boundaries are advertised as verified word alignment. Long
subtitle paragraphs are refused rather than split using invented word times.

Configured AI is optional and sends transcript/evidence JSON only when the user
requests that mode. It uses the exact selected model through the existing local
gateway, never an arbitrary first model. Returned segment/moment IDs are resolved
against source evidence; raw model timestamps and unknown IDs are rejected. A
submitted ambiguous/error response is not automatically resent.

## Native authority and output safety

`concat-agent::shorts_plan` compiles native Commands on a planning snapshot, then
returns a revision-bound `PreparedDraft`. Application uses the existing Session
approval/Undo path. Existing media/timelines are preserved. Rendering uses
`concat-host::export`, not another FFmpeg script/render engine. Captions use the
same native text painter in preview and output.

Current reframing is explicitly **static center-fill fallback**. It is not face
tracking. Inspect edge-positioned subjects, groups and important text manually;
use the native editor's transform/keyframe controls when necessary.

Output is rendered in an exclusively created staging directory, fully decoded
and checked before the directory is published. QA verifies streams, real frame
timestamps, dimensions, duration and bounded audio samples. Near-black frames,
long mono RMS silence and clipping generate technical review warnings; they do
not prove editorial mistakes. Caption safe-zone geometry is validated, but text
meaning and face framing still need visual review.

No existing output is overwritten. Covers are actual rendered frames with
preserved aspect. Publish packs are local files, **not uploads**. No publisher
adapter/account is configured by this feature; remote publish controls stay
disabled. No private platform endpoints or browser cookies are used.

## Queue, watch and cancellation

The local ledger lives at `<AppDirs.data>/auto-shorts/jobs.json` and is bounded to
200 records with three attempts per job. Source/config fingerprints deduplicate
queued, running and terminal work. Process locks protect ledger writes and an
Engine lifetime lock prevents another process from recovering active work.
Interrupted work is marked interrupted, never silently completed.

Watch intake waits for stable size/mtime observations, ignores unfinished files,
processes serially and pauses at review. It does **not** silently approve edits,
render new videos or publish them. Retry is explicit and finite. Cancellation is
cooperative; an in-flight external AI request may already have consumed quota.
No source videos, existing models, key stores or user projects are deleted.

Analysis caches are bounded (12 entries / 32 MB); preview retention is eight
owned files. Per-render scratch is removed after worker completion. Final
publish packs and the bounded job ledger are retained for the user.

## Honest limits

Not yet provided by Auto Shorts: face detector/automatic subject acquisition,
multi-person tracking, VLM scene descriptions, diarization, verified per-word
karaoke alignment, a dedicated transcript correction/vocabulary UI, automatic
music licensing/selection, job priority, unattended approval/export policy, or
real platform publishing. Existing native caption styling/editing, effects,
transitions, audio controls and MCP remain available in the main editor.

Verification levels and concrete results are tracked in `HOTCLIP_VERIFICATION.md`.
