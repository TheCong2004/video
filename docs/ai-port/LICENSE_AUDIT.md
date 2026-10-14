# Preliminary license audit

This is a technical inventory, not legal advice or clearance for every file,
dependency, model or asset. Source commit pins are in SOURCE_INVENTORY.

- OpenChatCut root AGPL-3.0-or-later: preserve copyright/license notices for
  translated/copied material and comply with source/network obligations.
- WeftCut MIT: Copyright 2026 UncleChair; carry the full MIT notice if adapting
  source expression. Cartcut root MIT: Copyright 2025 cartesiancs; retain its
  full notice for source adaptations even though package metadata omits it.
- Velorn GPL-3.0-only: retain its GPL/copyright notices for adapted source.
  GPLv3 and AGPLv3 section 13 expressly permit a combined work under their
  stated conditions; this does not erase the original component's terms.
  See [GNU compatibility guidance](https://www.gnu.org/licenses/license-compatibility.en.html),
  [GPLv3](https://www.gnu.org/licenses/gpl.en.html) and
  [AGPLv3](https://www.gnu.org/licenses/agpl-3.0.html).
- Target Concat is AGPL-3.0-or-later with an existing plugin exception. Do not
  assume that exception applies to upstream material from another project.

## Material introduced so far

The native mint fix, Editor draft/revision API, Session wrappers and initial
`concat-agent` implementation are newly written native Rust using Concat's
existing model/commands. Upstream policies/proposals/plan shapes were inspected
to identify required behavior, not copied verbatim. Existing Concat notices
remain; new files carry AGPL-3.0-or-later and Concat contributor attribution.
No upstream UI/assets/models/workflow JSON/packages have been copied yet.

Before translating more upstream implementation, record source path/commit,
retained attribution, derivative target paths and dependency licenses here.
Model weights, reference assets, fonts and provider terms need separate review;
repository code license alone does not license those artifacts.
