import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const read=p=>readFileSync(new URL(`../${p}`,import.meta.url),'utf8');
test('AI Edit exposes clear external/API modes and a real no-key import/export route',()=>{
  const ui=read('src/crates/concat/ui/workspace/ai-editing-pane.slint');
  for(const callback of ['toggle-bridge','save-mcp-config','export-context','import-plan']) assert.match(ui,new RegExp(`AiEditing\\.${callback}\\(\\)`));
  assert.match(ui,/external-mode: true/);
  assert.match(ui,/enabled: !AiEditing.busy && AiEditing.preview-ready/);
  assert.match(ui,/Chỉ chia sẻ cấu trúc và thời gian trên timeline/);
  assert.match(ui,/AI không tự xem hình hoặc nghe video/);
  const native=read('src/crates/concat/src/ai_editing.rs');
  assert.match(native,/plan\.compile\(&snapshot\)/);
  assert.match(native,/scope != format!/);
  assert.ok(/stage_or_commit_draft\(\s*app,\s*studio,\s*models,\s*&scope/.test(native));
  assert.ok(native.includes('generation != import_generation'));
  assert.ok(native.includes('import_epoch != host::project_epoch()'));
  assert.doesNotMatch(native,/sign in through Studio settings/);
});

test('web handoff is explicit, updates typed prompts and never scrapes chat sessions',()=>{
  const ui=read('src/crates/concat/ui/workspace/ai-editing-pane.slint');
  const native=read('src/crates/concat/src/ai_editing.rs');
  assert.match(ui,/edited\(text\) => \{ AiEditing\.prompt = text; \}/);
  assert.match(ui,/edited\(text\) => \{ AiEditing\.web-result = text; \}/);
  assert.match(ui,/copy-buffer\.copy\(\)/);
  assert.match(native,/on_submit_web_result/);
  assert.match(native,/let text = ui\.get_web_result\(\)\.to_string\(\);\s*let parsed = concat_studio::edit_bridge::parse_external_draft\(&text\)/);
  assert.match(native,/"chatgpt" => "https:\/\/chatgpt.com\/"/);
  assert.doesNotMatch(native,/document\.cookie|Runtime\.evaluate|chatgpt.com\/backend-api/);
});
test('MCP has no approval tool or independent editor executor',()=>{
  const adapter=read('tools/ai-edit-mcp/server.mjs');
  assert.equal((adapter.match(/registerTool\(/g)||[]).length,3);
  assert.doesNotMatch(adapter,/registerTool\(['"](?:approve|concat_approve|execute|concat_execute)/);
  const bridge=read('src/crates/concat-studio/src/edit_bridge.rs');
  assert.match(bridge,/TcpListener::bind\("127\.0\.0\.1:0"\)/);
  assert.match(bridge,/crate::auth::protect/);
  assert.match(bridge,/try_send\(pending\)/);
  assert.match(bridge,/active\(&self\)/);
  assert.doesNotMatch(bridge,/commit_draft|Command::|execute_command/);
});
test('distribution contains SDK adapter and pinned portable dependencies',()=>{
  const pack=read('scripts/windows-package.ps1');
  assert.match(pack,/tools\\ai-edit-mcp/);
  assert.match(pack,/Bundling AI Edit MCP runtime failed/);
  const packageJson=JSON.parse(read('tools/ai-edit-mcp/package.json'));
  assert.match(packageJson.dependencies['@modelcontextprotocol/sdk'],/^\d+\.\d+\.\d+$/);
  assert.match(read('tools/ai-edit-mcp/.npmrc'),/node-linker=hoisted/);
});
