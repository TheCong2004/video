// Reproducible hardening of the already-built ArtCraft standalone runtime.
// No provider/database files are opened. Unknown upstream builds fail closed.
import { readFileSync, writeFileSync, readdirSync } from "node:fs";
import { resolve, join } from "node:path";
import { createHash } from "node:crypto";
import { pathToFileURL } from "node:url";

const originalHash = "7d854b10058651e8dc3cdc0876861e9d4db81087b93926a87c60fc76c5b60356";
const original = "async function o(a){return!1}h=(i.then?(await i)():i)[0],d()}catch(a){d(a)}})},818366:";
const secured = original.replace("return!1", "return!0");
const moduleEnd = "async function o(a){return!1}h=(i.then?(await i)():i)[0],d()}catch(a){d(a)}})}";
const secureModuleEnd = moduleEnd.replace("return!1", "return!0");
const disableCli = 'process.env.OMNIROUTE_DISABLE_CLI_TOKEN = "true";\n';
const digest = value => createHash("sha256").update(value).digest("hex");

export function hardenedChunk(source) {
  if (digest(source) === originalHash && source.split(original).length === 2) {
    return source.replace(original, secured);
  }
  // Idempotent, but still bind the ENTIRE artifact to the known input.
  if (source.split(secured).length === 2 && digest(source.replace(secured, original)) === originalHash) {
    return source;
  }
  throw new Error("Unrecognized OmniRoute auth bundle. Rebuild/review upstream auth; do not ship it unchecked.");
}

export function hardenRuntime(root, checkOnly = false) {
  const chunk = join(resolve(root), ".build/next/server/chunks/74251.js");
  const server = join(resolve(root), "server.js");
  const before = readFileSync(chunk, "utf8");
  const after = hardenedChunk(before);
  const entry = readFileSync(server, "utf8");
  // Webpack duplicates the shared auth module into route entry bundles. Fixing
  // just the shared chunk leaves some management endpoints anonymous.
  const changes = [];
  function scan(dir) {
    for (const item of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, item.name);
      if (item.isDirectory()) scan(path);
      else if (item.name.endsWith(".js")) {
        const text = readFileSync(path, "utf8");
        if (!text.includes("769405:")) continue;
        const oldCount = text.split(moduleEnd).length - 1;
        const newCount = text.split(secureModuleEnd).length - 1;
        if (!text.includes("ux:()=>o") || oldCount + newCount !== 1) {
          throw new Error("Unrecognized duplicated OmniRoute auth module; refuse unchecked packaging.");
        }
        if (oldCount) changes.push([path, text.replace(moduleEnd, secureModuleEnd)]);
      }
    }
  }
  scan(join(resolve(root), ".build/next/server"));
  if (checkOnly) {
    if (changes.length || before !== after || !entry.startsWith(disableCli)) {
      throw new Error("OmniRoute runtime is not hardened; run stage-local-ai.ps1 first.");
    }
  } else {
    for (const [path, text] of changes) writeFileSync(path, text);
    if (!entry.startsWith(disableCli)) writeFileSync(server, disableCli + entry);
  }
  return digest(after);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (!process.argv[2]) throw new Error("Expected standalone runtime directory");
  console.log(`LOCAL_AI_AUTH_SHA256=${hardenRuntime(process.argv[2], process.argv.includes("--check"))}`);
}
