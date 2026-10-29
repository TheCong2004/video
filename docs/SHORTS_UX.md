# Shorts workflow clarification and overview removal

Contract: remove the Overview navigation/page, without orphaning scripts,
workflow or job routes. Studio's initial route is LIVE. Advanced tools previously
on Overview remain reachable from the production page.

Shorts means selecting excerpts from a longer source, reviewing them and saving
separate videos. It is not image-to-video generation or automatic publishing.
The reported source is 5 seconds while the minimum excerpt is 15 seconds: reject
this before analysis or any paid AI request. Preserve chosen durations; offer a
different source or explicit adjustment, never invent highlights or stretch it.

The primary screen shows source, desired excerpt lengths and the next action.
Secondary format/subtitle/analysis settings are disclosed on demand. Concise
Vietnamese results remain on the main surface; full existing warnings stay in
diagnostics, and actionable missing-speech-model/framing limits remain visible.
No model download or network/AI request runs on mount.

Verify native renders at 1080x720 and 360x900 for empty, five-second mismatch,
busy, no-result, results and AI opt-in states. Check real native callback/input
routing with isolated layout fixtures. Native app rebuild requires saving and
closing the currently running Concat; do not kill a user process unprompted.

## Verification

- Four pure Rust tests execute the actual preflight/feedback helpers, including
  invalid duration, below-four-second source, exact minimum, the reported 5/15
  mismatch, no fabricated result, and retained diagnostic warnings.
- 45 JS controller/wiring regressions pass, including Overview removal, advanced
  route reachability, and validation before the AI client is opened.
- 12 focused native Slint captures pass at the two target viewports.
- Native input fixture passes: mismatch cannot submit; limits are not silently
  changed; explicit reduction to four seconds enables one callback; busy blocks
  repeat submission; optional controls remain reachable. No media/AI call occurs.
- 105 shared native renders and five existing pointer/key/clipboard/scroll
  regression checks pass. Diff whitespace check and evidence schema check pass.
- Full application build: pass, 19m02s; existing missing-docs, unused-code and
  unrelated padding warnings remain. New EXE startup/fixture connection and
  12-frame native motion decode smokes pass. The pure Rust tests were rerun and
  pass. No full screen-reader/UIA suite or new real-clip export was performed.
- New executable: `src/target/debug/concat.exe`, built 2026-10-19 21:45 local time.
  Local evidence: `.runtime/shorts-ux/evidence.json` and PNGs beside it.

The screenshots use explicit isolated layout fixtures, not invented runtime
results. No original video is overwritten, model downloaded or paid AI called.
