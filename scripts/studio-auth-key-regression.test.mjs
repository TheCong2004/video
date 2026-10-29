import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

// Wiring checks supplement (not replace) the Rust HTTP/DPAPI fixture tests.
const read = p => readFileSync(new URL(`../src/crates/${p}`, import.meta.url), "utf8");

test("all main native surfaces open locally without restoring or fabricating a cloud session", () => {
  const app = read("concat/ui/app.slint");
  for (const surface of ["StartScreen", "Workspace", "PhoneEditor"]) {
    assert.match(app, new RegExp(`if [^\\n]*: ${surface}`));
  }
  assert.doesNotMatch(app, /StudioLoginScreen|if [^\n]*StudioSession\.signed-in/);
  assert.doesNotMatch(read("concat/ui/workspace/studio-pane.slint"), /StudioSessionPanel|label: "Local Studio"/);
  assert.doesNotMatch(read("concat/ui/workspace/studio-session.slint"), /export component/);
  const session = read("concat/src/studio_session.rs");
  assert.doesNotMatch(session, /invoke_restore|AuthManager|LoginAttempt|QuickLogin|set_signed_in\(true\)/);
  assert.match(session, /set_signed_in\(false\)/);
  assert.doesNotMatch(session, /sleep\(.*from_millis\((500|600)\)/);
});

test("local edition does not authorize anonymous access to public Promax", () => {
  const session = read("concat/src/studio_session.rs");
  assert.match(session, /Err\(WorkflowError::Configuration\)/);
  assert.doesNotMatch(session, /CONCAT_PROMAX_ACCESS_TOKEN|\.account\(\)|\.client\(\)/);
});

test("new key opens a visible form and dispatches the existing authenticated mutation", () => {
  const ui = read("concat/ui/workspace/studio-ai.slint");
  const rust = read("concat/src/studio_ai.rs");
  assert.match(ui, /if !StudioAi\.new-open: StudioCard/);
  assert.match(ui, /if StudioAi\.new-open:/);
  assert.match(ui, /StudioAi\.quick-add\("gemini"\)/);
  assert.match(ui, /StudioAi\.add-provider\(\)/);
  assert.match(ui, /text: StudioAi\.busy \? [^\n]+ : StudioAi\.status/);
  assert.match(rust, /crate::studio_local_ai::client\(\)/);
  assert.match(rust, /\.ai_add_provider\(&provider, &name, &key, &model\)/);
  assert.doesNotMatch(rust + ui, /sk-omniroute-master-key-prod-01|http:\/\/127\.0\.0\.1:20128/);
});

test("email auth exchanges a verified identity, never an email-only bootstrap", () => {
  const auth = read("concat-studio/src/auth.rs");
  assert.match(auth, /\/auth\/v1\/token\?grant_type=password/);
  assert.match(auth, /\/auth\/v1\/signup/);
  assert.match(auth, /\/api\/v1\/auth\/supabase\/exchange/);
  assert.doesNotMatch(auth, /"\/api\/v1\/auth\/session"/);
  assert.match(auth, /Registration::ConfirmationRequired/);
});

test("local runtime belongs to the local workspace and shutdown, not the cloud worker", () => {
  const owner = read("concat/src/studio_local_ai.rs");
  const session = read("concat/src/studio_session.rs");
  const runtime = read("concat-studio/src/local_ai.rs");
  assert.match(session, /studio_local_ai::set_owner\("concat-local-workspace-v1"\)/);
  assert.match(read("concat/src/lib.rs"), /studio_local_ai::shutdown\(\)/);
  assert.match(owner, /generation.*owner\.generation/);
  assert.match(runtime, /JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE/);
  assert.match(runtime, /lock\.try_lock\(\)/);
  assert.match(runtime, /"HOSTNAME", "127\.0\.0\.1"/);
  assert.doesNotMatch(runtime, /Stop-Process|taskkill|\.set\("Authorization"/);
  for (const path of ["concat/src/ai_editing.rs", "concat/src/studio_live.rs"]) {
    assert.match(read(path), /studio_local_ai::client\(\)/);
  }
});

test("ask button opens the confirmation that its native guard actually requires", () => {
  const ui = read("concat/ui/workspace/studio-ai.slint");
  assert.match(ui, /clicked => \{ StudioAi\.confirm-ask = true; \}/);
  assert.match(ui, /if StudioAi\.confirm-ask: VerticalLayout/);
  assert.match(ui, /clicked => \{ StudioAi\.ask\(\); \}/);
  assert.match(read("concat/src/studio_ai.rs"), /!state\.get_confirm_ask\(\)/);
});

test("AI actions use the owned API and cannot fabricate a successful test or answer", () => {
  const rust = read("concat/src/studio_ai.rs");
  assert.match(rust, /client\.ai_test_connection\(&id, &model\)/);
  assert.match(rust, /client\.ai_ask\(&model, &prompt\)/);
  assert.doesNotMatch(rust, /Reply::Tested\(true\)|CLOUD_PROVIDERS|save_stored_providers|KỊCH BẢN ĐỀ XUẤT/);
  assert.match(rust, /generation != crate::studio_local_ai::generation\(\)/);
  const clientOffset = rust.indexOf("let client = crate::studio_local_ai::client()");
  const actionOffset = rust.indexOf("match action", clientOffset);
  assert.match(rust.slice(clientOffset, actionOffset), /generation\.is_none\(\).*generation != crate::studio_local_ai::generation\(\)/s);
  assert.match(read("concat-studio/src/legacy_keys.rs"), /auth::protect/);
});

test("Nexora settings and diagnostics share the configured local client without OAuth", () => {
  const browser = read("concat/src/studio_browser.rs");
  assert.match(browser, /on_save_connection/);
  assert.match(browser, /on_choose_executable/);
  assert.match(browser, /on_start/);
  assert.doesNotMatch(read("concat/src/studio_session.rs"), /authorize_url|open_https/);
  assert.match(read("concat/src/studio_bridge.rs"), /studio_browser::client\(\)/);
  assert.doesNotMatch(read("concat/src/studio_bridge.rs"), /chay-nexora\.bat/);
});

test("Windows distribution includes TikTok connector and its Node runtime", () => {
  const packageScript = readFileSync(new URL("./windows-package.ps1", import.meta.url), "utf8");
  assert.match(packageScript, /tiktok-live-connector/);
  assert.match(packageScript, /resources\\node/);
  assert.match(read("concat-studio/src/live_runtime.rs"), /packaged_connector_handshakes_without_joining_or_posting/);
});
