# Storyboard v1

The canonical in-process and on-disk contract is the serde `Storyboard` type in
`concat-studio/src/storyboard.rs`. All native views, exports and render jobs use
that type. Its `version` is 1. Additive optional fields require serde defaults;
incompatible changes require a new version and an explicit migration.

A board contains ordered shots with persistent IDs. Each shot owns narration,
duration, visual direction, local background/foreground/audio references and
verified generated output. Local paths remain local. The existing production
workflow receives only the selected shot's text settings; it never receives a
local path presented as an uploaded URL. A successful workflow submission is
not a completed or downloaded video.

Local previews and final assembly use actual image/video/audio inputs. SRT timing
is derived from the same shot durations used by assembly. Narration text is not
treated as synthesized audio. Rendering with unrecorded narration is rejected.
Editing direction invalidates preview/generated-output selection but preserves
the original files. Queue jobs freeze a board snapshot and stop between shots.

The existing generic workflow's `inputs.duration_seconds` accepts a finite JSON
number from 0.5 to 7200 seconds. Fractional scene durations must not be truncated
to integer seconds. This is an additive relaxation of the former native 10-second
minimum; the BE workflow inputs already accept numeric values. Provider-specific
duration limits are still determined by the selected workflow.

Generated AI scripts are parsed as bounded shot data, never executed as commands.
No UI can mark an asset or preview verified based only on an AI text response.
