# Follow-up: one connection row and reachable actions

## Click-path findings

1. Production had two Page refresh controls and an extra workflow refresh above
   the actual Page selector. The upper card is removed; select Page, refresh and
   Open Nexora share one responsive row. Selecting a Page already loads workflows.
2. Returning from Tasks/Templates/Workflows constructed Production again and
   unconditionally loaded Pages, clearing the selected profile. A successful
   catalogue is now retained; explicit refresh restores the same ID if present.
3. `NexoraSettings::start` returned profiles when the manager was healthy, without
   asking it to show its window. Explicit Open now uses `show`, launching the
   installed executable through Nexora's existing single-instance handler. The
   cold-start path keeps its existing occupied-port and no-retry safeguards.
   Status/errors are displayed in the row where Open was clicked.
4. Global anchor selection was hidden once scenes existed, although native
   preparation still required that anchor. It is now always available at step 4,
   before scene controls. Missing-reference validation navigates there with a
   visible message; navigation resets scrolling rather than retaining old offset.
5. Scene/script-only native plans no longer require entering a second brief at
   step 1. An existing board is described as such in plan metadata; actual scene
   content remains in the existing storyboard snapshot. Empty input is not
   fabricated into an executable script.
6. Auxiliary Studio screens have an explicit “Back to AI workshop” control.
   This is a return to the hub, not a claim of arbitrary navigation history.
   Redundant project/theme/settings shortcuts in the Studio header are removed;
   the existing global shell/sidebar controls remain.

## Verification

```powershell
node --test scripts/production-practical-ui.test.mjs scripts/native-production-contract.test.mjs scripts/production-steps-regression.test.mjs scripts/production-session-regression.test.mjs scripts/studio-launcher-layout.test.mjs scripts/shorts-ux-regression.test.mjs scripts/native-build-resource-regression.test.mjs
$env:CARGO_BUILD_JOBS='1'
$env:CARGO_INCREMENTAL='0'
cargo build --manifest-path tools/studio-preview/Cargo.toml --target-dir .runtime/nexpulse-build
.runtime/nexpulse-build/debug/concat-studio-preview.exe .runtime/ui-practical-20261023/navigation.png production-navigation 1440 1000
.runtime/nexpulse-build/debug/concat-studio-preview.exe .runtime/ui-practical-20261023/create.png production-create 1440 1000
.\scripts\windows-studio.ps1 -Build
```

The navigation pointer fixture checks Tasks -> Back, unchanged Page index and
zero catalogue reload callbacks. Native Create's pointer fixture uses a stub
start handler, not provider calls. The actual desktop storyboard smoke additionally
invokes its real native controller with scenes and a missing reference: it must
reach step 4 without opening a browser or sending generation.

No app is killed or login added. No real provider video is claimed: a repeated
read-only production WebSocket request still returns 503
`TRANSPORT_ORIGIN_NOT_CONFIGURED`. A reachable Page is not a ready scheduler.

## Interim verification receipts

- Node regression set: 40 pass, zero failures.
- Final headless native UI build passes. Actual pointer fixtures pass for
  Tasks -> Back (Page retained, zero automatic reloads), native Create once
  (duplicate busy click blocked), and sidebar -> workshop -> all seven layouts.
- Step 4 with six fixture scenes was rendered at 960px and inspected: the
  reference picker is above the scene editor, and the connection controls share
  one row. The launcher fixture still emits the pre-existing embedded-image
  warning while cycling empty media; the pointer assertions pass. This is not
  a claim of warning-free media rendering.
- An auxiliary `cargo test --profile quick nexora::tests` invocation was stopped
  after it began rebuilding dependencies. It is NOT reported as a passed test.
  The existing debug target is used for the final app build instead.
- Pre-build cleanup removed 1.36 GB of obsolete Concat rlib/rmeta artifacts.
  They are regenerable; the runnable EXE, user projects and models were retained.
- Effective native browser preset now also uses 9:16, matching the visible
  native graph even when the source channel preset was landscape. Per-scene-only
  content uses a neutral shared identity constraint, not invented scene content.

## Final receipts

- Main `windows-studio.ps1 -Build`: exit 0, 25m09s. Existing
  `src/target/debug/concat.exe` replaced in place at 21:46:56 local;
  269,219,840 bytes. No installer/archive created.
- New EXE `--smoke` with `CONCAT_STORYBOARD_SMOKE=1`: exit 0.
  Log `concat-20261023-144746.log` confirms storyboard operations, then real
  native Create validation with existing scenes -> missing reference -> step 4,
  and `smoke: ok`. No provider/browser job was sent by that smoke test.
- `tools/runtime-checks/nexora-show.rs` was linked against the already-built
  Studio library, without a separate Cargo target. Actual configured loopback
  API and installed Nexora `show()` request pass; profile IDs unchanged and the
  original manager PID 31700 remains. This verifies the request path and
  singleton/catalogue behavior, not OS-level foreground pixel inspection.
- Final Node regression set: 40 pass, zero failures.
- Final read-only production WebSocket check still returns 503
  `TRANSPORT_ORIGIN_NOT_CONFIGURED`. Real generation/import is NOT verified and
  remains blocked by the missing scheduler transport service.
- A tool policy denied cleanup of the small temporary diagnostic executable
  `.runtime/nexora-show-check.exe`; it was retained rather than bypassing the
  denial. It is reproducible from the diagnostic Rust source, not another Concat
  product build. The earlier 1.36 GB cleanup succeeded.
