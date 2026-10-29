import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = path => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
const palette = name => Object.fromEntries([...read(`src/crates/concat/ui/theme/${name}.slint`).matchAll(/out property <color> ([\w-]+):\s*(#[\da-f]{6});/gi)].map(([, key, hex]) => [key, hex]));
const luminance = hex => {
  const channels = hex.slice(1).match(/../g).map(s => parseInt(s, 16) / 255).map(v => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4);
  return channels[0] * .2126 + channels[1] * .7152 + channels[2] * .0722;
};
const contrast = (a, b) => { const values = [luminance(a), luminance(b)].sort((a, b) => b-a); return (values[0]+.05)/(values[1]+.05); };
for (const mode of ['light', 'dark']) {
  test(`${mode} text, brand actions and selected navigation meet normal text contrast`, () => {
    const p = palette(mode);
    for (const [ink, ground] of [['fg', 'launch-panel'], ['fg-muted', 'launch-panel'], ['on-brand', 'brand'], ['nav-selected-ink', 'nav-selected']]) {
      assert.ok(p[ink] && p[ground], `missing ${ink}/${ground}`);
      assert.ok(contrast(p[ink], p[ground]) >= 4.5, `${mode} ${ink}/${ground} contrast=${contrast(p[ink], p[ground]).toFixed(2)}`);
    }
    assert.equal(p.stage, '#000000', 'video preview stays neutral black');
  });
}
test('Studio compatibility palette delegates to the shared theme', () => {
  const source = read('src/crates/concat/ui/theme/floword.slint');
  assert.doesNotMatch(source, /#[\da-f]{6}/i);
  assert.match(source, /reduced-motion <=> Theme\.reduced-motion/);
});
test('UI preference callbacks save through existing configuration, not a new store', () => {
  const source = read('src/crates/concat/src/lib.rs');
  for (const action of ['sidebar_mode', 'reduced_motion']) {
    assert.ok(source.includes(`prefs.${action}`));
  }
  assert.ok(source.includes('studio.prefs.save(&studio.host.dirs)'));
});
