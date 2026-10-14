import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

// Structural regression checks, not a substitute for interactive UI testing.
const read = (path) => readFileSync(new URL(`../src/crates/concat/${path}`, import.meta.url), "utf8");
const start = read("ui/start.slint");
const media = read("ui/workspace/media-pane.slint");
const studio = read("ui/workspace/studio-pane.slint");
const app = read("ui/app.slint");
const rail = start.slice(start.indexOf("component Rail inherits"), start.indexOf("component NewProjectCard"));
const launcher = start.slice(start.indexOf("export component StartScreen"), start.indexOf("export component NewProjectDialog"));

test("all twelve Studio routes live in the launcher rail below New project", () => {
  const firstRoute = rail.indexOf("StudioRoute.overview");
  assert.ok(firstRoute > rail.indexOf('I18n.t("start.newProject")'));
  for (const route of ["overview", "production", "opentalking", "workflows", "scripts", "bulk", "jobs", "channels", "publish", "history", "settings", "keys"]) {
    assert.match(rail, new RegExp(`StudioRoute\\.${route}\\b`));
  }
  assert.match(rail, /callback studio-picked\(route: StudioRoute\)/);
  assert.match(rail, /PaneScroll/);
  assert.match(rail, /callback projects\(\)/);
});

test("waiting workflows can resume only through the confirmed native action", () => {
  const production = read("ui/workspace/studio-production.slint");
  const binding = read("src/studio_production.rs");
  const adapter = read("../concat-studio/src/workflows.rs");
  assert.match(production, /if run\.status == "waiting": StudioButton \{ label: I18n\.t\("nativeStudio\.resumeJob"\)/);
  assert.match(production, /I18n\.t\("nativeStudio\.resumeJob"\)/);
  assert.match(production, /if ProductionStudio\.resume-id != ""/);
  assert.match(binding, /on_resume_run/);
  assert.match(binding, /Action::Resume/);
  assert.match(adapter, /workflow-runs\/\{path\}\/resume/);
  assert.match(adapter, /resumed\.id != run_id/);
});

test("OpenTalking generation is native and imports only into the captured project", () => {
  const opentalking = read("ui/workspace/studio-opentalking.slint");
  const binding = read("src/studio_opentalking.rs");
  const adapter = read("../concat-studio/src/opentalking.rs");
  assert.match(studio, /StudioRoute\.opentalking: StudioOpenTalkingPage/);
  assert.match(opentalking, /StudioOpenTalking\.confirm-create = true/);
  assert.match(opentalking, /StudioOpenTalking\.delete-id = job\.id/);
  assert.match(opentalking, /StudioOpenTalking\.delete\(StudioOpenTalking\.delete-id\)/);
  assert.match(binding, /session\.path\(\) != expected_project\.as_str\(\)/);
  assert.match(binding, /MediaMsg::Import\(vec!\[path\]\)/);
  assert.match(adapter, /MAX_VIDEO_BYTES/);
  assert.match(adapter, /OpenTalkingClient/);
});

test("Media no longer owns any Studio route or rendering state", () => {
  assert.doesNotMatch(media, /StudioPage|StudioRoute|StudioBridge|studio-open|productionStudio\./);
  for (const filter of ["all", "video", "audio", "images", "speech", "processed"]) {
    assert.ok(media.includes(`current: root.filter == MediaFilter.${filter};`));
  }
  assert.match(media, /callback import-media\(\)/);
});

test("Studio opens on the start screen without a project/session guard", () => {
  assert.match(app, /if root\.on-start: StartScreen/);
  assert.match(launcher, /studio-picked\(route\) => \{ root\.studio-route = route; root\.studio-open = true; \}/);
  assert.match(launcher, /if root\.studio-open: PaneScroll/);
  assert.match(launcher, /if !root\.studio-open: Rectangle/);
  assert.match(launcher, /projects => \{ root\.studio-open = false; \}/);
  assert.match(launcher, /if !root\.narrow \|\| !root\.phone: Rail/);
});

test("project-free Studio has an honest project chooser, not a dead import action", () => {
  assert.match(studio, /callback choose-project\(\)/);
  assert.match(studio, /productionStudio\.chooseProjectHint/);
  assert.doesNotMatch(studio, /import-artifact|back-to-media/);
  assert.match(launcher, /choose-project => \{ root\.studio-open = false; \}/);
  assert.match(studio, /StudioBridge\.refresh-workflows\(\)/);
  assert.match(studio, /StudioBridge\.check-services\(\)/);
});

test("script route owns a real local CRUD editor, with explicit delete confirmation", () => {
  const scripts = read("ui/workspace/studio-scripts.slint");
  const binding = read("src/studio_scripts.rs");
  assert.match(studio, /if root\.route == StudioRoute\.scripts: StudioCard \{ StudioScripts/);
  for (const action of ["refresh", "save-script", "delete-script"]) {
    assert.ok(scripts.includes(`ScriptLibrary.${action}(`));
  }
  assert.match(scripts, /if ScriptLibrary\.confirming-delete/);
  assert.match(scripts, /!ScriptLibrary\.busy && !ScriptLibrary\.dirty/);
  assert.match(binding, /store\.save\(draft\)/);
  assert.match(binding, /store\.delete\(&id, revision\)/);
  assert.match(binding, /upgrade_in_event_loop/);
  assert.match(app, /ScriptLibrary, StudioPromptRow/);
});

test("production is a native form with a separate explicit confirmation boundary", () => {
  const production = read("ui/workspace/studio-production.slint");
  const binding = read("src/studio_production.rs");
  assert.match(studio, /StudioProduction \{ \}/);
  assert.match(production, /if ProductionStudio\.confirm-start: StudioCard/);
  assert.match(production, /ProductionStudio\.ready && !ProductionStudio\.uncertain/);
  assert.match(binding, /current == prepared/);
  assert.match(binding, /RunError::UnknownOutcome\s*\{\s*state\.set_uncertain\(true\)/);
  assert.doesNotMatch(production + binding, /WebView|webview|<iframe/);
});

test("service diagnostics are a settings section, not every page's shared content", () => {
  const settings = studio.indexOf("if root.route == StudioRoute.settings: StudioCard");
  const diagnostics = studio.indexOf('I18n.t("productionStudio.connectionScope")');
  assert.ok(settings >= 0 && diagnostics > settings);
  assert.ok(studio.indexOf('I18n.t("productionStudio.migrationNotice")') > settings);
});

test("scroll content uses the available width rather than its text's preferred width", () => {
  const pane = read("ui/workspace/pane.slint");
  assert.match(pane, /content := VerticalLayout \{\s*y: root\.at;\s*width: parent\.width - 12px;/);
});

test("AI Keys owns native actions with separate connection and routed chat confirmation", () => {
  const ai = read("ui/workspace/studio-ai.slint");
  const binding = read("src/studio_ai.rs");
  assert.match(studio, /if root\.route == StudioRoute\.keys: StudioAiPage/);
  for (const action of ["load-providers", "load-models", "add-provider", "test-connection", "change-provider", "delete-provider", "ask"]) {
    assert.ok(ai.includes(`StudioAi.${action}(`));
  }
  assert.match(ai, /secret: true/);
  assert.match(ai, /if StudioAi\.confirm-ask/);
  assert.match(ai, /if StudioAi\.delete-id != ""/);
  assert.match(binding, /set_new_key\(""\.into\(\)\)/);
  assert.doesNotMatch(binding, /println!|log::|fs::write/);
});

test("browser lifecycle is native, explicit and separate from generation readiness", () => {
  const browser = read("ui/workspace/studio-browser.slint");
  const production = read("ui/workspace/studio-production.slint");
  const binding = read("src/studio_browser.rs");
  assert.match(production, /StudioBrowserPanel \{ \}/);
  assert.match(browser, /StudioBrowser\.confirm-stop \? I18n/);
  assert.match(binding, /get_confirm_stop\(\)/);
  assert.match(browser, /studioBrowser\.noReadiness/);
  assert.doesNotMatch(binding, /Command::|taskkill|Stop-Process/);
});

test("LIVE exposes the actual AI model and OpenTalking voice used by auto-reply", () => {
  const live = read("ui/workspace/studio-live.slint");
  const binding = read("src/studio_live.rs");
  assert.match(live, /options: StudioAi\.models; current <=> StudioAi\.model-index/);
  assert.match(live, /StudioAi\.load-models\(\)/);
  assert.match(live, /options: StudioOpenTalking\.voice-options; current <=> StudioOpenTalking\.voice-index/);
  assert.match(live, /StudioOpenTalking\.refresh\(\)/);
  assert.match(live, /for comment in StudioLive\.comments/);
  assert.match(live, /StudioLive\.reply-to-comment\(comment\.username, comment\.content\)/);
  assert.match(binding, /get_model_index\(\)/);
  assert.match(binding, /get_voice_index\(\)/);
  assert.match(binding, /synthesize_reply\(&reply, &voice\)/);
  assert.match(binding, /play_tts\(playback, audio\)/);
  assert.match(binding, /on_reply_to_comment/);
  assert.match(binding, /let ai = app\.global::<StudioAi>\(\)/);
  assert.match(binding, /let talking = app\.global::<StudioOpenTalking>\(\)/);
  assert.match(binding, /send_message\(&reply\)/);
  assert.match(binding, /while history\.len\(\) > 30/);
});

test("Channels uses Concat's native page catalog rather than the Donut browser panel", () => {
  const pagesUi = read("ui/workspace/studio-pages.slint");
  const binding = read("src/studio_pages.rs");
  const store = read("../concat-studio/src/pages.rs");
  assert.match(studio, /StudioRoute\.channels: StudioPagesPanel/);
  assert.doesNotMatch(studio, /StudioRoute\.channels: StudioBrowserPanel/);
  for (const action of ["load-pages", "new-page", "save-page", "select-page", "archive-page"]) {
    assert.ok(pagesUi.includes(`StudioChannels.${action}(`));
  }
  assert.match(binding, /ContentPageStore::new/);
  assert.match(binding, /store\.save\(draft\)/);
  assert.match(binding, /store\.archive\(&id, revision, archived\)/);
  assert.match(store, /try_lock\(\)/);
  assert.match(store, /revision != draft\.revision/);
  assert.match(store, /page catalog is damaged; it has not been overwritten/);
});
