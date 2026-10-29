# Native Windows package

From the Concat root run `./scripts/windows-package.ps1`. This reuses the
existing D: development build cache and copies the executable and required
native DLLs into a timestamped directory under `dist/windows`. `-Release`
requests a separate optimized release build; it can require considerable time
and disk space. `-SkipBuild` packages an already verified build of the selected
profile. Keep the DLLs and `OmniRoute` folder beside `Concat.exe`; copying only
the EXE is insufficient. For the first developer build, stage the existing
standalone runtime using `./scripts/stage-local-ai.ps1` before packaging.
The standard Windows development profile omits debug symbols for the generated
window crate to reduce memory/disk pressure; it is not an optimized release.

## Regenerable build/test cache

`scripts/cleanup-build.ps1` previews candidates; add `-Apply` to delete them.
`-PruneOwnCache` also removes test-preview incremental caches, unsupported quick
editor caches and older native editor configurations, retaining the two newest
native configurations. The same bounded pruning activates automatically below
8 GiB free, including the cleanup before/after `windows-studio.ps1` builds.
It never targets source, `.deps`, models, user media/projects or the current
`concat.exe`. Compiler/process and Cargo-lock guards prevent cleanup during
active builds; reparse points are rejected. Deletion bypasses the Recycle Bin,
but these caches are recreated by building; a later preview build may be slower.

On 2026-10-20 explicit pruning removed 18.03 GiB of logical cache files and
increased actual D: free space from about 0.13 to 15.9 GiB (hard-linked cache
files can share physical storage). This is not deletion of project media or
Python/model environments.

Opening the application requires native Promax sign-in. Email/password uses
Supabase, followed by the Promax identity exchange; registration can require an
email confirmation. Google opens the system browser unless a Nexora profile
is explicitly selected. OAuth redirect URLs must be allowed by the identity
provider. No real-account end-to-end login is claimed by the automated fixtures.
The identity provider's public settings were checked: email and Google enabled,
sign-up enabled, and email confirmation required.

Default backend: `https://promax-backend.thecong2610.workers.dev`.
`CONCAT_PROMAX_ORIGIN` is an explicit developer override. Cached native sessions
are encrypted using Windows DPAPI and bound to that backend origin.

## Local AI runtime

After verified sign-in (including encrypted session restoration), Concat starts
its own hidden loopback OmniRoute process off the UI thread. No public gateway
URL is needed. AI Keys CRUD, model discovery, test/chat, LIVE text replies and
structured edit proposals use this local client; account/workflow APIs still
use Promax cloud. A free loopback port is chosen rather than claiming 20128.
The UI identifies the local service; it never displays a sample master key or
the private management credential.

Runtime secrets use Windows DPAPI. Provider storage is separate from the package,
under Concat's user data/local-ai profile, isolated by verified account identity
and Promax origin. Re-signing in to the same verified identity retains the profile.
Sign-out/account switch and normal shutdown stop only Concat's owned child;
a Windows kill-on-close Job also contains its child processes. An externally
started OmniRoute instance is neither reused nor killed. A terminated owned child
is restarted before the next action; submitted actions are never automatically
replayed. Missing runtime assets produce a visible error, not a fake success.

Staging/packaging excludes .env files and database/log files. No developer's
provider keys or Promax bearer tokens are copied into the runtime package.
The reused standalone build had management authentication disabled. Staging
mechanically hardens its known, hash-checked compiled auth module AND all
Webpack duplicates, and disables the machine-wide CLI-token bypass. Unknown
auth bundles fail packaging instead of silently shipping anonymous management.
This is tracked in `scripts/harden-local-ai.mjs`; upstream runtime upgrades
require a new review and the real anonymous-rejection integration test.

## Deployment requirements and current limitations

- The backend health endpoint being reachable does not prove authentication,
  database persistence, model inference, or provider management works.
- The deployed worker's legacy provider list returned HTTP 500 during the
  2026-10-15 audit. Native AI Keys no longer calls that endpoint. A worker cannot
  contact a desktop's loopback gateway. Root deployment config now disables
  optional cloud AI; only a separately enabled cloud AI deployment would need
  a secured public HTTPS gateway and server-side management credentials.
- The deployed `/api/v1/auth/me` accepted an unauthenticated request as a test
  principal. This is not a safe production configuration. Enable strict auth
  with the required production database/secrets before distributing publicly;
  do not enable strict production blindly without provisioning those settings.
- OpenTalking rendering, model assets and optional LIVE/Nexora services still
  require their local runtimes. This package does not bundle those Python/Node
  engines or make them available on Cloudflare automatically.

Provider-management requests authenticate directly to the owned local runtime.
The UI does not expose its administrator credential or fabricate successful saves.
Real provider inference and real-account OAuth must still be verified with the
user's configured account; fixture/runtime CRUD tests do not prove quota or
Google redirect readiness. This is a development-profile package, not an
optimized/signed public installer.

## 2026-10-16 connection updates

Stage `scripts/stage-live-runtime.ps1` before packaging. Packages now include
`resources/tiktok-live-connector` and `resources/node/node.exe`; users must not
copy just Concat.exe. The connector is launched hidden on explicit connect,
and its bounded handshake test does not join a room or send a message.

Studio Settings exposes the Nexora numeric-loopback API origin, optional local
API token and executable picker. The token is stored with Windows DPAPI. Start /
Connect reuses a verified ready manager, refuses an occupied unrelated port,
and otherwise opens the selected/installed Nexora executable. Nexora itself is
an independently installed prerequisite: its database, cookies and user profiles
are never bundled. OAuth profile launch and service diagnostics share this
same configuration. Promax authentication is not a Nexora local API token.

AI Keys now routes catalogue, CRUD, Test and Ask to the owned local runtime;
the intervening canned test/chat implementation is removed. Connection testing
is not proof of remaining quota; Ask displays actual returned model/text.
Unknown post-dispatch outcomes are not automatically retried.

The old unscoped `omniroute-providers.json` is preserved as a DPAPI encrypted
`omniroute-providers.dpapi` archive, with verified read-back before plaintext is
removed. A conflicting/corrupt archive prevents deletion. This archive is not
silently assigned to the next signed-in account; old keys may need to be added
again explicitly to that account's actual local gateway. New keys are stored
by OmniRoute using STORAGE_ENCRYPTION_KEY, whose secret is DPAPI-protected.

To generate the per-user installer after a verified package:
`scripts/windows-installer.ps1 -PackageDirectory <package folder>`.
Uninstall removes the exact packaged-file manifest, not user projects or Nexora
data. Public production release remains blocked by the cloud-auth configuration
and real-account OAuth/provider end-to-end checks described above.
