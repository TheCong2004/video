import test from "node:test";
import assert from "node:assert/strict";
import { hardenRuntime, hardenedChunk } from "./harden-local-ai.mjs";
import { fileURLToPath } from "node:url";

test("unknown or unauthenticated substitute bundles fail closed", () => {
  for (const source of ["", "async function o(a){return!1}", "async function o(a){return!0}"]) {
    assert.throws(() => hardenedChunk(source), /Unrecognized/);
  }
});

test("staged runtime matches the full reviewed hardened auth bundle", () => {
  const root = fileURLToPath(new URL("../.deps/omniroute", import.meta.url));
  const hash = hardenRuntime(root, true);
  assert.match(hash, /^[a-f0-9]{64}$/);
});
