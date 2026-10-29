import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
const read=p=>readFileSync(new URL(p,import.meta.url),'utf8');
test('large native UI avoids incremental snapshots without changing dependency profiles',()=>{
  const cargo=read('../src/Cargo.toml');
  assert.match(cargo,/\[profile.dev.package.concat\]\s+incremental = false/);
  assert.match(cargo,/\[profile.dev.package\."\*"\]\s+opt-level = 2/);
});
test('launcher defaults to one job and rejects low-disk compilation, not RunBuilt',()=>{
  const launcher=read('./windows-studio.ps1');
  assert.match(launcher,/if \(!\$env:CARGO_BUILD_JOBS\) \{ \$env:CARGO_BUILD_JOBS = '1' \}/);
  assert.match(launcher,/if \(!\$RunBuilt\)\s*\{[\s\S]*?AvailableFreeSpace -lt 4GB/);
});
test('explicit incremental cleanup retains compiler, lock and path safeguards',()=>{
  const cleanup=read('./cleanup-build.ps1');
  assert.match(cleanup,/\[switch\]\$PruneMainIncremental/);
  assert.match(cleanup,/if \(\$busy.Count\).*return/);
  assert.match(cleanup,/\[IO.FileShare\]::None/);
  assert.match(cleanup,/\$resolved = Assert-CleanupPath \$candidate/);
  assert.match(cleanup,/if \(\$PruneMainIncremental\)[\s\S]*?foreach \(\$dir in \$dirs\)/);
});
