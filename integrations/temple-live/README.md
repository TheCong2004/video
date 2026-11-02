# Chánh Điện runtime in Concat

Original component: `D:\chanh-dien-online`, commit
`17feae0905f4892fe09656d19c09fc3bb8713738`, MIT © 2026 Kent Juno (see LICENSE).
Only source, locked dependency manifests and the original visual/audio assets
are imported. No account data, TTS cache, `.env` or generated files are imported.
No Artcraft source or runtime is used.

The native Concat controls subscribe to `/ws?role=control` and send the original
`{t:"ctl",cmd:...}` protocol. The HTTP `/api/control`, `/status`, `/api/status`
and standalone `/live/control.html`, `/live/studio.html`, `/live/overlay.html`
routes remain available. The newer `a`/`simChat` client syntax is accepted as a
compatibility alias, not as a different state owner.

Install locked dependencies with `npm ci --ignore-scripts` in `live`. Start
with `PORT=3088 node live/server.mjs` (PowerShell: set `$env:PORT='3088'`).
Only localhost is bound by default. `CONCAT_TEMPLE_DATA_DIR` selects the private
runtime cache/settings directory. The native runtime launcher supplies it and
starts no TikTok account connection or broadcast automatically.

Extensions repair the existing incomplete controls: simulated likes and skin
changes, real product pinning, stopping queued/active speech, truthful bot and
TTS state, bounded Edge TTS deadlines/retry/cleanup and persistent settings.
The original 1080×1920 canvas, art, sprites, animation and sound remain intact.
