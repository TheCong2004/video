// Rebuild only Concat's controller; the staged connector/vendor code is unchanged.
import { readFileSync, writeFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export function compileController(runtimeRoot = resolve(root, '.deps/tiktok-live-connector')) {
  const source = readFileSync(resolve(root, 'integrations/tiktok-live/artcraft-live-runtime-controller.ts'), 'utf8');
  const protocol = resolve(runtimeRoot, 'corresponding-source/src/artcraft-live-protocol.ts');
  const protocolJs = stripTypeScriptTypes(readFileSync(protocol, 'utf8'), { mode: 'transform' });
  const protocolUrl = `data:text/javascript;base64,${Buffer.from(protocolJs).toString('base64')}`;
  const standalone = source.replace(/from '\.\/artcraft-live-protocol\.js'/, `from '${protocolUrl}'`);
  return { source, js: stripTypeScriptTypes(source, { mode: 'transform' }), standalone: stripTypeScriptTypes(standalone, { mode: 'transform' }) };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const runtimeRoot = resolve(process.argv[2] || resolve(root, '.deps/tiktok-live-connector'));
  const { source, js } = compileController(runtimeRoot);
  const target = resolve(runtimeRoot, 'dist/artcraftLiveRuntime.js');
  const bundle = readFileSync(target, 'utf8');
  const start = bundle.indexOf('//#region src/artcraft-live-runtime-controller.ts');
  const end = bundle.indexOf('//#region src/artcraft-live-runtime.ts', start);
  if (start < 0 || end < 0) throw new Error('Unsupported staged LIVE runtime; controller markers missing');
  const body = js.replace(/^import[\s\S]*?from ['"]\.\/artcraft-live-protocol\.js['"];?\s*/m, '')
    .replace('export class ArtCraftLiveRuntimeController', 'class ArtCraftLiveRuntimeController');
  writeFileSync(target, bundle.slice(0, start) + '//#region src/artcraft-live-runtime-controller.ts\n' + body + '\n//#endregion\n' + bundle.slice(end));
  writeFileSync(resolve(runtimeRoot, 'corresponding-source/src/artcraft-live-runtime-controller.ts'), source);
  console.log('LIVE_CONTROLLER_STAGED');
}
