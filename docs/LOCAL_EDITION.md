# Local edition — 2026-10-17

User-requested replacement of mandatory Concat/Nexora account login.

- Concat opens projects/editor/Studio directly, without restoring a Promax
  session or opening an OAuth listener. Local access never sets `signed-in`.
- OmniRoute uses a distinct `concat-local-workspace-v1` storage namespace under
  the OS user's application data. Existing DPAPI encryption, private loopback
  credentials, generation checks and owned-child shutdown remain in place.
- Account-scoped key stores and saved cloud sessions are not copied, deleted or
  silently shared with this local namespace. Add personal AI keys in AI Keys.
- Nexora opens local browser management without Supabase gating. Old `auth`
  and `account` navigation is redirected to profiles; account entry points,
  account modal and intermediate local-mode notice UI are removed.
  Startup cloud refresh and frontend Supabase persistence/refresh are disabled.
- This does NOT make cloud workflow/product/sync APIs public. Concat's cloud
  workflow client reports configuration unavailable in this edition. Remote
  workflows, cloud product management and account sync need a separate local
  implementation if they must work without accounts. Server auth is unchanged.
- Local profile API security, website logins inside browser profiles, DPAPI and
  individual AI-provider API keys are NOT application-account login and remain.

The existing installed EXEs are unchanged until rebuilt/repackaged. Do not ship
an old installer as the local edition. Source checks are not a real install or
GUI end-to-end test.

## Evidence and delivery review

- `node --test scripts/studio-auth-key-regression.test.mjs`: 9/9 passing.
- Nexora `node --test scripts/local-edition.test.mjs`: 3/3 passing.
- Nexora `pnpm exec tsc --noEmit --incremental false`: exit 0 after the
  final removal of legacy login views and desktop OAuth helper.
- Scoped `git diff --check`: passed in both repositories.
- Concat `scripts/windows-studio.ps1 -CheckOnly`: exit 0, 9m27s;
  prior Slint/doc warnings and the now-unused account-change helper only.
  Automatic post-check cleanup reclaimed 1.99 GB of regenerable cache.
- Nexora native Rust compilation and interactive first-launch checks were not
  run; frontend type checking and wiring checks do not substitute for these.
- No production authentication guards changed, no cloud secret changed,
  and no user data directory removed.
- No replacement installer produced and no install/GUI end-to-end claim.

Self-evaluation (evidence-based, not a release certification): accuracy 4/5
(wiring/TypeScript checks, not real GUI); completeness 3/5 (local entry is
implemented, installers and no-account replacements for cloud services remain);
clarity 4/5 (local/cloud boundary documented, legacy library auth retained for
encryption/compatibility); actionability 3/5 (Concat native check passed,
installed binaries unchanged); conciseness 4/5 (single delivery note, retained
compatibility shims). Overall 3.6/5. The user should not treat this as a finished
download. Highest impact follow-ups: validate Nexora native and build/test
both distribution packages, then independently migrate cloud-only operations
to real local implementations rather than removing server authorization.

## Follow-up: remove account UI and publish backend (2026-10-17)

- Removed Concat's `StudioSessionPanel` and duplicate Local Studio header
  button. Kept the actual settings/Nexora connection panel and direct workspace
  entry. Removed Nexora's AuthScreen component, account/auth rendering branches,
  modal state, header account pill and sidebar account controls.
- Concat wiring regression: 9/9 passed; Nexora local-edition regression: 3/3
  passed; Nexora TypeScript check: exit 0; scoped whitespace checks passed.
- No new native executable/installer was created. The prior Concat native
  check predates this follow-up's UI-only removals. A standalone UI compiler
  check was not executed because the tool rejected the proposed command.
- Backend `pnpm test:worker-auth`: build plus 12/12 Worker auth/config/WebSocket
  regressions passed. Cloudflare dry-run passed; deployed the existing root
  production environment with strict auth and PostgreSQL storage unchanged.
- Public URL: https://promax-backend.thecong2610.workers.dev
- Previous deployment version: `6154178c-e6df-4e7b-9e5e-3d93c49b413c`.
- New deployment version: `0349b3d1-ce65-4b19-88b3-774ca8490dc1`.
- Post-deploy checks: `/health` 200; anonymous `/api/v1/auth/me` 401;
  `/health/ready` 200, `status=ready`, `database=connected` (checked twice).
  No claim of all backend capabilities or paid AI end-to-end verification.
- No secrets were printed/changed, no production data migrations performed,
  and no old EXE was advertised as the updated local edition.
