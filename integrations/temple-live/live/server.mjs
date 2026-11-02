// Chánh Điện Tịnh Tâm — live server
// Runs on the streaming PC. Serves the overlay + control panel, listens to a TikTok LIVE room,
// keeps the list of seated viewers, generates Vietnamese voice lines with Edge TTS and pushes
// everything to the overlay over WebSocket.
//
//   cd live && npm install && node server.mjs        → http://localhost:3000/live/control.html
//
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { WebSocketServer } from 'ws';
import { synthesize } from './tts-service.mjs';
import { liveUser, liveChat, liveGift } from './tiktok-events.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, '..');            // repo root: serves /assets, /live
const DATA_DIR = path.resolve(process.env.CONCAT_TEMPLE_DATA_DIR || path.join(ROOT, 'data'));
const TTS_DIR = path.join(DATA_DIR, 'tts');
const STATE_FILE = path.join(DATA_DIR, 'state.json');
fs.mkdirSync(TTS_DIR, { recursive: true });
const PORT = +process.env.PORT || 3088;
let initial = {};
try {
  const source = process.env.CONCAT_TEMPLE_HANDOVER || STATE_FILE;
  if (fs.statSync(source).size <= 256 * 1024) initial = JSON.parse(fs.readFileSync(source, 'utf8'));
} catch {}
const CAPABILITIES = ['control-v1', 'likes', 'skins', 'pin', 'stop-speech', 'tts-deadline', 'bots-state'];

/* ------------------------------------------------------------------ state */
const settings = {
  title: 'Chánh Điện Tịnh Tâm',
  subtitle: 'Nam Mô A Di Đà Phật',
  voice: 'vi-VN-NamMinhNeural',      // or vi-VN-HoaiMyNeural
  pitch: -18,                        // Hz
  rate: -20,                         // %
  music: true, mo: true, moBpm: 52,
  autoChant: true, chantEveryMin: 3,
  seatMode: 'auto',                  // auto | 24 | 36 | 64 | 100
  joinOnEnter: false,                // also seat viewers who only enter the room
  gifts: { readAll: true, minDiamonds: 1 },
};
const viewers = new Map();           // id -> { id, name, skin, avatar, coins, joinedAt, lastAt, lastText, lastTextAt, demo }
const startedAt = Number.isFinite(initial.startedAt) && initial.startedAt > 0 && initial.startedAt <= Date.now() ? initial.startedAt : Date.now();
let pinned = null, simulationSkin = '';
let persistence = Promise.resolve();
function persist(){
  const data = JSON.stringify({ version:1, settings, startedAt, pinned, simulationSkin });
  persistence = persistence.catch(() => {}).then(async () => {
    const temporary = STATE_FILE + '.tmp';
    await fs.promises.writeFile(temporary, data);
    await fs.promises.rename(temporary, STATE_FILE);
  }).catch(() => addLog('Không lưu được cấu hình Chánh Điện.'));
}
let tiktok = null, tiktokState = { status: 'idle', uniqueId: '', error: '' };
const log = [];
function addLog(s){ const line = `[${new Date().toLocaleTimeString('vi-VN')}] ${s}`; log.push(line); if (log.length > 300) log.shift(); broadcast({ t:'log', line }, 'control'); console.log(line); }

const CAPS = [24, 36, 64, 100];
function capacity(){ if (settings.seatMode !== 'auto') return +settings.seatMode; const n = viewers.size; return CAPS.find(c => n <= c) || 100; }

/* skins: sprites that exist in /assets. Walking strips exist for man / woman / granny. */
// A comment changes the outfit only when it IS the keyword (≤ 3 words), so "Nam mô A Di Đà Phật" never reads as "nam".
const SKIN_WORDS = [
  ['master', ['cao tăng','cao tang']], ['monk', ['sư','su','sư thầy','su thay','thầy','thay']], ['nun', ['ni','ni cô','ni co','sư cô','su co']],
  ['child', ['tiểu','tieu','chú tiểu','chu tieu','bé','be']], ['granny', ['bà','ba','áo nâu','ao nau','cụ','cu']],
  ['woman', ['nữ','nu','chị','chi','gái','gai']], ['man', ['nam','anh','trai']],
];
function skinFromText(t){
  const w = String(t).toLowerCase().normalize('NFC').replace(/[^\p{L}\s]/gu, ' ').replace(/\s+/g, ' ').trim();
  if (!w || w.split(' ').length > 3) return null;
  for (const [skin, words] of SKIN_WORDS) if (words.some(k => w === k || w.startsWith(k + ' '))) return skin;
  return null;
}
const DEFAULT_SKINS = ['man', 'woman', 'woman', 'man', 'granny'];

function pub(v){ return { id:v.id, name:v.name, skin:v.skin, avatar:v.avatar||'', coins:v.coins, demo:!!v.demo }; }
function snapshot(){ return { t:'state', settings, viewers:[...viewers.values()].map(pub), cap: capacity(), startedAt, tiktok: tiktokState, pinned, capabilities:CAPABILITIES, bots:{on:!!botTimer}, simulationSkin }; }

function seat(id, name, opts = {}){
  if (!id) return {v:null, isNew:false};
  let v = viewers.get(id);
  if (v){ v.lastAt = Date.now(); if (opts.avatar) v.avatar = opts.avatar; return { v, isNew:false }; }
  if (viewers.size >= 100){                     // full: the longest-quiet non-donor stands up
    const quiet = [...viewers.values()].filter(x => x.coins === 0).sort((a,b) => a.lastAt - b.lastAt)[0];
    if (!quiet) return { v:null, isNew:false };
    viewers.delete(quiet.id); broadcast({ t:'leave', id: quiet.id });
  }
  v = { id, name: (name||id).slice(0, 18), skin: opts.skin || DEFAULT_SKINS[viewers.size % DEFAULT_SKINS.length],
        avatar: opts.avatar || '', coins: 0, joinedAt: Date.now(), lastAt: Date.now(), demo: !!opts.demo };
  viewers.set(id, v);
  broadcast({ t:'join', v: pub(v), cap: capacity() });
  return { v, isNew:true };
}

/* ------------------------------------------------------------- events in */
function onChat(id, name, text, avatar, demo = false){
  text = String(text || '').slice(0, 120);
  const want = skinFromText(text);
  const { v, isNew } = seat(id, name, { avatar, skin: want || (demo ? simulationSkin : '') || undefined, demo });
  if (!v) return;
  if (!isNew && want && want !== v.skin){ v.skin = want; broadcast({ t:'skin', id, skin: want }); }
  v.lastText = text; v.lastTextAt = Date.now();
  broadcast({ t:'chat', id, name: v.name, text });
  addLog(`💬 ${v.demo ? '[Mô phỏng] ' : ''}${v.name}: ${text}`);
}

function onGift(id, name, giftName, count, diamonds, avatar, demo = false, valueKnown = true){
  giftName = String(giftName || 'quà').slice(0, 30); count = Math.max(1, Math.min(9999, +count || 1));
  const { v } = seat(id, name, { avatar, demo, skin: demo && simulationSkin ? simulationSkin : undefined });
  if (!v) return;
  const coins = Math.max(0, (Number(diamonds) || 0) * (count || 1));
  v.coins += coins;
  const prayer = (v.lastText && Date.now() - v.lastTextAt < 90_000 && v.lastText.length > 6) ? v.lastText : pickPrayer();
  const tier = coins >= 99 ? 3 : coins >= 10 ? 2 : 1;
  const line = `Nam mô A Di Đà Phật. Phật tử ${spoken(v.name)} thành tâm dâng ${giftName}${count > 1 ? ', ' + count + ' lần' : ''}. ${prayer}.`;
  broadcast({ t:'gift', id, name: v.name, giftName, count, coins, total: v.coins, tier, prayer, valueKnown });
  if (settings.gifts.readAll || coins >= settings.gifts.minDiamonds) say(line, { tag:'gift', id });
}

function pickPrayer(){
  const P = ['Nguyện cầu gia đạo bình an, vạn sự cát tường', 'Nguyện cầu cha mẹ mạnh khỏe, sống lâu', 'Nguyện hồi hướng công đức cho khắp pháp giới chúng sinh',
             'Nguyện cầu thân tâm an lạc, phiền não tiêu trừ', 'Nguyện cầu công việc hanh thông, gia đình hạnh phúc'];
  return P[Math.floor(Math.random() * P.length)];
}
const spoken = s => String(s).replace(/[_@.]+/g, ' ').replace(/[^\p{L}\p{N}\s]/gu, '').trim() || 'ẩn danh';

/* ------------------------------------------------------------------ TTS */
const ttsQueue = []; let ttsBusy = false, ttsSeq = 0, ttsGeneration = 0, ttsAbort = null;
function say(text, meta = {}){ if (ttsQueue.length >= 15) { addLog('Hàng đợi giọng đọc đầy, bỏ qua 1 câu'); return; } ttsQueue.push({ text: String(text).slice(0, 500), meta, voice:{...settings} }); pumpTTS(); }
function stopSpeech(){ ttsGeneration++; ttsQueue.length = 0; ttsAbort?.abort(); broadcast({t:'stop'}); addLog('⏹ Đã dừng giọng và hàng đợi TTS.'); }
async function pumpTTS(){
  if (ttsBusy || !ttsQueue.length) return; ttsBusy = true;
  const { text, meta, voice } = ttsQueue.shift();
  const generation = ttsGeneration, abort = new AbortController(); ttsAbort = abort;
  try {
    const file = `tts_${Date.now()}_${++ttsSeq}.mp3`;
    const bytes = await synthesize(text, voice, {signal:abort.signal});
    if (generation !== ttsGeneration) return;
    await fs.promises.writeFile(path.join(TTS_DIR, file), bytes);
    if (generation !== ttsGeneration) { await fs.promises.unlink(path.join(TTS_DIR, file)).catch(()=>{}); return; }
    broadcast({ t:'tts', url: `/live/tts/${file}`, text, ...meta });
    addLog(`🔊 Đã tạo giọng Edge TTS (${voice.voice}).`);
    const old = (await fs.promises.readdir(TTS_DIR)).filter(f=>/^tts_\d+_\d+\.mp3$/.test(f)).sort();
    while (old.length > 40) await fs.promises.unlink(path.join(TTS_DIR, old.shift())).catch(()=>{});
  } catch (e) { if (e?.code !== 'TTS_CANCELLED') { addLog('TTS lỗi: ' + (e?.code || 'TTS_FAILED')); broadcast({t:'ttsStatus',status:'error',code:e?.code || 'TTS_FAILED'},'control'); } }
  finally { if (ttsAbort === abort) ttsAbort = null; ttsBusy = false; setTimeout(pumpTTS, 50); }
}

const CHANTS = [
  'Nam mô Bổn Sư Thích Ca Mâu Ni Phật. Nguyện đem công đức này, hướng về khắp tất cả, đệ tử và chúng sinh, đều trọn thành Phật đạo.',
  'Nam mô A Di Đà Phật. Nguyện sanh Tây Phương Tịnh Độ trung, cửu phẩm liên hoa vi phụ mẫu.',
  'Nam mô Đại Bi Quán Thế Âm Bồ Tát. Nguyện cho tất cả chúng sinh thân tâm an lạc, tai qua nạn khỏi.',
  'Quán Tự Tại Bồ Tát hành thâm Bát Nhã Ba La Mật Đa thời, chiếu kiến ngũ uẩn giai không, độ nhất thiết khổ ách.',
];
let chantIdx = 0, chantTimer = null;
function chant(){ const text = CHANTS[chantIdx++ % CHANTS.length]; broadcast({ t:'chant', text }); say(text, { tag:'chant' }); }
function scheduleChant(){ clearInterval(chantTimer); if (settings.autoChant) chantTimer = setInterval(chant, Math.max(1, settings.chantEveryMin) * 60_000); }
scheduleChant();

/* --------------------------------------------------------------- TikTok */
let TikTokLib = null;
let tiktokGeneration = 0;
async function connectTikTok(uniqueId){
  uniqueId = String(uniqueId || '').trim().replace(/^.*tiktok\.com\/@?/i, '').replace(/^@/, '').replace(/\/.*$/, '');
  if (!uniqueId) return;
  const generation = ++tiktokGeneration, previous = tiktok;
  tiktok = null;
  if (previous) { try { await previous.disconnect(); } catch {} }
  if (generation !== tiktokGeneration) return;
  tiktokState = { status: 'connecting', uniqueId, error: '' }; broadcast({ t:'tiktok', ...tiktokState });
  try {
    TikTokLib ||= await import('tiktok-live-connector');
    if (generation !== tiktokGeneration) return;
    const { TikTokLiveConnection, WebcastEvent, ControlEvent } = TikTokLib;
    // Read live events anonymously. The two optional HTTP metadata prefetches
    // require provider signing/premium access and are not needed by this scene.
    // Gift names/value still come from each event's giftDetails below.
    const conn = new TikTokLiveConnection(uniqueId, { enableExtendedGiftInfo: false, fetchRoomInfoOnConnect: false, processInitialData: false });
    tiktok = conn;
    const active = () => generation === tiktokGeneration && tiktok === conn;
    conn.on(WebcastEvent.CHAT, d => { if (active()) { const e=liveChat(d);onChat(e.id,e.name,e.text,e.avatar); } });
    conn.on(WebcastEvent.GIFT, d => {
      if (!active()) return;
      const e=liveGift(d);if(!e)return;
      onGift(e.id,e.name,e.giftName,e.count,e.diamonds,e.avatar,false,e.valueKnown);
      addLog(`🎁 ${e.name} tặng ${e.giftName} x${e.count}${e.valueKnown?'':' · chưa có mệnh giá trong sự kiện'}`);
    });
    conn.on(WebcastEvent.MEMBER, d => { if (active() && settings.joinOnEnter) {const e=liveUser(d.user||d);seat(e.id,e.name,{avatar:e.avatar});} });
    conn.on(WebcastEvent.LIKE, d => { if (active()) broadcast({ t:'like', id: liveUser(d.user||d).id, n: Number(d.likeCount) || 1 }); });
    conn.on(WebcastEvent.STREAM_END, e => { if(active())addLog(`TikTok gửi streamEnd (action ${e?.action??'?'}) cho @${uniqueId}.`); });
    conn.on(ControlEvent.DISCONNECTED, e => { if (!active()) return; tiktokState = { ...tiktokState, status: 'disconnected' }; broadcast({ t:'tiktok', ...tiktokState }); addLog(`TikTok: mất kết nối · code ${e?.code??'?'} ${String(e?.reason||'').slice(0,120)}`); });
    conn.on(ControlEvent.ERROR, e => { if (active()) addLog('TikTok lỗi: ' + (e?.info || e?.message || e)); });
    const st = await conn.connect();
    if (!active()) { try { await conn.disconnect(); } catch {} return; }
    tiktokState = { status: 'connected', uniqueId, error: '', roomId: st?.roomId || '' };
    addLog(`✅ Đã kết nối live @${uniqueId}`);
  } catch (e) {
    if (generation !== tiktokGeneration) return;
    const failed = tiktok; tiktok = null;
    try { await failed?.disconnect(); } catch {}
    tiktokState = { status: 'error', uniqueId, error: String(e?.message || e) };
    addLog(`❌ Không kết nối được @${uniqueId}: ${tiktokState.error}`);
  }
  broadcast({ t:'tiktok', ...tiktokState });
}
async function disconnectTikTok(){ tiktokGeneration++; const previous=tiktok; tiktok=null; tiktokState = { status:'idle', uniqueId:'', error:'' }; broadcast({ t:'tiktok', ...tiktokState }); if(previous) { try { await previous.disconnect(); } catch {} } }

/* ------------------------------------------------------------- test data */
const DEMO_NAMES = ['Diệu Hạnh','Minh Tâm','Thiện Đức','Huệ An','Tịnh Nhã','Quảng Trí','Chơn Như','Pháp Hỷ','Nguyên Thanh','Tâm Đăng','Diệu Liên','Trí Hải','Phước Lộc','Hạnh Nguyện','Như Ý','Bảo Châu','Thanh Lương','Từ Tâm','An Lạc','Giác Hoa','Tuệ Minh','Liên Hương','Thu Hà','Ngọc Lan','Đức Anh','Hải Đăng','Mai Anh','Phương Thảo','Trung Kiên','Quang Huy','Bảo Ngọc','Diệu Linh'];
function fillDemo(n){
  const have = [...viewers.values()].filter(v => v.demo).length;
  for (let i = have; i < n && viewers.size < 100; i++){
    const name = DEMO_NAMES[i % DEMO_NAMES.length] + (i >= DEMO_NAMES.length ? ' ' + (Math.floor(i / DEMO_NAMES.length) + 1) : '');
    seat('demo_' + i, name, { demo: true, skin: ['man','woman','woman','man','granny','monk','nun','child'][i % 8] });
  }
}
function clearSeats(){ viewers.clear(); broadcast(snapshot()); addLog('Đã làm trống bồ đoàn'); }

/* ------------------------------------------------------------- control */
function onControl(input){
  const aliases = {simChat:'chat',simGift:'gift',simLike:'like',demo:'fill'};
  const m = {...input, cmd:input.cmd || aliases[input.a] || input.a};
  if (!m.uniqueId) m.uniqueId = m.uid;
  if (m.diamonds == null) m.diamonds = m.dia;
  switch (m.cmd){
    case 'connect': connectTikTok(m.uniqueId); break;
    case 'disconnect': disconnectTikTok(); addLog('Đã ngắt kết nối TikTok'); break;
    case 'chat': { const n = String(m.name || 'Khách').slice(0, 18); onChat('sim_' + n, n, String(m.text || 'Nam mô A Di Đà Phật').slice(0, 120), undefined, true); break; }
    case 'gift': { const n = String(m.name || 'Khách').slice(0, 18); onGift('sim_' + n, n, String(m.gift || 'Hoa hồng').slice(0, 30), clamp(m.count ?? 1, 1, 99), clamp(m.diamonds ?? 1, 1, 50000), undefined, true); addLog(`🎁 [Mô phỏng] ${n} tặng ${String(m.gift || 'Hoa hồng').slice(0,30)}`); break; }
    case 'like': { const name = String(m.name || 'Khách').slice(0,18); const {v}=seat('sim_'+name,name,{demo:true,skin:simulationSkin||undefined}); if(v) broadcast({t:'like',id:v.id,n:clamp(m.n??1,1,100)}); break; }
    case 'fill': fillDemo(clamp(m.n, 1, 100)); addLog(`Đổ người mẫu: ${viewers.size} người`); break;
    case 'clear': clearSeats(); break;
    case 'bell': broadcast({ t:'bell' }); break;
    case 'mo': broadcast({ t:'mo1' }); break;
    case 'chant': chant(); break;
    case 'say': if (m.text) say(String(m.text).slice(0, 400), { tag:'say' }); break;
    case 'stop': stopSpeech(); break;
    case 'pin': pinned = m.title ? {title:String(m.title).slice(0,160),price:String(m.price||'').slice(0,60)} : null; broadcast({t:'pin',pinned}); persist(); addLog(pinned ? `📌 Đã ghim: ${pinned.title}` : 'Đã bỏ ghim sản phẩm.'); break;
    case 'skin':
      if (!['monk','child','nun','granny','man','woman'].includes(m.skin)) throw new Error('invalid skin');
      simulationSkin = m.skin;
      for (const v of viewers.values()) if (v.demo) { v.skin=m.skin; broadcast({t:'skin',id:v.id,skin:m.skin}); }
      persist(); addLog(`Đã đổi hình tướng mô phỏng: ${m.skin}`); break;
    case 'settings':
      applySettings(m.settings || {});
      scheduleChant(); persist(); broadcast({ t:'settings', settings, cap: capacity() }); break;
    case 'bots': setBots(!!m.on, +m.n || 12, m.pace); break;
    default: throw new Error('unsupported command');
  }
}

// Only known keys, right types, sane ranges: the panel is local, but nothing from the socket is trusted blindly.
const SETTING_RULES = {
  title: v => String(v).slice(0, 40), subtitle: v => String(v).slice(0, 60),
  voice: v => ['vi-VN-NamMinhNeural', 'vi-VN-HoaiMyNeural'].includes(v) ? v : settings.voice,
  pitch: v => clamp(v, -50, 50), rate: v => clamp(v, -50, 50), moBpm: v => clamp(v, 20, 120), chantEveryMin: v => clamp(v, 1, 60),
  music: Boolean, mo: Boolean, autoChant: Boolean, joinOnEnter: Boolean,
  gifts: v => v && typeof v === 'object' ? {readAll:typeof v.readAll==='boolean'?v.readAll:settings.gifts.readAll,minDiamonds:clamp(v.minDiamonds??settings.gifts.minDiamonds,1,50000)} : settings.gifts,
  seatMode: v => ['auto', '24', '36', '64', '100'].includes(String(v)) ? String(v) : 'auto',
};
const clamp = (v, a, b) => Math.max(a, Math.min(b, Math.round(+v || 0)));
function applySettings(o){ for (const [k, fn] of Object.entries(SETTING_RULES)) if (k in o) settings[k] = fn(o[k]); }

/* ----------------------------------------------------------------- bots */
let botTimer = null;
const BOT_LINES = ['Nam mô A Di Đà Phật', 'Nam mô Bổn Sư Thích Ca Mâu Ni Phật', 'Cầu cho gia đình bình an', 'Con xin thành tâm sám hối', 'nữ', 'nam', 'bà',
  'Nguyện cầu cha mẹ mạnh khỏe', 'Mô Phật, an lạc quá', 'Chúc mọi người một ngày bình an', 'sư', 'Cầu công việc hanh thông'];
function setBots(on, n, pace){
  clearInterval(botTimer); botTimer = null;
  if (on){
    n = clamp(n, 1, 100);
    for (let i = 0; i < n; i++) seat('bot_' + i, DEMO_NAMES[(i + 7) % DEMO_NAMES.length] + ' ·bot', { demo: true });
    const every = { slow: 4000, mid: 1800, fast: 700 }[pace] || 1800;
    botTimer = setInterval(() => {
      const ids = [...viewers.keys()].filter(id => id.startsWith('bot_')); if (!ids.length) return;
      const id = ids[Math.floor(Math.random() * ids.length)], v = viewers.get(id), r = Math.random();
      if (r < .62) onChat(id, v.name, BOT_LINES[Math.floor(Math.random() * BOT_LINES.length)]);
      else if (r < .9) broadcast({ t:'like', id, n: 1 + Math.floor(Math.random() * 4) });
      else if (r < .985) onGift(id, v.name, 'Hoa hồng', 1 + Math.floor(Math.random() * 3), 1);
      else onGift(id, v.name, 'Đèn hoa sen', 1, 99);
    }, every);
    addLog(`🤖 Bật ${n} bot, nhịp ${pace}`);
  } else {
    for (const id of [...viewers.keys()]) if (id.startsWith('bot_')) { viewers.delete(id); broadcast({ t:'leave', id }); }
    addLog('🤖 Tắt bot');
  }
  broadcast({ t:'bots', on }, 'control');
}

/* -------------------------------------------------------------- server */
// Security notes (this file is meant to be shared publicly):
//  • listens on 127.0.0.1 only; nothing is reachable from the LAN or internet unless HOST is changed on purpose
//  • Host header must be localhost/127.0.0.1 (blocks DNS-rebinding pages from talking to the server)
//  • WebSocket only accepts pages served by this server (Origin check), so other sites open in the
//    same browser cannot send control commands
//  • static files: only /assets and /live, no dot-files, no node_modules, no server source, no path escapes
//  • no TikTok password, cookie or session is ever used or stored; only public room events are read
const HOST = process.env.HOST || '127.0.0.1';
const MIME = { '.html':'text/html; charset=utf-8', '.js':'text/javascript', '.css':'text/css', '.json':'application/json',
  '.png':'image/png', '.jpg':'image/jpeg', '.webp':'image/webp', '.mp3':'audio/mpeg', '.svg':'image/svg+xml', '.ico':'image/x-icon' };
const ALLOWED_HOSTS = new Set([`localhost:${PORT}`, `127.0.0.1:${PORT}`, `[::1]:${PORT}`]);
const okHost = h => ALLOWED_HOSTS.has(String(h || '').toLowerCase());
const SEP = path.sep;
function resolveStatic(urlPath){
  let p = urlPath === '/' ? '/live/control.html' : urlPath;
  if (!/^\/(assets|live)\//.test(p)) return null;
  if (p.split('/').some(seg => seg.startsWith('.') || seg === 'node_modules')) return null;
  if (/\.(mjs|json|md|log)$/i.test(p) && !p.startsWith('/assets/')) return null;      // never serve server code / package files
  const file = path.normalize(path.join(ROOT, p));
  if (p.startsWith('/live/tts/')) { const name=p.slice('/live/tts/'.length); return /^tts_\d+_\d+\.mp3$/.test(name) ? path.join(TTS_DIR,name) : null; }
  if (!file.startsWith(ROOT + SEP)) return null;
  return file;
}
const server = http.createServer((req, res) => {
  if (!okHost(req.headers.host)) { res.writeHead(421).end('wrong host'); return; }
  let p; try { p = decodeURIComponent(new URL(req.url, 'http://x').pathname); } catch { res.writeHead(400).end(); return; }
  if (p === '/favicon.ico') { res.writeHead(204).end(); return; }
  if (p === '/status' || p === '/api/status') {
    res.writeHead(200, { 'Content-Type': 'application/json; charset=utf-8', 'Access-Control-Allow-Origin': '*' });
    res.end(JSON.stringify({ running: true, runtime:'concat-temple-live', protocol:1, capabilities:CAPABILITIES, port: PORT, viewers: viewers.size, capacity: capacity(), tiktok: tiktokState, settings, pinned, bots:{on:!!botTimer}, tts:{busy:ttsBusy,queued:ttsQueue.length} }));
    return;
  }
  if (p === '/api/control') {
    if (!['GET','POST'].includes(req.method)) { res.writeHead(405).end(); return; }
    let originAllowed=true;
    try { if(req.headers.origin) originAllowed=okHost(new URL(req.headers.origin).host); } catch { originAllowed=false; }
    if (!originAllowed || req.headers['sec-fetch-site']==='cross-site') { res.writeHead(403).end(); return; }
    let body = '';
    let tooLarge=false;
    req.on('data', chunk => { body += chunk; if(Buffer.byteLength(body)>64*1024){tooLarge=true;res.writeHead(413).end();req.destroy();} });
    req.on('end', () => {
      if(tooLarge) return;
      try {
        let m = {};
        if (body) {
          m = JSON.parse(body);
        } else {
          m = Object.fromEntries(new URL(req.url, 'http://x').searchParams.entries());
        }
        onControl(m);
        res.writeHead(200, { 'Content-Type': 'application/json; charset=utf-8', 'Access-Control-Allow-Origin': '*' });
        res.end(JSON.stringify({ ok: true, viewers: viewers.size, settings }));
      } catch (e) {
        res.writeHead(400).end('invalid control request');
      }
    });
    return;
  }
  if (req.method !== 'GET' && req.method !== 'HEAD') { res.writeHead(405).end(); return; }
  const file = resolveStatic(p);
  if (!file) { res.writeHead(404).end('not found'); return; }
  fs.stat(file, (err, st) => {
    if (err || !st.isFile()) { res.writeHead(404).end('not found'); return; }
    res.writeHead(200, { 'Content-Type': MIME[path.extname(file).toLowerCase()] || 'application/octet-stream',
      'Cache-Control': p.startsWith('/live/tts/') ? 'no-store' : 'no-cache', 'X-Content-Type-Options': 'nosniff', 'Referrer-Policy': 'no-referrer' });
    if (req.method === 'HEAD') { res.end(); return; }
    fs.createReadStream(file).pipe(res);
  });
});
const wss = new WebSocketServer({ server, path: '/ws', maxPayload: 64 * 1024,
  verifyClient: ({ origin, req }) => {
    if (!okHost(req.headers.host)) return false;
    if (!origin) return true;                                   // OBS / non-browser clients send no Origin
    try { return okHost(new URL(origin).host); } catch { return false; }
  } });
function broadcast(msg, only){ const s = JSON.stringify(msg); for (const c of wss.clients) if (c.readyState === 1 && (!only || c.role === only)) c.send(s); }
wss.on('connection', (ws, req) => {
  ws.role = new URL(req.url, 'http://x').searchParams.get('role') === 'control' ? 'control' : 'overlay';
  ws.send(JSON.stringify(snapshot()));
  if (ws.role === 'control') ws.send(JSON.stringify({ t:'logs', lines: log }));
  ws.on('message', buf => {
    if (ws.role !== 'control') return;                          // overlays only listen
    try { const m = JSON.parse(buf); if (m && m.t === 'ctl') onControl(m); } catch (e) { addLog('Lệnh lỗi: ' + e.message); }
  });
});
applySettings(initial.settings || {});
if (initial.pinned?.title) pinned={title:String(initial.pinned.title).slice(0,160),price:String(initial.pinned.price||'').slice(0,60)};
if (['monk','child','nun','granny','man','woman'].includes(initial.simulationSkin)) simulationSkin=initial.simulationSkin;
scheduleChant();
server.listen(PORT, HOST, () => { persist(); addLog(`Server chạy tại http://localhost:${PORT}  ·  Bảng điều khiển: http://localhost:${PORT}/live/studio.html  ·  Overlay: http://localhost:${PORT}/live/overlay.html`); });
