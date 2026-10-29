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

## Transcript workbench (HotClip follow-up, 2026-10-20)

Source implementation now includes a native **Cắt theo lời thoại** workbench,
including when analysis finds zero automatic candidates:

1. Chọn video gốc; thêm SRT/VTT đúng video nếu chưa cài Whisper đa ngôn ngữ.
2. Bấm **Tìm đoạn để cắt**. Khi có lời thoại, phần **Cắt theo lời thoại** xuất hiện.
3. Tìm cụm từ có dấu; chọn **Bắt đầu** ở câu đầu và **Kết thúc** ở câu cuối.
   Mốc hiện bằng giây của video nguồn; khoảng nghỉ giữa các câu được giữ nguyên.
4. Bấm **Thêm vùng chọn vào danh sách clip**. Vùng chọn phải nằm trong giới hạn
   độ dài đã đặt lúc phân tích; không tự kéo dài video. Clip trùng vùng bị từ chối.
5. **Xem đoạn**, rồi chọn xuất hoặc **Dựng trên timeline** để xem/sửa tiếp.
   Việc thêm vùng chọn chưa ghi file, chưa sửa dự án, chưa tự chọn xuất.
6. **Sửa câu** chỉ sửa nội dung phụ đề, không sửa tiếng nói. Có 64 bước hoàn tác/
   làm lại riêng cho lời thoại. **Lưu bản phụ đề SRT** để giữ bản sửa trước khi
   đóng app, đổi nguồn hoặc phân tích lại; chọn tên mới, không ghi đè phụ đề gốc.

Search uses HotClip's normalized phrase matching concept (NFKC/lowercase/NFC,
ignoring punctuation/spacing), supports Vietnamese combining accents and phrases
spanning cues. It is not semantic/fuzzy search and does not remove accents.
Results are bounded to 2,000 occurrences and 20 displayed cues per page. Source
review is limited to 20,000 ordered, unique-ID cues / 1 MiB text. Edits are limited
to 180 characters per cue to match native caption limits; timestamps never change.

Text corrections feed both native subtitle rendering and exported clip SRTs.
They clear prior output selections and mark candidates for review rather than
silently retaining approval for changed evidence. Automatic title/rank suggestions
are not regenerated on a text correction. Manual picks have explicit provenance
and **no AI score**. Corrected source SRT export uses source times; clip exports
continue remapping those times to their occupied intervals via the native renderer.

The workbench itself makes no AI requests, downloads no model and uses no GPU.
Analysis still obeys the explicitly selected local/configured-AI mode. Keep native
multilingual Whisper for Vietnamese: HotClip's current SenseVoice, Paraformer and
FireRed catalogue is not a drop-in Vietnamese ASR replacement. Do not install its
default CLI model pack automatically.

**Tìm câu nói lặp** also adapts HotClip `src/core/retakes.ts`: normalized character-
bigram Dice similarity ≥0.72, at least six characters, looking at the next two
cues within twenty source seconds. It scans on a worker only when requested,
reports at most 2,000 pairs and can filter the transcript to those cues. Each
marked cue links by source time to the similar cue. The measure is textual
similarity, not a probability, speech-quality score or proof of a bad take.
Unlike automatic deletion in a retake pipeline, Concat does **not** assume the
last repetition is better: no audio/video is removed. Text edits/Undo invalidate
the scan so stale pair labels are never used. Re-scan after corrections.

### HotClip reuse map

| HotClip capability | Concat status after this source change |
|---|---|
| Source-time highlight evidence, quote matching, gates and moments | Already adapted in `concat-agent/highlight.rs` |
| Analysis, SRT/VTT and installed ASR | Native implementation in `concat-shorts/analysis.rs`; no second ASR runtime |
| Transcript search/correction and cue-boundary clipping | Added in `concat-agent/transcript.rs`, `concat/auto_shorts_transcript.rs` and native Slint UI; session edits can be saved as a new source SRT |
| Batch/watch, finite retry/cancel, safe output and QA | Already native; watch pauses for human review |
| Preview, export, cover, source remapping and publish pack | Already uses the existing native timeline/renderer; no HotClip Electron pipeline |
| Face/shot-aware reframing and multi-person selection | Not connected to a detector; static center-fill remains explicit |
| Repeated takes | Added explicit nearby-cue similarity hints/filter; no automatic take removal |
| Silence subtraction | Not integrated as audio/VAD-grounded reviewable proposals yet; do not infer silence from missing subtitles or cut speech automatically |
| Word-aligned karaoke and speaker labels | Not available without a verified alignment/diarization pipeline |
| Danmaku-driven highlights | Requires an explicit TikTok-event-to-source-time mapping; not wired |
| Glossary, persistent transcript workspace, checkpointed ASR resume | Still missing; SRT save/import is the current persistence path |
| Offline speech enhancement | Not integrated; must not be advertised as a fix for LIVE capture feedback |
| Unattended editing/publishing or platform-specific upload | Not enabled by this integration |

These are capability distinctions, not a claim that the reference application's
entire feature set has been ported. Native project/revision/Undo authority remains
unchanged. No ArtCraft or Promax runtime dependency is introduced.

## Honest limits

Not yet provided by Auto Shorts: face detector/automatic subject acquisition,
multi-person tracking, VLM scene descriptions, diarization, verified per-word
karaoke alignment, a vocabulary/glossary editor, automatic
music licensing/selection, job priority, unattended approval/export policy, or
real platform publishing. Existing native caption styling/editing, effects,
transitions, audio controls and MCP remain available in the main editor.

Verification levels and concrete results are tracked in `HOTCLIP_VERIFICATION.md`.
