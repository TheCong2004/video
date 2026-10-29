// ─── Chống lag / drop FPS khi cửa sổ app mất focus, bị che khuất hoặc chạy nền ─
try {
  Object.defineProperty(document, 'hidden', { get: () => false, configurable: true });
  Object.defineProperty(document, 'visibilityState', { get: () => 'visible', configurable: true });
  window.addEventListener('visibilitychange', (e) => {
    if (document.visibilityState !== 'visible') {
      e.stopImmediatePropagation();
    }
  }, true);
} catch (_) {}

let pc = null;
let mediaReady = false;

function attachRemoteTrack(event) {
  const element = document.getElementById(event.track.kind === 'video' ? 'video' : 'audio');
  if (!element) return;

  // aiortc may emit a track without event.streams[0]. Build an explicit
  // one-track MediaStream so video and audio are never assigned `undefined`
  // or duplicated through the same mixed stream.
  element.srcObject = new MediaStream([event.track]);
  element.autoplay = true;
  element.playsInline = true;
  if (event.track.kind === 'video') {
    element.muted = true;
    element.addEventListener('pause', () => {
      if (mediaReady && element.srcObject) {
        element.play().catch(() => {});
      }
    });
  }

  const play = () => element.play().catch((error) => {
    console.warn(`Không thể tự phát ${event.track.kind}:`, error);
    if (event.track.kind === 'audio' && typeof window.onWebRTCAudioBlocked === 'function') {
      window.onWebRTCAudioBlocked(error);
    }
  });

  if (event.track.kind === 'video') {
    const markReady = () => {
      play();
      if (mediaReady) return;
      mediaReady = true;
      if (typeof window.onWebRTCConnected === 'function') window.onWebRTCConnected();
    };
    if (element.readyState >= 2) markReady();
    else element.addEventListener('loadeddata', markReady, { once: true });
  } else {
    if (element.readyState >= 2) play();
    else element.addEventListener('canplay', play, { once: true });
  }
}

async function negotiate() {
  pc.addTransceiver('video', { direction: 'recvonly' });
  pc.addTransceiver('audio', { direction: 'recvonly' });
  const offer = await pc.createOffer();
  await pc.setLocalDescription(offer);
  if (pc.iceGatheringState !== 'complete') {
    await new Promise((resolve) => {
      const check = () => {
        if (pc.iceGatheringState === 'complete') {
          pc.removeEventListener('icegatheringstatechange', check);
          resolve();
        }
      };
      pc.addEventListener('icegatheringstatechange', check);
    });
  }
  const avatarSelect = document.getElementById('select-avatar');
  const response = await fetch('/offer', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      sdp: pc.localDescription.sdp,
      type: pc.localDescription.type,
      avatar: avatarSelect ? avatarSelect.value : ''
    })
  });
  if (!response.ok) throw new Error((await response.json().catch(() => ({}))).error || 'Không thể kết nối WebRTC.');
  const answer = await response.json();
  document.getElementById('sessionid').value = answer.sessionid;
  if (!pc || pc.signalingState !== 'have-local-offer') return;
  await pc.setRemoteDescription(answer);
}

async function start() {
  if (pc) {
    try { pc.close(); } catch (e) {}
    pc = null;
  }
  mediaReady = false;
  const config = { sdpSemantics: 'unified-plan' };
  const stun = document.getElementById('use-stun');
  if (stun && stun.checked) config.iceServers = [{ urls: ['stun:stun.l.google.com:19302'] }];
  pc = new RTCPeerConnection(config);
  pc.addEventListener('track', attachRemoteTrack);
  pc.addEventListener('connectionstatechange', () => {
    if (pc && ['failed', 'closed', 'disconnected'].includes(pc.connectionState) && typeof window.onWebRTCDisconnected === 'function') {
      window.onWebRTCDisconnected();
    }
  });
  try {
    await negotiate();
  } catch (error) {
    if (pc) {
      pc.close();
      pc = null;
    }
    if (typeof window.onWebRTCDisconnected === 'function') window.onWebRTCDisconnected();
    throw error;
  }
}

function stop() {
  if (pc) {
    pc.close();
    pc = null;
  }
  mediaReady = false;
  ['video', 'audio'].forEach((id) => {
    const element = document.getElementById(id);
    if (element) element.srcObject = null;
  });
  if (typeof window.onWebRTCDisconnected === 'function') window.onWebRTCDisconnected();
}

window.addEventListener('unload', () => { if (pc) pc.close(); });
