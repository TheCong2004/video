# Campaign Automation: audit and current boundary

Date: 2026-10-29. Scope: additive native Concat campaign workflow behind `CONCAT_CAMPAIGN_ENABLED=1`; ordinary editor, Film, Factory, existing AI Control and OpenTalking remain their current owners.

| Capability | Existing owner reused | Campaign change / state |
|---|---|---|
| Native UI/project editor | Rust/Slint Studio, Factory and project/editor services | Dedicated Campaign Manager in the existing AI Control window; editor import opens the real Concat project |
| AI account/runtime | Existing `LocalAiRuntime`, DPAPI and AI connection selection | Independent worker attaches to the same owner/profile; it does not clone credentials |
| Web-chat planning | Existing owned ChatGPT/Claude/Gemini browser profiles, web receipts | Structured `Plan`/draft path; web result uncertainty stays explicit |
| Nexora/Grok media | Existing Nexora profile/catalog and Grok production adapter | Campaign Media stage uses selected Nexora profile and operator-selected image; actual generation still needs that configured profile, service and account |
| Voice, captions, render | Existing Vietnamese Edge TTS, Factory render, FFmpeg and font assets | Real audio render; bounded media QC checks decode, duration, sound and caption bounds |
| Edit MCP | Existing AI edit MCP and permission bridge | Four campaign list/get/draft/control tools added to the existing server; no external execution/approval grant |
| Persistent state/worker | Existing application data and worker launch conventions | Versioned JSON records, per-campaign OS locks, global publishing-slot reservation, independent bounded worker process |
| Page publishing | Prior SocialPublisher registry had no real publisher | New official Meta Graph/Reels adapter and DPAPI Page tokens; only fake-origin private HTTP fixtures were exercised so far |
| Test and scale evidence | Existing Rust/Node suites | Deterministic scheduling/claims at 9, 30, 100 and 1,000 jobs; real local FFmpeg fixture. This is orchestration/QC evidence, not a 1,000-video paid-generation benchmark |

## State guarantees

A draft is inert. The operator reviews and approves a hash and revision before worker claims. The worker persists a lease before work, checks the approved version and budget before provider calls, and bounds concurrency. AI-written plans cannot silently modify locked fields. A timeout or lost reply from a paid generation or Facebook write becomes `Unknown`; the system does not replay that write. Facebook success requires Meta status plus the matching video ID and public permalink. Unknown already-dispatched publications may be reconciled by bounded, read-only status polling.

Campaign duration is a per-video field. Optional completion deadline is a distinct UTC instant derived from the selected IANA timezone. Page lines accept `PAGE_ID | topic A, topic B`, allowing the scheduler to distribute topics to compatible Pages. Quotas and slots are held globally across campaigns; DST gaps are skipped and repeated local times count once.

## Verification snapshot

- `concat-studio` campaign unit tests and private-origin Meta/DPAPI tests; real media test explicitly invokes installed local FFmpeg and cleans its own fixtures.
- MCP contract tests use an in-memory MCP transport and assert the existing server surface plus the four constrained campaign tools.
- Native app/UI still requires the current Windows SDK build and final launch smoke. No screenshot automation has been claimed.
- No actual Grok generation, authenticated Page connection, external publication, or end-to-end campaign was performed. Live setup requires operator-selected Nexora profile/assets and a Meta app with the documented official Page permissions. Never treat UI setup state as a verified publish.

## Scale and acceptance

A passing scheduler/lease test proves 1,000 IDs can be planned and assigned under its configured slot capacity. It does not certify time, memory, GPU, provider rate limits, fees, rendition quality for 1,000 paid assets, or Facebook quota behavior. Acceptance still requires staged campaigns with real connected services and operator authorization for a small Page publish before increasing volume. The current standalone worker resumes a persisted approved campaign when started again; automatic OS-start registration is not included.
