// SPDX-License-Identifier: MIT
// Edge TTS remains the provider. Each request has a deadline, one bounded
// retry for transport failures, and closes its streams/socket on every path.
import { MsEdgeTTS, OUTPUT_FORMAT } from 'msedge-tts';

const error = (code) => Object.assign(new Error(code), { code });
const escapeXml = value => String(value).replace(/[<>&"']/g, c => ({'<':'&lt;','>':'&gt;','&':'&amp;','"':'&quot;',"'":'&apos;'}[c]));

export async function synthesize(text, settings, {
  signal, timeoutMs = 15_000, maxAttempts = 2,
  createClient = () => new MsEdgeTTS({ connectionTimeoutMs: 8_000, synthesisTimeoutMs: 14_000 }),
} = {}) {
  const attempts = Math.max(1, Math.min(2, maxAttempts));
  for (let attempt = 0; attempt < attempts; attempt++) {
    if (signal?.aborted) throw error('TTS_CANCELLED');
    let client, stream, timer, abort, cancelled = false;
    try {
      client = createClient();
      const deadline = new Promise((_, reject) => {
        timer = setTimeout(() => reject(error('TTS_TIMEOUT')), timeoutMs);
        abort = () => reject(error('TTS_CANCELLED'));
        signal?.addEventListener('abort', abort, { once: true });
      });
      const work = (async () => {
        await client.setMetadata(settings.voice, OUTPUT_FORMAT.AUDIO_24KHZ_96KBITRATE_MONO_MP3);
        if (cancelled || signal?.aborted) { try { client.close(); } catch {} throw error('TTS_CANCELLED'); }
        stream = client.toStream(escapeXml(text), {
          pitch: `${settings.pitch >= 0 ? '+' : ''}${settings.pitch}Hz`,
          rate: `${settings.rate >= 0 ? '+' : ''}${settings.rate}%`,
        }).audioStream;
        return new Promise((resolve, reject) => {
          const chunks = []; let bytes = 0, finished = false;
          const fail = e => { if (!finished) { finished = true; reject(e); } };
          stream.on('data', chunk => {
            bytes += chunk.length;
            if (bytes > 8 * 1024 * 1024) { fail(error('TTS_TOO_LARGE')); stream.destroy(); return; }
            chunks.push(chunk);
          });
          stream.once('end', () => {
            if (finished) return;
            finished = true;
            if (!bytes) reject(error('TTS_EMPTY_AUDIO')); else resolve(Buffer.concat(chunks));
          });
          stream.once('error', fail);
          stream.once('close', () => { if (!finished) fail(error('TTS_PREMATURE_CLOSE')); });
        });
      })();
      return await Promise.race([work, deadline]);
    } catch (failure) {
      const code = String(failure?.code || failure?.cause?.code || failure?.message || 'TTS_FAILED');
      const transient = /ECONNRESET|ETIMEDOUT|EAI_AGAIN|TTS_TIMEOUT|PREMATURE_CLOSE|timed out|no turn.end/i.test(code);
      if (signal?.aborted || code === 'TTS_CANCELLED') throw error('TTS_CANCELLED');
      if (!transient || attempt + 1 === attempts) throw error(transient ? (code.includes('ECONNRESET') ? 'TTS_CONNECTION_RESET' : 'TTS_TIMEOUT_OR_CLOSED') : 'TTS_FAILED');
    } finally {
      cancelled = true;
      clearTimeout(timer);
      signal?.removeEventListener('abort', abort);
      try { stream?.destroy(); } catch {}
      try { client?.close(); } catch {}
    }
  }
  throw error('TTS_FAILED');
}
