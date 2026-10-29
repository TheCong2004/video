// Serialize manual sign-in and automated work in the same app-owned profile.
import { open, readFile, unlink, mkdir } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import path from 'node:path';

function processExists(pid) {
  try { process.kill(pid, 0); return true; }
  catch (error) { return error.code !== 'ESRCH'; }
}
function busy() { return Object.assign(new Error('Browser profile is already in use'), { code: 'PROFILE_BUSY' }); }

export async function acquireBrowserProfile(profile, { isAlive = processExists } = {}) {
  if (!path.isAbsolute(profile)) throw new Error('Profile path must be absolute');
  await mkdir(profile, { recursive: true });
  const file = path.join(profile, 'concat-browser-owner.json');
  const token = randomUUID();
  for (let attempt = 0; attempt < 3; attempt++) {
    let handle;
    try { handle = await open(file, 'wx'); }
    catch (error) {
      if (error.code !== 'EEXIST') throw error;
      const content = await readFile(file, 'utf8').catch(() => '');
      let owner;
      try { if (content.length > 1024) throw busy(); owner = JSON.parse(content); } catch { throw busy(); }
      if (!Number.isSafeInteger(owner.pid) || owner.pid <= 0 || typeof owner.token !== 'string' || isAlive(owner.pid)) throw busy();
      // Serialize stale recovery as well: two recoverers must never unlink a
      // new lease created between checking and removing the old file.
      const recoveryFile = file + '.recovery';
      const recovery = await open(recoveryFile, 'wx').catch(() => { throw busy(); });
      try {
        if (await readFile(file, 'utf8').catch(() => '') !== content || isAlive(owner.pid)) throw busy();
        await unlink(file).catch(error => { if (error.code !== 'ENOENT') throw error; });
      } finally { await recovery.close(); await unlink(recoveryFile); }
      continue;
    }
    try { await handle.writeFile(JSON.stringify({ pid: process.pid, token })); }
    catch (error) { await handle.close(); await unlink(file).catch(() => {}); throw error; }
    await handle.close();
    let released = false;
    return async () => {
      if (released) return;
      released = true;
      const owner = await readFile(file, 'utf8').then(JSON.parse).catch(() => null);
      if (owner?.token === token) await unlink(file).catch(error => { if (error.code !== 'ENOENT') throw error; });
    };
  }
  throw busy();
}
