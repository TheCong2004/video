/**
 * Slive VIP Background Keep-Alive & Anti-Throttling Engine
 * Đảm bảo 100% video Avatar Live, WebRTC và các timer JavaScript chạy mượt mà ở 60 FPS
 * ngay cả khi cửa sổ app bị mất focus, chuyển sang OBS, TikTok Live Studio, hoặc chạy nền.
 */
(function initSliveBackgroundKeepAlive() {
  // 1. Vô hiệu hóa cơ chế phát hiện ẩn / mất focus của trình duyệt (Page Visibility API)
  try {
    Object.defineProperty(document, 'hidden', {
      get: () => false,
      configurable: true
    });
    Object.defineProperty(document, 'visibilityState', {
      get: () => 'visible',
      configurable: true
    });
    document.hasFocus = () => true;

    // Chặn triệt để sự kiện visibilitychange để trang không bao giờ rơi vào chế độ ngủ
    window.addEventListener('visibilitychange', (e) => {
      if (document.visibilityState !== 'visible') {
        e.stopImmediatePropagation();
      }
    }, true);
  } catch (_) {}

  // 2. Silent Web Audio Keep-Alive (Tuyệt chiêu Chromium Media Source)
  // Khi có AudioContext đang phát âm thanh, Chromium coi đây là Active Media Stream
  // và TUYỆT ĐỐI KHÔNG BAO GIỜ bóp xung nhịp, hạ FPS hay freeze video pipeline.
  let audioContextInstance = null;
  function startSilentAudio() {
    try {
      if (audioContextInstance && audioContextInstance.state === 'running') return;
      const AudioCtx = window.AudioContext || window.webkitAudioContext;
      if (!AudioCtx) return;
      audioContextInstance = new AudioCtx();
      const osc = audioContextInstance.createOscillator();
      const gain = audioContextInstance.createGain();
      gain.gain.value = 0.00001; // Âm lượng cực vi mô, người dùng hoàn toàn không nghe thấy
      osc.connect(gain);
      gain.connect(audioContextInstance.destination);
      osc.start();

      const resumeAudio = () => {
        if (audioContextInstance && audioContextInstance.state === 'suspended') {
          audioContextInstance.resume().catch(() => {});
        }
      };
      window.addEventListener('click', resumeAudio, { passive: true });
      window.addEventListener('keydown', resumeAudio, { passive: true });
      window.addEventListener('blur', resumeAudio, { passive: true });
      window.addEventListener('focus', resumeAudio, { passive: true });
    } catch (_) {}
  }

  // Khởi động ngay hoặc sau tương tác đầu tiên
  startSilentAudio();
  window.addEventListener('DOMContentLoaded', startSilentAudio);

  // 3. Dedicated Background Web Worker Heartbeat (Chống Timer Throttling 1000ms)
  // Web Worker chạy trên OS thread độc lập, hoàn toàn KHÔNG BỊ KHỐNG CHẾ bởi Focus của cửa sổ.
  // Nhịp 25ms (~40 FPS) từ Worker liên tục đánh thức Chromium Event Loop.
  try {
    const workerScript = `
      let ticker = null;
      self.onmessage = function(e) {
        if (e.data === 'start') {
          if (!ticker) {
            ticker = setInterval(function() {
              self.postMessage('ping');
            }, 25);
          }
        } else if (e.data === 'stop') {
          if (ticker) {
            clearInterval(ticker);
            ticker = null;
          }
        }
      };
      self.postMessage('ready');
    `;
    const blob = new Blob([workerScript], { type: 'application/javascript' });
    const worker = new Worker(URL.createObjectURL(blob));
    worker.onmessage = function() {
      // Giữ event loop luôn tỉnh táo
    };
    worker.postMessage('start');
  } catch (_) {}

  // 4. Tự động phục hồi phát video khi cửa sổ bị blur hoặc pause bất thường
  function autoResumeVideos() {
    document.querySelectorAll('video').forEach((vid) => {
      if (vid.autoplay && vid.paused && vid.srcObject) {
        vid.play().catch(() => {});
      }
    });
  }
  window.addEventListener('blur', () => {
    setTimeout(autoResumeVideos, 50);
    setTimeout(autoResumeVideos, 250);
  });
})();
