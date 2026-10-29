(() => {
  'use strict';

  const byId = (id) => document.getElementById(id);
  const storedNumber = (key, fallback, min, max) => {
    const raw = localStorage.getItem(key);
    const value = raw === null ? fallback : Number(raw);
    return Math.max(min, Math.min(max, Number.isFinite(value) ? value : fallback));
  };

  const SERVICE_LABEL = 'Live VIP';
  const SERVICE_TYPE = 'live_face';
  const PRESENCE_HEARTBEAT_MS = 10000;
  const presenceClientId = (() => {
    const randomPart = window.crypto?.randomUUID?.()
      || `${Date.now()}-${Math.random().toString(36).slice(2)}`;
    return `live_face-${randomPart}`;
  })();

  const state = {
    profile: null,
    elapsed: 0,
    remaining: 0,
    rate: 20000,
    startedAt: null,
    running: false,
    avatarConnected: false,
    billingTimer: null,
    localTimer: null,
    profilePollInterval: null,
    presenceHeartbeatInterval: null,
    presenceHeartbeatInFlight: false,
    invoicePollInterval: null,
    invoiceCheckInFlight: false,
    forceLogoutTimer: null,
    forceLogoutActive: false,
    forceLogoutRemainingSeconds: 10,
    forceLogoutFinishing: false,
    chatPoll: null,
    eventPoll: null,
    speechPoll: null,
    speechFrame: null,
    activeSpeechElement: null,
    activeSpeechSequence: 0,
    completedSpeechSequence: 0,
    avatarTaskPoll: null,
    activeInvoiceId: null,
    currentSelectedAmount: 100000,
    loadedInvoices: [],
    speechVoice: localStorage.getItem('slive-vip-voice') || 'Ngọc Linh',
    speechVolume: storedNumber('slive-vip-volume', .8, 0, 1),
    speechSpeed: storedNumber('slive-vip-speed', 1, .8, 1.4),
    heartbeatFailures: 0,
    resourcesKnown: false,
    resourcesInstalled: true,
    resourceStatus: null,
    resourcePoll: null,
    tiktokConnected: false,
    currentAvatar: localStorage.getItem('slive-vip-selected-avatar') || 'mau10',
    activePromptTopic: '',
    activeOptimizedPrompt: '',
  };

  const avatarChannel = typeof BroadcastChannel !== 'undefined'
    ? new BroadcastChannel('slive-avatar-live') : null;
  let avatarConnectResolve = null;
  let avatarConnectReject = null;

  function waitForDesktopApi(timeoutMs = 5000) {
    if (window.pywebview?.api?.open_avatar_window) return Promise.resolve(true);
    return new Promise((resolve) => {
      let finished = false;
      const startedAt = Date.now();
      const finish = (ready) => {
        if (finished) return;
        finished = true;
        clearInterval(timer);
        window.removeEventListener('pywebviewready', check);
        resolve(ready);
      };
      const check = () => {
        if (window.pywebview?.api?.open_avatar_window) finish(true);
        else if (Date.now() - startedAt >= timeoutMs) finish(false);
      };
      const timer = setInterval(check, 100);
      window.addEventListener('pywebviewready', check);
      check();
    });
  }

  async function ensureAvatarWindow() {
    const desktopReady = await waitForDesktopApi();
    if (!desktopReady) {
      console.warn('Avatar Live chỉ mở thành cửa sổ riêng khi chạy bằng ứng dụng desktop.');
      return false;
    }
    await window.pywebview.api.open_avatar_window();
    return true;
  }

  function sendAvatarCommand(type, extra = {}) {
    avatarChannel?.postMessage({ type, avatar: byId('select-avatar')?.value || '', ...extra });
  }

  function sendAvatarSettings() {
    sendAvatarCommand('settings', { volume: state.speechVolume });
  }

  async function syncSpeechSettings() {
    sendAvatarSettings();
    const sessionId = byId('sessionid')?.value;
    if (!sessionId || sessionId === '0') return;
    await api('/speech_settings', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        sessionid: String(sessionId),
        voice: state.speechVoice,
        speed: state.speechSpeed,
      }),
    });
  }

  function setupSpeechControls() {
    const voice = byId('tts-voice-select');
    const volume = byId('tts-volume');
    const speed = byId('tts-speed');
    if (!voice || !volume || !speed) return;
    voice.value = state.speechVoice;
    volume.value = String(state.speechVolume);
    speed.value = String(state.speechSpeed);
    setText('tts-volume-value', `${Math.round(state.speechVolume * 100)}%`);
    setText('tts-speed-value', `${state.speechSpeed.toFixed(2)}x`);

    loadVoiceCatalog(state.speechVoice).catch(() => { });

    voice.addEventListener('change', () => {
      state.speechVoice = voice.value || 'Ngọc Linh';
      localStorage.setItem('slive-vip-voice', state.speechVoice);
      syncSpeechSettings().catch(() => { });
    });
    volume.addEventListener('input', () => {
      state.speechVolume = Math.max(0, Math.min(1, Number(volume.value) || 0));
      localStorage.setItem('slive-vip-volume', String(state.speechVolume));
      setText('tts-volume-value', `${Math.round(state.speechVolume * 100)}%`);
      sendAvatarSettings();
    });
    speed.addEventListener('input', () => {
      state.speechSpeed = Math.max(.8, Math.min(1.6, Number(speed.value) || 1));
      localStorage.setItem('slive-vip-speed', String(state.speechSpeed));
      setText('tts-speed-value', `${state.speechSpeed.toFixed(2)}x`);
      syncSpeechSettings().catch(() => { });
    });
    speed.addEventListener('change', () => syncSpeechSettings().catch(() => { }));
  }

  /* ==========================================================================
     VOICE CLONING & CUSTOM VOICE CONTROLLER (STUDIO TTS)
     ========================================================================== */
  let voiceCatalog = {
    presets: [],
    custom: new Map()
  };
  let vcSourceMode = 'file';
  let vcRecordedWavBlob = null;
  let vcMediaStream = null;
  let vcAudioCtx = null;
  let vcScriptNode = null;
  let vcMicSource = null;
  let vcRecordedSamples = [];
  let vcRecordingTimer = null;
  let vcRecordStartTime = 0;
  let vcIsRecording = false;
  let vcCurrentTestAudio = null;
  let vcConvertedFileBlob = null;
  let vcIsConvertingFile = false;

  async function loadVoiceCatalog(selectVoiceName) {
    const select = byId('tts-voice-select');
    if (!select) return;
    try {
      const response = await fetch('/api/voices', { cache: 'no-store' });
      if (!response.ok) return;
      const data = await response.json();
      const presets = Array.isArray(data.presets) ? data.presets : [];
      const custom = Array.isArray(data.custom) ? data.custom : [];

      voiceCatalog = {
        presets: presets,
        custom: new Map(custom.map(item => [item.name, item]))
      };

      const previousVal = selectVoiceName || select.value || state.speechVoice || 'Ngọc Linh';
      select.innerHTML = '';

      // 1. Group custom cloned voices at top if any exist
      if (custom.length) {
        const groupCustom = document.createElement('optgroup');
        groupCustom.label = '⭐ Giọng cá nhân (Đã clone)';
        custom.forEach(item => {
          const option = document.createElement('option');
          option.value = item.name;
          const desc = item.description ? ` - ${item.description}` : '';
          const gender = item.gender ? ` [${item.gender}]` : '';
          option.textContent = `⭐ ${item.name}${gender}${desc}`;
          groupCustom.appendChild(option);
        });
        select.appendChild(groupCustom);
      }

      // 2. Group presets by Female and Male
      const femaleVoices = presets.filter(p => p.gender === 'Nữ');
      const maleVoices = presets.filter(p => p.gender === 'Nam');
      const otherVoices = presets.filter(p => p.gender !== 'Nữ' && p.gender !== 'Nam');

      if (femaleVoices.length) {
        const groupFemale = document.createElement('optgroup');
        groupFemale.label = 'Giọng Nữ';
        femaleVoices.forEach(item => {
          const option = document.createElement('option');
          option.value = item.name || item.id;
          const desc = item.description ? ` (${item.description})` : '';
          option.textContent = `${item.name}${desc}`;
          groupFemale.appendChild(option);
        });
        select.appendChild(groupFemale);
      }

      if (maleVoices.length) {
        const groupMale = document.createElement('optgroup');
        groupMale.label = 'Giọng Nam';
        maleVoices.forEach(item => {
          const option = document.createElement('option');
          option.value = item.name || item.id;
          const desc = item.description ? ` (${item.description})` : '';
          option.textContent = `${item.name}${desc}`;
          groupMale.appendChild(option);
        });
        select.appendChild(groupMale);
      }

      if (otherVoices.length) {
        const groupOther = document.createElement('optgroup');
        groupOther.label = 'Giọng khác';
        otherVoices.forEach(item => {
          const option = document.createElement('option');
          option.value = item.name || item.id;
          option.textContent = item.name;
          groupOther.appendChild(option);
        });
        select.appendChild(groupOther);
      }

      const available = [...select.options].map(option => option.value);
      if (available.includes(previousVal)) {
        select.value = previousVal;
      } else if (available.length > 0) {
        select.value = data.default_voice && available.includes(data.default_voice) ? data.default_voice : available[0];
      }
      state.speechVoice = select.value || 'Trúc Ly';

      updateVoiceCountBadge();
      renderCustomVoiceList();
    } catch (error) {
      console.warn('[Voices] Không tải được danh sách giọng:', error);
    }
  }

  function updateVoiceCountBadge() {
    const badge = byId('vc-count-badge');
    if (badge && voiceCatalog && voiceCatalog.custom) {
      badge.textContent = voiceCatalog.custom.size || 0;
    }
  }

  function openVoiceModal() {
    const modal = byId('voice-modal');
    if (!modal) return;
    modal.style.display = 'flex';
    modal.classList.add('open');
    updateVoiceCountBadge();
    renderCustomVoiceList();
    resetVoiceCloneForm();
  }

  function closeVoiceModal() {
    const modal = byId('voice-modal');
    if (modal) {
      modal.style.display = 'none';
      modal.classList.remove('open');
    }
    if (vcIsRecording) {
      stopVoiceRecording(true);
    }
    if (vcCurrentTestAudio) {
      try { vcCurrentTestAudio.pause(); } catch (e) { }
      vcCurrentTestAudio = null;
    }
    const filePreview = byId('vc-file-preview');
    if (filePreview) filePreview.pause();
    const micPreview = byId('vc-mic-preview');
    if (micPreview) micPreview.pause();
  }

  function switchVoiceModalTab(tab) {
    const createBtn = byId('tab-vc-create-btn');
    const listBtn = byId('tab-vc-list-btn');
    const createContent = byId('vc-tab-create-content');
    const listContent = byId('vc-tab-list-content');

    if (tab === 'create') {
      if (createBtn) createBtn.classList.add('active');
      if (listBtn) listBtn.classList.remove('active');
      if (createContent) createContent.style.display = 'block';
      if (listContent) listContent.style.display = 'none';
    } else {
      if (listBtn) listBtn.classList.add('active');
      if (createBtn) createBtn.classList.remove('active');
      if (listContent) listContent.style.display = 'block';
      if (createContent) createContent.style.display = 'none';
      renderCustomVoiceList();
    }
  }

  function setVoiceSourceMode(mode) {
    vcSourceMode = mode;
    const cardFile = byId('vc-source-card-file');
    const cardMic = byId('vc-source-card-mic');
    const panelFile = byId('vc-panel-file');
    const panelMic = byId('vc-panel-mic');

    if (mode === 'file') {
      if (cardFile) cardFile.classList.add('active');
      if (cardMic) cardMic.classList.remove('active');
      if (panelFile) panelFile.style.display = 'block';
      if (panelMic) panelMic.style.display = 'none';
    } else {
      if (cardMic) cardMic.classList.add('active');
      if (cardFile) cardFile.classList.remove('active');
      if (panelMic) panelMic.style.display = 'block';
      if (panelFile) panelFile.style.display = 'none';
    }
  }

  async function handleVoiceFileSelected(input) {
    const file = input && input.files && input.files[0];
    const label = byId('vc-file-label');
    const audioBox = byId('vc-file-audio-box');
    const preview = byId('vc-file-preview');
    const statusMsg = byId('vc-status-msg');

    vcConvertedFileBlob = null;

    if (file) {
      vcIsConvertingFile = true;
      const sizeMb = (file.size / (1024 * 1024)).toFixed(2);
      if (label) label.innerHTML = `Đang tải & chuẩn hóa: <strong style="color:var(--primary);">${file.name}</strong> (${sizeMb} MB)...`;

      try {
        const arrayBuffer = await file.arrayBuffer();
        const AudioCtxClass = window.AudioContext || window.webkitAudioContext;
        const ctx = new AudioCtxClass();
        try {
          const audioBuffer = await ctx.decodeAudioData(arrayBuffer);
          const duration = audioBuffer.duration;
          if (duration < 1.0) {
            showVoiceModalStatus('Mẫu giọng quá ngắn (phải từ 1.0 giây trở lên). Vui lòng chọn file khác.', 'error');
            if (label) label.innerHTML = '<span style="color:var(--danger);">File quá ngắn (&lt; 1.0s)</span>';
            return;
          }
          if (duration > 30.0) {
            showVoiceModalStatus('Mẫu giọng quá dài (' + duration.toFixed(1) + 's). Giới hạn tối đa là 30 giây.', 'error');
            if (label) label.innerHTML = '<span style="color:var(--danger);">File quá dài (&gt; 30s)</span>';
            return;
          }
          const numChannels = audioBuffer.numberOfChannels;
          const len = audioBuffer.length;
          const monoSamples = new Float32Array(len);
          if (numChannels === 1) {
            monoSamples.set(audioBuffer.getChannelData(0));
          } else {
            const ch0 = audioBuffer.getChannelData(0);
            const ch1 = audioBuffer.getChannelData(1);
            for (let i = 0; i < len; i++) {
              monoSamples[i] = (ch0[i] + ch1[i]) * 0.5;
            }
          }
          vcConvertedFileBlob = encodeWAV(monoSamples, audioBuffer.sampleRate || 44100);
          if (label) label.innerHTML = `Đã chọn: <strong style="color:var(--primary);">${file.name}</strong> (${duration.toFixed(1)}s - Chuẩn WAV)`;
          if (audioBox) audioBox.style.display = 'flex';
          if (preview) {
            preview.src = URL.createObjectURL(vcConvertedFileBlob);
            preview.load();
          }
          if (statusMsg) statusMsg.style.display = 'none';
        } finally {
          try { ctx.close(); } catch (e) { }
        }
      } catch (decodeErr) {
        console.warn('[VoiceFile] Giải mã Web Audio thất bại, fallback file gốc:', decodeErr);
        vcConvertedFileBlob = file;
        if (label) label.innerHTML = `Đã chọn: <strong style="color:var(--primary);">${file.name}</strong> (${sizeMb} MB)`;
        if (audioBox) audioBox.style.display = 'flex';
        if (preview) {
          preview.src = URL.createObjectURL(file);
          preview.load();
        }
      } finally {
        vcIsConvertingFile = false;
      }
    } else {
      vcIsConvertingFile = false;
      if (label) label.innerHTML = 'Nhấp để chọn file <strong style="color: var(--primary);">.WAV, .MP3, .M4A</strong> từ máy tính';
      if (audioBox) audioBox.style.display = 'none';
      if (preview) preview.src = '';
    }
  }

  function resetVoiceCloneForm() {
    const nameInput = byId('vc-name');
    if (nameInput) nameInput.value = '';
    const descInput = byId('vc-desc');
    if (descInput) descInput.value = '';
    const fileInput = byId('vc-file-input');
    if (fileInput) fileInput.value = '';
    const denoiseSelect = byId('vc-denoise-mode');
    if (denoiseSelect) denoiseSelect.value = 'auto';
    handleVoiceFileSelected(null);

    vcIsConvertingFile = false;
    vcConvertedFileBlob = null;
    vcRecordedWavBlob = null;
    const micAudioBox = byId('vc-mic-audio-box');
    if (micAudioBox) micAudioBox.style.display = 'none';
    const micPreview = byId('vc-mic-preview');
    if (micPreview) micPreview.src = '';

    const timerEl = byId('vc-rec-timer');
    if (timerEl) timerEl.textContent = '00:00';

    const statusMsg = byId('vc-status-msg');
    if (statusMsg) {
      statusMsg.style.display = 'none';
      statusMsg.textContent = '';
    }
    const progressBox = byId('vc-progress-box');
    if (progressBox) progressBox.style.display = 'none';

    const submitBtn = byId('vc-submit-btn');
    if (submitBtn) {
      submitBtn.disabled = false;
      submitBtn.textContent = '🚀 Bắt đầu Clone giọng';
    }
  }

  function encodeWAV(samples, sampleRate) {
    const buffer = new ArrayBuffer(44 + samples.length * 2);
    const view = new DataView(buffer);
    function writeString(offset, string) {
      for (let i = 0; i < string.length; i++) {
        view.setUint8(offset + i, string.charCodeAt(i));
      }
    }
    writeString(0, 'RIFF');
    view.setUint32(4, 36 + samples.length * 2, true);
    writeString(8, 'WAVE');
    writeString(12, 'fmt ');
    view.setUint32(16, 16, true);
    view.setUint16(20, 1, true); // PCM format
    view.setUint16(22, 1, true); // Mono
    view.setUint32(24, sampleRate, true);
    view.setUint32(28, sampleRate * 2, true); // byte rate
    view.setUint16(32, 2, true); // block align
    view.setUint16(34, 16, true); // bits per sample
    writeString(36, 'data');
    view.setUint32(40, samples.length * 2, true);

    let offset = 44;
    for (let i = 0; i < samples.length; i++) {
      let s = Math.max(-1, Math.min(1, samples[i]));
      view.setInt16(offset, s < 0 ? s * 0x8000 : s * 0x7FFF, true);
      offset += 2;
    }
    return new Blob([view], { type: 'audio/wav' });
  }

  async function toggleVoiceRecording() {
    if (vcIsRecording) {
      stopVoiceRecording(false);
    } else {
      await startVoiceRecording();
    }
  }

  async function startVoiceRecording() {
    try {
      if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
        alert('Trình duyệt hoặc môi trường hiện tại không hỗ trợ ghi âm trực tiếp qua Micro. Vui lòng sử dụng tính năng tải file .wav/.mp3.');
        return;
      }
      try {
        vcMediaStream = await navigator.mediaDevices.getUserMedia({
          audio: {
            echoCancellation: false,
            noiseSuppression: false,
            autoGainControl: false,
            channelCount: 1,
            sampleRate: 48000
          }
        });
      } catch (micErr) {
        vcMediaStream = await navigator.mediaDevices.getUserMedia({ audio: true });
      }
      const AudioCtxClass = window.AudioContext || window.webkitAudioContext;
      vcAudioCtx = new AudioCtxClass();
      vcMicSource = vcAudioCtx.createMediaStreamSource(vcMediaStream);
      vcScriptNode = vcAudioCtx.createScriptProcessor(4096, 1, 1);
      vcRecordedSamples = [];

      vcScriptNode.onaudioprocess = (e) => {
        if (!vcIsRecording) return;
        const inputData = e.inputBuffer.getChannelData(0);
        for (let i = 0; i < inputData.length; i++) {
          vcRecordedSamples.push(inputData[i]);
        }
      };

      vcMicSource.connect(vcScriptNode);
      vcScriptNode.connect(vcAudioCtx.destination);

      vcIsRecording = true;
      vcRecordStartTime = Date.now();

      const recBtn = byId('vc-record-btn');
      const recDot = byId('vc-rec-dot');
      const recText = byId('vc-rec-text');
      const timerEl = byId('vc-rec-timer');

      if (recBtn) recBtn.classList.add('vc-recording-pulse');
      if (recDot) recDot.style.background = '#ff0055';
      if (recText) recText.textContent = 'Dừng ghi âm';

      if (vcRecordingTimer) clearInterval(vcRecordingTimer);
      vcRecordingTimer = setInterval(() => {
        const elapsed = Math.floor((Date.now() - vcRecordStartTime) / 1000);
        const mins = String(Math.floor(elapsed / 60)).padStart(2, '0');
        const secs = String(elapsed % 60).padStart(2, '0');
        if (timerEl) timerEl.textContent = `${mins}:${secs}`;

        if (elapsed >= 20) {
          stopVoiceRecording(false);
        }
      }, 200);

    } catch (err) {
      console.error('[VoiceRec] Không thể mở micro:', err);
      alert('Không thể truy cập Micro: ' + (err.message || err));
    }
  }

  function stopVoiceRecording(cancelled = false) {
    vcIsRecording = false;
    if (vcRecordingTimer) {
      clearInterval(vcRecordingTimer);
      vcRecordingTimer = null;
    }
    if (vcScriptNode) {
      try { vcScriptNode.disconnect(); } catch (e) { }
      vcScriptNode = null;
    }
    if (vcMicSource) {
      try { vcMicSource.disconnect(); } catch (e) { }
      vcMicSource = null;
    }
    if (vcMediaStream) {
      try {
        vcMediaStream.getTracks().forEach(track => track.stop());
      } catch (e) { }
      vcMediaStream = null;
    }

    const recBtn = byId('vc-record-btn');
    const recDot = byId('vc-rec-dot');
    const recText = byId('vc-rec-text');
    if (recBtn) recBtn.classList.remove('vc-recording-pulse');
    if (recDot) recDot.style.background = '#fff';
    if (recText) recText.textContent = 'Bắt đầu ghi âm';

    if (!cancelled && vcRecordedSamples.length > 0 && vcAudioCtx) {
      const sampleRate = vcAudioCtx.sampleRate || 44100;
      vcRecordedWavBlob = encodeWAV(vcRecordedSamples, sampleRate);
      const micAudioBox = byId('vc-mic-audio-box');
      const micPreview = byId('vc-mic-preview');
      if (micAudioBox) micAudioBox.style.display = 'flex';
      if (micPreview) {
        micPreview.src = URL.createObjectURL(vcRecordedWavBlob);
        micPreview.load();
      }
    }

    if (vcAudioCtx) {
      try { vcAudioCtx.close(); } catch (e) { }
      vcAudioCtx = null;
    }
  }

  async function submitVoiceClone() {
    const nameInput = byId('vc-name');
    const descInput = byId('vc-desc');
    const genderSelect = byId('vc-gender');
    const statusMsg = byId('vc-status-msg');
    const progressBox = byId('vc-progress-box');
    const submitBtn = byId('vc-submit-btn');

    const name = (nameInput ? nameInput.value : '').trim();
    if (!name) {
      showVoiceModalStatus('Vui lòng nhập tên cho giọng đọc.', 'error');
      if (nameInput) nameInput.focus();
      return;
    }

    let audioFileToSend = null;
    let filename = 'reference.wav';

    if (vcSourceMode === 'file') {
      if (vcIsConvertingFile) {
        showVoiceModalStatus('Đang chuẩn hóa file âm thanh sang chuẩn WAV, vui lòng chờ giây lát...', 'info');
        return;
      }
      const fileInput = byId('vc-file-input');
      if (!fileInput || !fileInput.files || !fileInput.files[0]) {
        showVoiceModalStatus('Vui lòng chọn 1 file âm thanh (.wav, .mp3, .m4a...).', 'error');
        return;
      }
      audioFileToSend = vcConvertedFileBlob || fileInput.files[0];
      filename = 'reference.wav';
    } else {
      if (!vcRecordedWavBlob || vcRecordedWavBlob.size < 2000) {
        showVoiceModalStatus('Vui lòng thực hiện ghi âm ít nhất 3 giây trước khi clone giọng.', 'error');
        return;
      }
      audioFileToSend = vcRecordedWavBlob;
      filename = 'microphone_reference.wav';
    }

    const formData = new FormData();
    formData.append('file', audioFileToSend, filename);
    formData.append('name', name);
    formData.append('description', (descInput ? descInput.value : '').trim() || 'Giọng cá nhân');
    formData.append('gender', (genderSelect ? genderSelect.value : '') || '');
    formData.append('style', 'tu_nhien');
    const denoiseSelect = byId('vc-denoise-mode');
    formData.append('denoise', (denoiseSelect ? denoiseSelect.value : '') || 'auto');

    if (submitBtn) {
      submitBtn.disabled = true;
      submitBtn.textContent = '⏳ Đang xử lý...';
    }
    if (progressBox) progressBox.style.display = 'block';
    if (statusMsg) statusMsg.style.display = 'none';

    try {
      const res = await fetch('/api/voices/enroll', {
        method: 'POST',
        body: formData
      });
      const result = await res.json();

      if (!res.ok || result.success === false) {
        const errDetail = result.error || result.detail || result.msg || 'Không thể tạo giọng đọc.';
        showVoiceModalStatus('Lỗi: ' + errDetail, 'error');
        if (submitBtn) {
          submitBtn.disabled = false;
          submitBtn.textContent = '🚀 Bắt đầu Clone giọng';
        }
        if (progressBox) progressBox.style.display = 'none';
        return;
      }

      showVoiceModalStatus(`🎉 Chúc mừng! Giọng "${name}" đã được clone thành công và nạp vào hệ thống.`, 'success');
      if (progressBox) progressBox.style.display = 'none';

      await loadVoiceCatalog(name);
      selectVoiceDirectly(name);

      setTimeout(() => {
        switchVoiceModalTab('list');
      }, 1200);

    } catch (err) {
      console.error('[VoiceClone] Lỗi mạng/API:', err);
      showVoiceModalStatus('Không thể kết nối tới dịch vụ TTS: ' + (err.message || err), 'error');
      if (submitBtn) {
        submitBtn.disabled = false;
        submitBtn.textContent = '🚀 Bắt đầu Clone giọng';
      }
      if (progressBox) progressBox.style.display = 'none';
    }
  }

  function showVoiceModalStatus(msg, type = 'error') {
    const statusMsg = byId('vc-status-msg');
    if (!statusMsg) return;
    statusMsg.style.display = 'block';
    statusMsg.textContent = msg;
    if (type === 'error') {
      statusMsg.style.background = 'rgba(255, 0, 85, 0.15)';
      statusMsg.style.border = '1px solid rgba(255, 0, 85, 0.3)';
      statusMsg.style.color = '#fda4af';
    } else {
      statusMsg.style.background = 'rgba(16, 185, 129, 0.15)';
      statusMsg.style.border = '1px solid rgba(16, 185, 129, 0.3)';
      statusMsg.style.color = '#6ee7b7';
    }
  }

  function renderCustomVoiceList() {
    const container = byId('vc-list-container');
    if (!container) return;
    container.innerHTML = '';

    const currentVoice = (byId('tts-voice-select') || {}).value || state.speechVoice || '';
    const customVoices = (voiceCatalog && voiceCatalog.custom) ? Array.from(voiceCatalog.custom.values()) : [];
    updateVoiceCountBadge();

    if (customVoices.length === 0) {
      container.innerHTML = `
        <div style="text-align: center; padding: 30px 15px; color: var(--text-muted);">
          <div style="font-size: 32px; margin-bottom: 8px;">🎭</div>
          <div style="font-size: 13.5px; font-weight: 600; color: #cbd5e1; margin-bottom: 4px;">Chưa có giọng cá nhân nào</div>
          <div style="font-size: 11.5px; line-height: 1.5; margin-bottom: 14px;">
            Bạn có thể tạo giọng AI mang phong cách riêng của bạn để livestream bán hàng tự động.
          </div>
          <button type="button" class="btn btn-cyan" onclick="switchVoiceModalTab('create')" style="height: 32px; padding: 0 14px; font-size: 12px; border-radius: 6px; cursor: pointer;">
            ➕ Tạo giọng đầu tiên của bạn
          </button>
        </div>
      `;
      return;
    }

    customVoices.forEach(voice => {
      const isSelected = (currentVoice === voice.name);
      const card = document.createElement('div');
      card.className = 'vc-voice-item';
      card.innerHTML = `
        <div style="min-width: 0; flex: 1;">
          <div style="display: flex; align-items: center; gap: 8px; margin-bottom: 3px;">
            <strong style="color: #fff; font-size: 13.5px;">${voice.name}</strong>
            ${voice.gender ? `<span style="font-size: 10px; background: rgba(255,255,255,0.08); padding: 1px 6px; border-radius: 4px; color: #94a3b8;">${voice.gender}</span>` : ''}
            ${isSelected ? `<span style="font-size: 10px; background: rgba(16,185,129,0.2); color: var(--success); border: 1px solid rgba(16,185,129,0.4); padding: 1px 6px; border-radius: 4px; font-weight: bold;">ĐANG CHỌN</span>` : ''}
          </div>
          <div style="font-size: 11.5px; color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">
            ${voice.description || 'Giọng AI cá nhân đã nạp'}
          </div>
        </div>
        <div style="display: flex; align-items: center; gap: 6px; flex-shrink: 0;">
          <button type="button" class="btn btn-gray" onclick="testVoiceAudio('${voice.name.replace(/'/g, "\\'")}', this)" style="height: 28px; padding: 0 10px; font-size: 11.5px; border-radius: 5px; cursor: pointer;" title="Đọc thử câu mẫu">
            🔊 Thử giọng
          </button>
          ${!isSelected ? `
            <button type="button" class="btn btn-cyan" onclick="selectVoiceDirectly('${voice.name.replace(/'/g, "\\'")}')" style="height: 28px; padding: 0 10px; font-size: 11.5px; border-radius: 5px; cursor: pointer;">
              Chọn dùng
            </button>
          ` : ''}
          <button type="button" class="btn btn-gray" onclick="deleteVoiceDirectly('${(voice.id || voice.voice_id || voice.name).replace(/'/g, "\\'")}', '${voice.name.replace(/'/g, "\\'")}')" style="height: 28px; padding: 0 8px; font-size: 12px; border-radius: 5px; cursor: pointer; color: var(--danger);" title="Xóa giọng này">
            🗑️
          </button>
        </div>
      `;
      container.appendChild(card);
    });
  }

  function selectVoiceDirectly(voiceName) {
    const select = byId('tts-voice-select');
    if (select) {
      select.value = voiceName;
      state.speechVoice = voiceName;
      localStorage.setItem('slive-vip-voice', voiceName);
      syncSpeechSettings().catch(() => { });
    }
    closeVoiceModal();
  }

  async function deleteVoiceDirectly(voiceId, voiceName) {
    showCustomConfirm(
      'XÁC NHẬN XÓA GIỌNG',
      `Bạn có chắc chắn muốn xóa giọng cá nhân "${voiceName}" không?\nThao tác này sẽ xóa vĩnh viễn profile giọng đã clone.`,
      async () => {
        try {
          const res = await fetch('/api/voices/' + encodeURIComponent(voiceId), {
            method: 'DELETE'
          });
          const data = await res.json();
          if (!res.ok || data.success === false) {
            showAppAlert('Lỗi xóa giọng', data.error || 'Không thể xóa giọng này.');
            return;
          }
          await loadVoiceCatalog();
          renderCustomVoiceList();
        } catch (err) {
          showAppAlert('Lỗi kết nối', 'Không thể kết nối để xóa giọng: ' + (err.message || err));
        }
      },
      {
        icon: '🗑️',
        confirmText: 'XÓA GIỌNG',
        cancelText: 'HỦY BỎ',
        danger: true
      }
    );
  }

  async function testVoiceAudio(voiceName, btnEl) {
    if (vcCurrentTestAudio) {
      try { vcCurrentTestAudio.pause(); } catch (e) { }
      vcCurrentTestAudio = null;
    }
    const originalText = btnEl ? btnEl.textContent : '';
    if (btnEl) {
      btnEl.disabled = true;
      btnEl.textContent = '⏳ Đang sinh...';
    }
    try {
      const sampleText = 'Xin chào, đây là giọng đọc AI được clone từ giọng thật của bạn. Chúc bạn livestream bán hàng chốt thật nhiều đơn nhé!';
      const res = await fetch('/api/voices/test', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          text: sampleText,
          voice: voiceName,
          speed: 1.15
        })
      });
      if (!res.ok) {
        throw new Error('TTS Service status ' + res.status);
      }
      const blob = await res.blob();
      const audioUrl = URL.createObjectURL(blob);
      const audio = new Audio(audioUrl);
      vcCurrentTestAudio = audio;
      if (btnEl) btnEl.textContent = '🔊 Đang đọc...';
      audio.onended = () => {
        if (btnEl) {
          btnEl.disabled = false;
          btnEl.textContent = originalText;
        }
      };
      audio.onerror = () => {
        if (btnEl) {
          btnEl.disabled = false;
          btnEl.textContent = originalText;
        }
      };
      await audio.play();
    } catch (err) {
      console.error('[VoiceTest] Lỗi phát thử:', err);
      alert('Lỗi phát thử giọng: ' + (err.message || err));
      if (btnEl) {
        btnEl.disabled = false;
        btnEl.textContent = originalText;
      }
    }
  }

  async function startAvatarWindowStream() {
    if (!avatarChannel) throw new Error('Trình duyệt không hỗ trợ cửa sổ Avatar Live.');
    if (!await ensureAvatarWindow()) {
      throw new Error('Hãy chạy Live VIP bằng ứng dụng desktop để mở cửa sổ Avatar Live.');
    }
    sendAvatarSettings();
    return new Promise((resolve, reject) => {
      avatarConnectResolve = resolve;
      avatarConnectReject = reject;
      sendAvatarCommand('start');
      setTimeout(() => {
        if (!avatarConnectReject) return;
        avatarConnectReject(new Error('Cửa sổ Avatar Live không kết nối được WebRTC.'));
        avatarConnectResolve = null;
        avatarConnectReject = null;
      }, 30000);
    });
  }

  avatarChannel?.addEventListener('message', (event) => {
    const message = event.data || {};
    if (message.type === 'ready') {
      sendAvatarSettings();
      if (state.running || avatarConnectReject) sendAvatarCommand('start');
    } else if (message.type === 'connected') {
      if (byId('sessionid')) byId('sessionid').value = message.sessionid || '';
      syncSpeechSettings().catch(() => { });
      setWebRtcConnected(true, 'Online');
      avatarConnectResolve?.();
      avatarConnectResolve = null;
      avatarConnectReject = null;
    } else if (message.type === 'disconnected') {
      setWebRtcConnected(false, state.running ? 'Mất kết nối' : 'Offline');
    } else if (message.type === 'error') {
      avatarConnectReject?.(new Error(message.message || 'Không thể kết nối Avatar Live.'));
      avatarConnectResolve = null;
      avatarConnectReject = null;
    }
  });

  // Single Source of Truth Helpers for VIP Balance and Rate
  function getVipBalance(payload) {
    const nestedBalance = payload?.serviceBalances?.[SERVICE_TYPE]?.timeBalanceSeconds;
    if (nestedBalance !== undefined && nestedBalance !== null) {
      return Math.max(0, Number(nestedBalance) || 0);
    }

    // Session start/heartbeat responses expose the selected service balance
    // as flat fields instead of the profile's serviceBalances object.
    if (!payload?.serviceType || payload.serviceType === SERVICE_TYPE) {
      const flatBalance = payload?.remainingSeconds ?? payload?.timeBalanceSeconds;
      if (flatBalance !== undefined && flatBalance !== null) {
        return Math.max(0, Number(flatBalance) || 0);
      }
    }

    return 0;
  }

  function getVipRate(profile) {
    // 1. Custom rate for user's live_face service if set (> 0)
    const custom = profile?.serviceRates?.live_face?.hourlyRateVnd;
    if (Number.isFinite(custom) && custom > 0) return custom;

    // 2. Default rate configured for live_face in system pricing
    const defaultPricing = profile?.servicePricing?.live_face?.defaultHourlyRate;
    if (Number.isFinite(defaultPricing) && defaultPricing > 0) return defaultPricing;

    // 3. Fallback to general hourlyRateVnd if returned
    if (Number.isFinite(profile?.hourlyRateVnd) && profile.hourlyRateVnd > 0) return profile.hourlyRateVnd;

    // 4. Default fallback for Live VIP
    return 20000;
  }

  // Format Helpers
  const formatTime = (seconds) => {
    const safe = Math.max(0, Math.floor(Number(seconds) || 0));
    const hours = Math.floor(safe / 3600);
    const minutes = Math.floor((safe % 3600) / 60);
    const secs = safe % 60;
    return [hours, minutes, secs].map((p) => String(p).padStart(2, '0')).join(':');
  };

  const formatMoney = (val) => `${Math.max(0, Math.round(Number(val) || 0)).toLocaleString('vi-VN')} xu`;
  const formatDate = (val) => val ? new Date(val).toLocaleString('vi-VN') : '-';

  const escapeHtml = (val) => String(val ?? '').replace(/[&<>'"]/g, (c) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', "'": '&#39;', '"': '&quot;'
  })[c]);

  const safeStatus = (val) => String(val || 'unknown').replace(/[^a-z_-]/gi, '');
  const statusLabelVi = (val) => ({
    pending: 'Chờ thanh toán',
    completed: 'Thành công',
    cancelled: 'Đã hủy',
    active: 'Đang chạy',
    ended: 'Hoàn thành',
    expired: 'Hết hạn',
    failed: 'Thất bại',
    running: 'Đang xử lý',
    done: 'Hoàn tất'
  })[String(val || '').toLowerCase()] || String(val || 'Không rõ');

  // Generic API Helper
  async function api(url, options = {}) {
    const response = await fetch(url, options);
    const payload = await response.json().catch(() => ({}));
    if (payload.action === 'force_logout' || payload.status === 'locked') {
      handleLockedAccount(payload);
      const error = new Error(payload.message || payload.error || 'Tài khoản đã bị khóa.');
      error.isForcedLogout = true;
      throw error;
    }
    if (response.status === 401 || response.status === 403) {
      stopProfilePolling();
      stopInvoicePolling({ clearStoredId: false });
      window.location.href = '/login.html';
      throw new Error('Phiên đăng nhập đã hết hạn.');
    }
    if (!response.ok || payload.success === false || (typeof payload.code === 'number' && payload.code !== 0)) {
      const error = new Error(payload.error || payload.message || payload.msg || 'Yêu cầu không thành công.');
      error.status = response.status;
      throw error;
    }
    return payload;
  }

  const resourceSize = (bytes) => {
    if (!bytes) return 'Kích thước theo máy chủ';
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let value = Number(bytes);
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) {
      value /= 1024;
      unit += 1;
    }
    return `${value.toFixed(unit ? 1 : 0)} ${units[unit]}`;
  };

  function renderResourceStatus(payload) {
    state.resourceStatus = payload;
    state.resourcesKnown = true;
    const progress = payload.progress || {};
    state.resourcesInstalled = Boolean(payload.installed && !progress.running);
    if (state.resourcesInstalled && state.resourcePoll) {
      clearInterval(state.resourcePoll);
      state.resourcePoll = null;
    }
    const bundles = payload.bundles || [];
    const list = byId('resource-bundles');
    if (list) {
      list.innerHTML = bundles.map((bundle) => {
        const isRunning = Boolean(progress.running);
        const bPercent = bundle.installed ? 100 : Math.min(100, Math.max(0, bundle.percent || 0));
        const bStatus = bundle.status || (bundle.installed ? 'installed' : (isRunning ? 'downloading' : 'pending'));
        const isCurDownloading = bStatus === 'downloading';
        const isExtracting = bStatus === 'extracting';

        let statusBadge = '';
        if (bundle.installed) {
          statusBadge = `<span class="resource-state ok" style="color:#10b981;font-weight:600;font-size:13px;">✓ Hoàn thành</span>`;
        } else if (isExtracting || isCurDownloading) {
          const pctText = bPercent > 0 ? ` ${bPercent}%` : '';
          statusBadge = `<span class="resource-state" style="color:#00f0ff;font-weight:600;font-size:13px;">⚡ Đang thiết lập${pctText}</span>`;
        } else {
          statusBadge = `<span class="resource-state" style="color:#94a3b8;font-weight:500;font-size:13px;">Chờ thiết lập</span>`;
        }

        const downloadedText = (isCurDownloading && bundle.downloaded > 0)
          ? `${resourceSize(bundle.downloaded)} / ${resourceSize(bundle.size)}`
          : resourceSize(bundle.size);

        return `
        <div class="resource-bundle" style="display:flex;align-items:center;justify-content:space-between;padding:12px 16px;margin-bottom:8px;background:rgba(255,255,255,0.03);border:1px solid ${isCurDownloading ? 'rgba(0,240,255,0.35)' : 'rgba(255,255,255,0.08)'};border-radius:10px;">
          <div>
            <div style="font-weight:600;font-size:14px;color:#f1f5f9;">${escapeHtml(bundle.name || bundle.id || 'Tài nguyên')}</div>
            <small style="color:#94a3b8;font-size:12px;">${downloadedText}</small>
          </div>
          <div style="margin-left:14px;">${statusBadge}</div>
        </div>`;
      }).join('');
    }
    const percent = Math.max(0, Math.min(100, Number(progress.percent) || (payload.installed ? 100 : 0)));
    if (byId('resource-progress-bar')) byId('resource-progress-bar').style.width = `${percent}%`;
    setText('resource-percent', `${percent}%`);
    setText('resource-status', progress.current || (payload.installed ? 'Tài nguyên đã sẵn sàng' : 'Sẵn sàng thiết lập'));
    setText('resource-error', progress.error || '');

    // Cập nhật thông số: Dung lượng, Tốc độ, Thời gian còn lại
    const downloaded = progress.downloaded || 0;
    const total = progress.total || 0;
    const isRunning = !!progress.running;
    const sizeEl = byId('dashboard-resource-size');
    const speedEl = byId('dashboard-resource-speed');
    const etaEl = byId('dashboard-resource-eta');

    if (sizeEl) {
      sizeEl.textContent = total > 0 ? `${resourceSize(downloaded)} / ${resourceSize(total)}` : (payload.installed ? 'Đầy đủ' : '-- / --');
    }
    if (speedEl) {
      speedEl.textContent = isRunning
        ? (progress.speed_text || (progress.speed_mb > 0 ? `${progress.speed_mb.toFixed(1)} MB/s` : 'Đang tính...'))
        : (payload.installed ? 'Hoàn tất' : '-- MB/s');
    }
    if (etaEl) {
      let etaStr = progress.eta_text || '';
      if ((!etaStr || etaStr === 'Đang tính toán...' || etaStr === '--') && progress.speed_mb > 0.05 && total > downloaded) {
        const sec = Math.round((total - downloaded) / (progress.speed_mb * 1024 * 1024));
        if (sec > 0) {
          etaStr = sec < 60 ? `Khoảng ${sec} giây` : `Khoảng ${Math.floor(sec / 60)} phút ${sec % 60}s`;
        }
      }
      etaEl.textContent = isRunning ? (etaStr || 'Đang tính toán...') : (payload.installed ? 'Hoàn thành' : '--');
    }

    const notice = byId('resource-download-notice');
    if (notice) {
      if (progress.running) {
        notice.innerHTML = '<span>⚠️</span> <span>Vui lòng không đóng app khi đang tải</span>';
        notice.style.background = 'rgba(245, 158, 11, 0.12)';
        notice.style.borderColor = 'rgba(245, 158, 11, 0.35)';
        notice.style.color = '#f59e0b';
      } else if (state.resourcesInstalled) {
        notice.innerHTML = '<span>✅</span> <span>Đã tải xong toàn bộ tài nguyên. Vui lòng đóng app và mở lại.</span>';
        notice.style.background = 'rgba(16, 185, 129, 0.15)';
        notice.style.borderColor = 'rgba(16, 185, 129, 0.4)';
        notice.style.color = '#10b981';
      } else {
        notice.innerHTML = '<span>⚠️</span> <span>Vui lòng không đóng app khi đang tải</span>';
        notice.style.background = 'rgba(245, 158, 11, 0.12)';
        notice.style.borderColor = 'rgba(245, 158, 11, 0.35)';
        notice.style.color = '#f59e0b';
      }
    }

    if (state.resourcesInstalled) {
      setText('resource-status', 'Đã tải xong toàn bộ tài nguyên. Vui lòng đóng app và mở lại.');
    }

    // Nút HỦY TẢI / TẢI LẠI trong modal
    const cancelBtn = byId('resource-cancel-btn');
    if (cancelBtn) {
      if (progress.running) {
        cancelBtn.style.display = 'inline-block';
        cancelBtn.style.background = '#ef4444';
        cancelBtn.style.color = '#ffffff';
        cancelBtn.textContent = '⏹ HỦY TẢI';
        cancelBtn.onclick = () => confirmCancelFromModal();
      } else if (!state.resourcesInstalled) {
        cancelBtn.style.display = 'inline-block';
        cancelBtn.style.background = '#00e9ff';
        cancelBtn.style.color = '#050b14';
        cancelBtn.textContent = '🔄 TẢI LẠI';
        cancelBtn.onclick = () => handleResourceAction();
      } else {
        cancelBtn.style.display = 'none';
      }
    }

    // Nút duy nhất ĐÓNG: đóng modal bất cứ lúc nào, tải vẫn chạy ngầm
    const action = byId('resource-action');
    const later = byId('resource-later');
    if (later) {
      later.style.display = 'none';
    }
    const close = byId('resource-modal-x');
    if (state.resourceClosedAfterInstall) {
      if (action) {
        action.disabled = true;
        action.style.opacity = '0.6';
        action.style.cursor = 'not-allowed';
        action.innerHTML = '<span style="display:inline-block;width:14px;height:14px;border:2px solid #050b14;border-top-color:transparent;border-radius:50%;animation:spin 0.8s linear infinite;margin-right:6px;vertical-align:middle;"></span> ĐANG ĐÓNG APP...';
      }
      if (close) {
        close.disabled = true;
        close.style.pointerEvents = 'none';
        close.style.opacity = '0.5';
      }
    } else {
      if (action) {
        action.disabled = false;
        action.style.opacity = '1';
        action.style.cursor = 'pointer';
        action.textContent = 'ĐÓNG';
        action.onclick = () => handleCloseResourceClick();
      }
      if (close) {
        close.disabled = false;
        close.removeAttribute('disabled');
        close.style.pointerEvents = 'auto';
        close.style.opacity = '1';
        close.onclick = () => handleCloseResourceClick();
      }
    }

    const resourceButton = byId('btn-topbar-resources');
    resourceButton?.classList.toggle('installed', state.resourcesInstalled);
    setText('resource-header-badge', state.resourcesInstalled ? 'Đã cài' : progress.running ? `${percent}%` : 'Cần tải');
    byId('resource-callout')?.classList.toggle('hidden', state.resourcesInstalled);
  }

  async function refreshResourceStatus({ autoOpen = false } = {}) {
    try {
      const payload = await api('/api/resources/status');
      renderResourceStatus(payload);
      if (payload.progress?.running && !state.resourcePoll) {
        state.resourcePoll = setInterval(() => refreshResourceStatus().catch(() => { }), 2000);
      }
      if (autoOpen && !payload.installed && !state.userClosedResourceModal) {
        openResourceModal();
        if (!payload.progress?.running && !state.userCanceledDownload && !state.autoInstallTriggered) {
          state.autoInstallTriggered = true;
          handleResourceAction();
        }
      }
      return payload;
    } catch (error) {
      if (error.status === 404) {
        state.resourcesKnown = true;
        state.resourcesInstalled = true;
        byId('resource-callout')?.classList.add('hidden');
        setText('resource-header-badge', 'Đã cài');
        byId('btn-topbar-resources')?.classList.add('installed');
      } else {
        state.resourcesKnown = false;
        state.resourcesInstalled = false;
        setText('resource-header-badge', 'Cần tải');
        setText('resource-error', error.message || 'Không thể kiểm tra tài nguyên.');
      }
      return null;
    }
  }

  window.openResourceModal = function () {
    state.userClosedResourceModal = false;
    const modal = byId('resource-modal');
    if (modal) {
      modal.style.display = 'flex';
      modal.classList.add('open');
    }
    refreshResourceStatus().then((payload) => {
      if (payload && !payload.installed && !payload.progress?.running && !state.userCanceledDownload) {
        handleResourceAction();
      }
    }).catch(() => { });
  };

  // Cho phép đóng modal bất cứ lúc nào, quá trình tải vẫn tiếp tục chạy ngầm
  window.closeResourceModal = function () {
    state.userClosedResourceModal = true;
    const modal = byId('resource-modal');
    if (modal) {
      modal.classList.remove('open');
      modal.style.display = 'none';
    }
  };

  // Xử lý nút ĐÓNG: hiện spinner xác nhận đã click, khi tải xong chỉ đóng 1 lần và disable vĩnh viễn
  window.handleCloseResourceClick = function () {
    const action = byId('resource-action');
    const close = byId('resource-modal-x');

    // 1. Khi đã tải xong toàn bộ tài nguyên: chỉ đóng một lần duy nhất, hiện spin và disable nút
    if (state.resourcesInstalled) {
      if (state.resourceClosedAfterInstall) return;
      state.resourceClosedAfterInstall = true;
      if (action) {
        action.disabled = true;
        action.style.opacity = '0.6';
        action.style.cursor = 'not-allowed';
        action.innerHTML = '<span style="display:inline-block;width:14px;height:14px;border:2px solid #050b14;border-top-color:transparent;border-radius:50%;animation:spin 0.8s linear infinite;margin-right:6px;vertical-align:middle;"></span> ĐANG ĐÓNG APP...';
      }
      if (close) {
        close.disabled = true;
        close.style.pointerEvents = 'none';
        close.style.opacity = '0.5';
      }
      if (state.resourcePoll) {
        clearInterval(state.resourcePoll);
        state.resourcePoll = null;
      }
      closeApp();
      return;
    }

    // 2. Khi đang tải ngầm: bấm ĐÓNG sẽ hiện spinner nhẹ và đóng modal
    if (action) {
      action.disabled = true;
      action.innerHTML = '<span style="display:inline-block;width:14px;height:14px;border:2px solid #050b14;border-top-color:transparent;border-radius:50%;animation:spin 0.8s linear infinite;margin-right:6px;vertical-align:middle;"></span> Đang đóng...';
    }
    if (close) close.style.pointerEvents = 'none';
    setTimeout(() => {
      closeResourceModal();
      if (action && !state.resourceClosedAfterInstall) {
        action.disabled = false;
        action.textContent = 'ĐÓNG';
      }
      if (close && !state.resourceClosedAfterInstall) close.style.pointerEvents = 'auto';
    }, 250);
  };

  // Tải riêng từng gói tài nguyên được chọn
  window.downloadSingleBundle = async function (bundleId) {
    if (!bundleId) return;
    try {
      setText('resource-error', '');
      setText('resource-status', 'Đang khởi động kết nối tải gói...');
      const payload = await api('/api/resources/install', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ bundle_id: bundleId })
      });
      renderResourceStatus(payload);
      clearInterval(state.resourcePoll);
      state.resourcePoll = setInterval(() => refreshResourceStatus().catch(() => { }), 2000);
    } catch (error) {
      setText('resource-error', error.message || 'Lỗi khi tải gói tài nguyên.');
    }
  };

  window.confirmCancelFromModal = function () {
    showCustomConfirm(
      'CẢNH BÁO HỦY TẢI TÀI NGUYÊN',
      'Quá trình tải các thành phần vận hành Slive VIP đang diễn ra. Nếu hủy tải, các tính năng Livestream và AI sẽ tạm dừng cho đến khi bạn tải lại.\n\nBạn có chắc chắn muốn hủy tải không?',
      async () => {
        try {
          state.userCanceledDownload = true;
          setText('resource-error', '');
          setText('resource-status', 'Đã hủy tải tài nguyên.');
          const payload = await api('/api/resources/cancel', { method: 'POST' });
          renderResourceStatus(payload);
        } catch (error) {
          setText('resource-error', error.message || 'Lỗi khi hủy tải tài nguyên.');
        }
      },
      {
        icon: '⚠️',
        confirmText: 'HỦY TẢI',
        cancelText: 'TIẾP TỤC TẢI',
        danger: true
      }
    );
  };

  window.closeApp = function () {
    if (state.resourceStatus?.progress?.running) {
      showCustomConfirm(
        'CẢNH BÁO ĐÓNG ỨNG DỤNG',
        'Quá trình tải tài nguyên Slive VIP đang diễn ra. Nếu đóng app lúc này, việc tải sẽ bị gián đoạn và bạn sẽ cần tải lại vào lần mở sau.\n\nBạn có chắc chắn muốn đóng ứng dụng không?',
        async () => {
          try {
            await api('/api/app/exit', { method: 'POST' });
          } catch (_) { }
          try {
            window.close();
          } catch (_) { }
        },
        {
          icon: '🛑',
          confirmText: 'ĐÓNG APP',
          cancelText: 'Ở LẠI APP',
          danger: true
        }
      );
      return;
    }
    (async () => {
      try {
        await api('/api/app/exit', { method: 'POST' });
      } catch (_) { }
      try {
        window.close();
      } catch (_) { }
    })();
  };

  // Cảnh báo người dùng khi cố đóng tab / cửa sổ trong lúc tải tài nguyên
  window.addEventListener('beforeunload', (event) => {
    if (state.resourceStatus?.progress?.running) {
      event.preventDefault();
      event.returnValue = 'Quá trình tải tài nguyên Slive VIP đang diễn ra. Bạn có chắc muốn rời đi?';
      return event.returnValue;
    }
  });

  window.handleResourceAction = async function () {
    const action = byId('resource-action');
    try {
      if (state.resourcesInstalled) {
        if (state.resourceStatus?.mode === 'bootstrap') {
          closeApp();
        } else {
          closeResourceModal();
        }
        return;
      }
      state.userCanceledDownload = false;
      setText('resource-error', '');
      setText('resource-status', 'Đang thiết lập kết nối máy chủ tài nguyên...');
      if (action) {
        action.disabled = false;
        action.textContent = 'ĐÓNG';
        action.onclick = () => closeResourceModal();
      }
      const payload = await api('/api/resources/install', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({})
      });
      renderResourceStatus(payload);
      if (state.resourcePoll) clearInterval(state.resourcePoll);
      state.resourcePoll = setInterval(() => refreshResourceStatus().catch(() => { }), 2000);
    } catch (error) {
      setText('resource-error', error.message || 'Lỗi khi kết nối tải tài nguyên.');
      if (action) {
        action.disabled = false;
        action.textContent = 'ĐÓNG';
        action.onclick = () => closeResourceModal();
      }
    }
  };

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      closeResourceModal();
      if (typeof closeUpdateModal === 'function') closeUpdateModal();
    }
  });

  function requireResources() {
    if (state.resourcesKnown && state.resourcesInstalled) return true;
    openResourceModal();
    return false;
  }

  // Không chặn người dùng tương tác với giao diện, chỉ nhắc nhở nếu bấm nút Bắt đầu khi chưa có tài nguyên
  document.addEventListener('click', (event) => {
    if (state.resourcesKnown && state.resourcesInstalled) return;
    const control = event.target.closest('button');
    if (!control || control.id !== 'start') return;
    event.preventDefault();
    event.stopImmediatePropagation();
    openResourceModal();
  }, true);

  function isLockedResponse(data) {
    return data && (data.status === 'locked' || data.action === 'force_logout');
  }

  async function logoutForcedAccount() {
    if (state.forceLogoutFinishing) return;
    state.forceLogoutFinishing = true;
    if (state.forceLogoutTimer) clearInterval(state.forceLogoutTimer);
    state.forceLogoutTimer = null;
    try {
      await disconnectPresence();
      await fetch('/api/auth/logout', { method: 'POST' });
    } catch (error) {
      console.error('Forced logout request failed:', error);
    } finally {
      window.location.replace('/login.html');
    }
  }

  window.logoutForcedAccount = logoutForcedAccount;

  function updateForceLogoutCountdown() {
    const countdownEl = byId('force-logout-countdown');
    const buttonEl = byId('force-logout-button');
    if (countdownEl) countdownEl.textContent = state.forceLogoutRemainingSeconds;
    if (buttonEl) buttonEl.textContent = `Đăng xuất (${state.forceLogoutRemainingSeconds})`;
  }

  function showForceLogoutModal(options) {
    if (state.forceLogoutActive) return;
    state.forceLogoutActive = true;

    stopProfilePolling();
    stopInvoicePolling({ clearStoredId: false });
    stopTimers();
    stopPollingLogs();
    state.running = false;
    try { window.stop(); } catch (error) { console.error('WebRTC stop after account lock failed:', error); }

    const titleEl = byId('force-logout-modal-title');
    const messageEl = byId('force-logout-modal-message');
    if (titleEl) titleEl.textContent = options.title;
    if (messageEl) messageEl.textContent = options.message;

    byId('qr-modal')?.classList.remove('open');
    const lockedModal = byId('locked-modal');
    if (lockedModal) lockedModal.style.display = 'flex';

    state.forceLogoutRemainingSeconds = 10;
    updateForceLogoutCountdown();

    state.forceLogoutTimer = setInterval(() => {
      state.forceLogoutRemainingSeconds -= 1;
      if (state.forceLogoutRemainingSeconds <= 0) {
        clearInterval(state.forceLogoutTimer);
        state.forceLogoutTimer = null;
        logoutForcedAccount();
        return;
      }
      updateForceLogoutCountdown();
    }, 1000);
  }

  function handleLockedAccount(data) {
    if (!isLockedResponse(data) || state.forceLogoutActive) return;
    const message = data.message || data.error || 'Tài khoản đã bị khóa. Vui lòng liên hệ quản trị viên.';
    setText('billing-status', 'Tài khoản bị khóa');
    showForceLogoutModal({ title: 'Tài khoản bị khóa', message });
  }

  function setText(id, value) {
    const el = byId(id);
    if (el) {
      if (id === 'resource-error' || id.endsWith('-error') || id.endsWith('-notice')) {
        el.textContent = value || '';
        el.style.display = value ? 'block' : 'none';
      } else {
        el.textContent = (value !== undefined && value !== null && value !== '') ? value : '-';
      }
    }
  }

  // App Alert Modal (Replaces browser alert)
  window.showAppAlert = function (title, message) {
    setText('app-alert-title', title || '📢 Thông báo');
    setText('app-alert-message', message || '');
    byId('app-alert-modal')?.classList.add('open');
  };

  window.closeAppAlertModal = function () {
    byId('app-alert-modal')?.classList.remove('open');
  };

  window.openExpiredModal = function () {
    byId('expired-modal')?.classList.add('open');
  };

  window.closeExpiredModal = function () {
    byId('expired-modal')?.classList.remove('open');
  };

  // Update Topbar and Billing Displays
  function updateUiDisplays() {
    const elapsedStr = formatTime(state.elapsed);
    const balanceStr = formatTime(state.remaining);
    const cost = state.rate > 0 ? (state.elapsed / 3600) * state.rate : 0;

    setText('top-live-timer', elapsedStr);
    setText('mini-live-timer', elapsedStr);
    setText('top-time-balance', balanceStr);
    setText('top-hourly-rate', `${state.rate.toLocaleString('vi-VN')} xu/giờ`);

    setText('billing-timer', elapsedStr);
    setText('billing-balance-left', balanceStr);
    setText('billing-amount', formatMoney(cost));
    setText('billing-start-time', state.startedAt ? new Date(state.startedAt).toLocaleTimeString('vi-VN') : '--:--:--');
    setText('billing-status', state.running ? `Đang sử dụng ${SERVICE_LABEL} · ${elapsedStr}` : 'Chưa bắt đầu');
  }

  function renderUserProfileInfo() {
    const profile = state.profile || {};
    const user = profile.user || {};

    const username = profile.username || user.username || '--';
    setText('top-username', username);
    setText('profile-username', username);
    setText('profile-email', profile.email || user.email || '--');
    setText('profile-phone', profile.phone || user.phone || '--');
    setText('profile-status', (profile.isActive === false || user.isActive === false) ? 'Đang khóa' : 'Đang hoạt động');

    // Strict Live VIP Wallet Reading using Single Source of Truth
    setText('profile-balance-vip', formatTime(state.remaining));
    setText('profile-rate-vip', `${state.rate.toLocaleString('vi-VN')} xu/giờ`);

    // Strict Live Thường Separate Wallet Reading
    const normalBalance = profile.serviceBalances?.live_noface?.timeBalanceSeconds ?? 0;
    const normalRate = profile.serviceRates?.live_noface?.hourlyRateVnd ?? profile.servicePricing?.live_noface?.defaultHourlyRate ?? 10000;
    setText('profile-balance-noface', formatTime(normalBalance));
    setText('profile-rate-noface', `${normalRate.toLocaleString('vi-VN')} xu/giờ`);

    setText('profile-created-at', formatDate(profile.createdAt || user.createdAt));
    setText('profile-last-login-at', formatDate(profile.lastLoginAt || user.lastLoginAt));
  }

  function applyProfileData(profile) {
    state.profile = profile;

    // Update rate unconditionally
    state.rate = getVipRate(profile);

    // Only update remaining balance when live session is NOT running to prevent backwards timer jumps!
    if (!state.running) {
      state.remaining = getVipBalance(profile);
    }

    const tiktokInput = byId('tiktok-username');
    if (tiktokInput && !tiktokInput.value.trim() && !state.tiktokConnected) {
      const savedChannel = profile?.user?.currentTiktokChannel || profile?.user?.lastTiktokChannel;
      if (savedChannel) {
        tiktokInput.value = `@${savedChannel.replace(/^@+/, '')}`;
      }
    }

    renderUserProfileInfo();
    updateUiDisplays();
    calculatePackageDurations();
  }

  async function loadProfile() {
    try {
      const profile = await api('/api/billing/profile');
      applyProfileData(profile);
      return profile;
    } catch (err) {
      console.error('Profile load error:', err);
    }
  }

  // Periodic Profile Polling (Every 2 seconds)
  function startProfilePolling() {
    stopProfilePolling();
    state.profilePollInterval = setInterval(() => {
      if (state.running) {
        // Keep checking account status while live, but do not apply profile data
        // over the heartbeat-controlled timer and session rate snapshot.
        api('/api/billing/profile').catch((err) => {
          if (!err.isForcedLogout) console.error('Account status poll error:', err);
        });
        return;
      }
      loadProfile().catch((err) => console.error('Periodic profile poll error:', err));
    }, 2000);
  }

  function stopProfilePolling() {
    if (state.profilePollInterval) {
      clearInterval(state.profilePollInterval);
      state.profilePollInterval = null;
    }
  }

  async function sendPresenceHeartbeat() {
    if (state.presenceHeartbeatInFlight) return;
    state.presenceHeartbeatInFlight = true;
    try {
      await api('/api/billing/presence/heartbeat', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ clientId: presenceClientId }),
      });
    } catch (error) {
      if (!error.isForcedLogout) console.error('Presence heartbeat error:', error);
    } finally {
      state.presenceHeartbeatInFlight = false;
    }
  }

  function startPresenceHeartbeat() {
    stopPresenceHeartbeat();
    sendPresenceHeartbeat();
    state.presenceHeartbeatInterval = setInterval(sendPresenceHeartbeat, PRESENCE_HEARTBEAT_MS);
  }

  function stopPresenceHeartbeat() {
    if (state.presenceHeartbeatInterval) {
      clearInterval(state.presenceHeartbeatInterval);
      state.presenceHeartbeatInterval = null;
    }
  }

  async function disconnectPresence() {
    stopPresenceHeartbeat();
    try {
      await fetch('/api/billing/presence/disconnect', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ clientId: presenceClientId }),
        keepalive: true,
      });
    } catch (_) {
      // The backend timeout handles abrupt application shutdowns.
    }
  }

  window.addEventListener('focus', sendPresenceHeartbeat);
  window.addEventListener('pageshow', sendPresenceHeartbeat);
  window.addEventListener('online', sendPresenceHeartbeat);
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') sendPresenceHeartbeat();
  });

  function setServerStatus(online) {
    const el = byId('top-server-status');
    if (!el) return;
    if (online) {
      el.style.color = 'var(--success)';
      el.innerHTML = '<span style="display:inline-block; width:6px; height:6px; background:var(--success); border-radius:50%; box-shadow:0 0 6px var(--success)"></span> Online';
    } else {
      el.style.color = 'var(--danger)';
      el.innerHTML = '<span style="display:inline-block; width:6px; height:6px; background:var(--danger); border-radius:50%;"></span> Offline';
    }
  }

  function updateLiveIndicator(status) {
    const badge = byId('live-indicator-badge');
    const textEl = byId('live-indicator-text');
    if (!badge || !textEl) return;

    badge.classList.remove('status-ready', 'status-connecting', 'status-live', 'online');

    if (status === 'live' || (status === undefined && state.running && state.avatarConnected)) {
      badge.classList.add('status-live', 'online');
      textEl.textContent = 'ĐANG LIVE';
    } else if (status === 'connecting' || (status === undefined && state.running && !state.avatarConnected)) {
      badge.classList.add('status-connecting');
      textEl.textContent = 'Đang kết nối...';
    } else {
      badge.classList.add('status-ready');
      textEl.textContent = 'Sẵn sàng';
    }
  }

  function setWebRtcConnected(connected, label) {
    state.avatarConnected = Boolean(connected);
    setServerStatus(connected);
    byId('video-placeholder')?.classList.toggle('hidden', connected);
    setText('session-label', connected ? `SID: ${byId('sessionid').value}` : 'SID: -');

    updateLiveIndicator(connected ? 'live' : (state.running ? 'connecting' : 'ready'));

    if (connected) {
      syncSpeechSettings().catch(() => { });
    }
  }

  // Timer Management
  function startLocalTimer() {
    clearInterval(state.localTimer);
    state.localTimer = setInterval(() => {
      if (!state.running) return;
      state.elapsed += 1;
      state.remaining = Math.max(0, state.remaining - 1);
      updateUiDisplays();
    }, 1000);
  }

  function stopTimers() {
    clearInterval(state.billingTimer);
    clearInterval(state.localTimer);
    state.billingTimer = null;
    state.localTimer = null;
  }

  // Live content and TikTok feeds, styled and synchronized with WebRTC audio.
  function addLogMessage(containerId, text, kind = '') {
    const container = byId(containerId);
    if (!container) return;
    const item = document.createElement('div');
    item.className = `msg-item ${kind}`.trim();
    item.textContent = text;
    container.appendChild(item);
    if (containerId === 'chat-messages' && typeof chatScrollController !== 'undefined') {
      chatScrollController.scrollToBottom();
    } else if (containerId === 'event-messages' && typeof eventScrollController !== 'undefined') {
      eventScrollController.scrollToBottom();
    } else {
      container.scrollTop = container.scrollHeight;
    }
  }

  function trimFeed(container, limit = 100) {
    while (container.children.length > limit) container.firstElementChild?.remove();
  }

  /* ==========================================================================
     SMART AUTO-SCROLL & FREE BROWSING CONTROLLER (TIKTOK & SCRIPT STREAMS)
     - Cho phép người dùng tự do cuộn lên/xuống xem lịch sử mà không bị giật màn hình
     - Hiển thị nút nổi quay lại câu đang nói / tin mới nhất
     - Tự động giữ cuộn tiếp khi cuộn về câu đang nói hoặc tin mới
     ========================================================================== */
  function createStreamScrollController(options) {
    const {
      containerId,
      buttonId,
      getActiveElement,
      labelSpeaking = 'Quay lại câu đang nói',
      labelLatest = 'Cuộn xuống mới nhất'
    } = options;

    let autoFollow = true;
    let isProgrammaticScrolling = false;
    let scrollReleaseTimer = null;

    function getContainer() {
      return byId(containerId);
    }

    function getButton() {
      return byId(buttonId);
    }

    function isElementVisible(container, el) {
      if (!container || !el || !el.isConnected) return false;
      const cRect = container.getBoundingClientRect();
      const eRect = el.getBoundingClientRect();
      return (eRect.bottom >= cRect.top + 25 && eRect.top <= cRect.bottom - 25);
    }

    function getElementPosition(container, el) {
      if (!container || !el || !el.isConnected) return 'none';
      const cRect = container.getBoundingClientRect();
      const eRect = el.getBoundingClientRect();
      if (eRect.top < cRect.top + 25) return 'above';
      if (eRect.bottom > cRect.bottom - 25) return 'below';
      return 'visible';
    }

    function updateButtonState() {
      const container = getContainer();
      const button = getButton();
      if (!container || !button) return;

      if (autoFollow) {
        button.style.display = 'none';
        return;
      }

      const activeEl = getActiveElement ? getActiveElement() : null;
      const textSpan = button.querySelector('.btn-text');
      const arrowSvg = button.querySelector('.scroll-arrow-icon');

      if (activeEl && activeEl.isConnected) {
        const pos = getElementPosition(container, activeEl);
        if (pos === 'visible') {
          autoFollow = true;
          button.style.display = 'none';
          return;
        }

        button.style.display = 'inline-flex';
        if (textSpan) textSpan.textContent = labelSpeaking;
        if (arrowSvg) {
          if (pos === 'above') {
            arrowSvg.innerHTML = '<path d="M12 19V5M5 12l7-7 7 7"/>';
            button.title = 'Bấm để cuộn lên câu đang nói';
          } else {
            arrowSvg.innerHTML = '<path d="M12 5v14M19 12l-7 7-7-7"/>';
            button.title = 'Bấm để cuộn xuống câu đang nói';
          }
        }
      } else {
        const distFromBottom = container.scrollHeight - container.scrollTop - container.clientHeight;
        if (distFromBottom > 75) {
          button.style.display = 'inline-flex';
          if (textSpan) textSpan.textContent = labelLatest;
          if (arrowSvg) {
            arrowSvg.innerHTML = '<path d="M12 5v14M19 12l-7 7-7-7"/>';
            button.title = 'Bấm để cuộn xuống tin mới nhất';
          }
        } else {
          autoFollow = true;
          button.style.display = 'none';
        }
      }
    }

    function onUserInteraction() {
      if (isProgrammaticScrolling) return;
      const container = getContainer();
      if (!container) return;

      const activeEl = getActiveElement ? getActiveElement() : null;
      if (activeEl && activeEl.isConnected) {
        if (!isElementVisible(container, activeEl)) {
          autoFollow = false;
        } else {
          autoFollow = true;
        }
      } else {
        const distFromBottom = container.scrollHeight - container.scrollTop - container.clientHeight;
        if (distFromBottom > 75) {
          autoFollow = false;
        } else if (distFromBottom <= 35) {
          autoFollow = true;
        }
      }

      updateButtonState();
    }

    function scrollToElement(target, force = false) {
      const container = getContainer();
      if (!container || !target || (!autoFollow && !force)) return;

      isProgrammaticScrolling = true;
      if (scrollReleaseTimer) clearTimeout(scrollReleaseTimer);

      const cRect = container.getBoundingClientRect();
      const tRect = target.getBoundingClientRect();
      const centeredTop = container.scrollTop
        + (tRect.top - cRect.top)
        - Math.max(0, (container.clientHeight - tRect.height) / 2);

      container.scrollTo({ top: Math.max(0, centeredTop), behavior: 'smooth' });

      scrollReleaseTimer = setTimeout(() => {
        isProgrammaticScrolling = false;
        updateButtonState();
      }, 500);
    }

    function scrollToBottom(force = false) {
      const container = getContainer();
      if (!container || (!autoFollow && !force)) return;

      isProgrammaticScrolling = true;
      if (scrollReleaseTimer) clearTimeout(scrollReleaseTimer);

      container.scrollTo({ top: container.scrollHeight, behavior: 'smooth' });

      scrollReleaseTimer = setTimeout(() => {
        isProgrammaticScrolling = false;
        updateButtonState();
      }, 500);
    }

    function bindEvents() {
      const container = getContainer();
      const button = getButton();
      if (!container) return;

      if (!container.dataset.smartScrollHooked) {
        container.dataset.smartScrollHooked = 'true';
        container.addEventListener('scroll', onUserInteraction, { passive: true });
        container.addEventListener('wheel', () => {
          isProgrammaticScrolling = false;
          onUserInteraction();
        }, { passive: true });
        container.addEventListener('touchmove', () => {
          isProgrammaticScrolling = false;
          onUserInteraction();
        }, { passive: true });
      }

      if (button && !button.dataset.smartScrollHooked) {
        button.dataset.smartScrollHooked = 'true';
        button.addEventListener('click', (e) => {
          e.preventDefault();
          e.stopPropagation();
          autoFollow = true;
          const activeEl = getActiveElement ? getActiveElement() : null;
          if (activeEl && activeEl.isConnected) {
            scrollToElement(activeEl, true);
          } else {
            scrollToBottom(true);
          }
          button.style.display = 'none';
        });
      }
    }

    bindEvents();

    return {
      isAutoFollow: () => autoFollow,
      setAutoFollow: (val) => {
        autoFollow = val;
        updateButtonState();
      },
      scrollToElement,
      scrollToBottom,
      onSpeechActivated: (target) => {
        if (autoFollow) {
          scrollToElement(target, true);
        } else {
          updateButtonState();
        }
      },
      updateButtonState,
      bindEvents
    };
  }

  const chatScrollController = createStreamScrollController({
    containerId: 'chat-messages',
    buttonId: 'btn-scroll-active-chat',
    getActiveElement: () => byId('chat-messages')?.querySelector('.speech-track.active'),
    labelSpeaking: 'Quay lại câu đang nói',
    labelLatest: 'Cuộn xuống kịch bản mới'
  });

  const eventScrollController = createStreamScrollController({
    containerId: 'event-messages',
    buttonId: 'btn-scroll-active-event',
    getActiveElement: () => byId('event-messages')?.querySelector('.comment-item.active, .speech-track.active'),
    labelSpeaking: 'Quay lại câu đang nói',
    labelLatest: 'Cuộn xuống bình luận mới'
  });

  function updateActivePromptDisplay(topic) {
    state.activePromptTopic = topic;
    const topicEl = byId('active-prompt-topic');
    if (topicEl) topicEl.textContent = topic || 'Chưa có chủ đề nào được gửi';
    const card = byId('active-prompt-card');
    if (card) {
      if (topic) card.classList.add('has-content');
      else card.classList.remove('has-content');
    }
    const modalTopic = byId('modal-prompt-topic');
    if (modalTopic) modalTopic.textContent = topic || 'Chưa có chủ đề nào được gửi';
  }

  function updateOptimizedPromptDisplay(promptText) {
    state.activeOptimizedPrompt = promptText;
    const modalContent = byId('modal-prompt-content');
    if (modalContent) modalContent.textContent = promptText || 'Chưa có kịch bản tối ưu.';
    const editArea = byId('modal-prompt-edit-textarea');
    if (editArea && (!editArea.value || editArea.value.includes('Đang kết nối') || editArea.value.includes('Chưa có'))) {
      editArea.value = promptText || '';
    }
    const btn = byId('btn-view-prompt');
    if (btn) {
      btn.disabled = false;
    }
  }

  let promptProgressTimer = null;
  let promptProgressPercent = 0;
  let promptProgressStartTime = 0;

  function setPromptProgress(percent) {
    promptProgressPercent = Math.min(100, Math.max(0, percent));
    const percentEl = byId('generating-percent');
    if (percentEl) {
      percentEl.textContent = `${Math.round(promptProgressPercent)}%`;
    }
    const barEl = byId('generating-progress-bar');
    if (barEl) {
      barEl.style.width = `${promptProgressPercent}%`;
    }
  }

  function showPromptGeneratingIndicator() {
    const container = byId('chat-messages');
    if (!container) return;
    container.querySelector('.script-empty-state')?.remove();

    let indicator = byId('prompt-generating-indicator');
    if (indicator && indicator.dataset.finishing === 'true') {
      indicator.remove();
      indicator = null;
    }

    if (!indicator) {
      indicator = document.createElement('div');
      indicator.id = 'prompt-generating-indicator';
      indicator.className = 'prompt-generating-indicator';
      indicator.innerHTML = `
        <div class="generating-header">
          <div class="generating-spinner" id="generating-spinner"></div>
          <div class="generating-text-group">
            <div class="generating-title-row">
              <div class="generating-title" id="generating-title">
                <span>Đang nhập prompt để sinh nội dung live, vui lòng đợi...</span>
              </div>
              <div class="generating-percent" id="generating-percent">6%</div>
            </div>
            <div class="generating-subtitle" id="generating-subtitle">AI đang phân tích kịch bản và kết nối mô hình...</div>
          </div>
        </div>
        <div class="generating-progress-track">
          <div class="generating-progress-bar" id="generating-progress-bar" style="width: 6%"></div>
        </div>
      `;
      container.appendChild(indicator);
      container.scrollTop = container.scrollHeight;
    }

    if (promptProgressTimer) {
      clearInterval(promptProgressTimer);
      promptProgressTimer = null;
    }

    promptProgressStartTime = Date.now();
    promptProgressPercent = 6;
    setPromptProgress(6);

    promptProgressTimer = setInterval(() => {
      const elapsed = (Date.now() - promptProgressStartTime) / 1000;
      let target = 6;
      if (elapsed < 0.6) {
        // 6% -> 26% (0.6s)
        target = 6 + (elapsed / 0.6) * 20;
      } else if (elapsed < 1.8) {
        // 26% -> 60% (1.2s)
        target = 26 + ((elapsed - 0.6) / 1.2) * 34;
      } else if (elapsed < 3.2) {
        // 60% -> 84% (1.4s)
        target = 60 + ((elapsed - 1.8) / 1.4) * 24;
      } else if (elapsed < 5.0) {
        // 84% -> 94% (1.8s)
        target = 84 + ((elapsed - 3.2) / 1.8) * 10;
      } else {
        // Tăng dần chậm rãi tối đa 98%, chờ tín hiệu phản hồi hoàn tất từ AI
        target = Math.min(98, 94 + (1 - Math.exp(-(elapsed - 5.0) / 4.0)) * 3.8);
      }

      if (target > promptProgressPercent) {
        promptProgressPercent += Math.max(0.4, (target - promptProgressPercent) * 0.28);
      }
      setPromptProgress(promptProgressPercent);

      const subtitleEl = byId('generating-subtitle');
      if (subtitleEl) {
        if (promptProgressPercent < 30) {
          subtitleEl.textContent = 'Đang gửi prompt và phân tích kịch bản...';
        } else if (promptProgressPercent < 65) {
          subtitleEl.textContent = 'AI đang cấu trúc câu và nội dung livestream...';
        } else if (promptProgressPercent < 88) {
          subtitleEl.textContent = 'Đang chuyển đổi giọng đọc và nạp hàng đợi phát...';
        } else {
          subtitleEl.textContent = 'Chuẩn bị hoàn tất, sắp bắt đầu phát sóng...';
        }
      }
    }, 70);
  }

  function finishPromptGeneratingIndicator() {
    if (promptProgressTimer) {
      clearInterval(promptProgressTimer);
      promptProgressTimer = null;
    }

    const indicator = byId('prompt-generating-indicator');
    if (!indicator || indicator.dataset.finishing === 'true') return;
    indicator.dataset.finishing = 'true';

    setPromptProgress(100);
    indicator.classList.add('complete');

    const titleEl = byId('generating-title');
    if (titleEl) {
      titleEl.innerHTML = '<span style="color:#10b981; font-weight:800;">✓ Đã sinh nội dung Live thành công! (100%)</span>';
    }
    const subtitleEl = byId('generating-subtitle');
    if (subtitleEl) {
      subtitleEl.textContent = 'Nội dung đã hoàn tất 100% và đưa vào hàng đợi phát sóng.';
    }

    setTimeout(() => {
      indicator.style.transition = 'opacity 0.38s ease, transform 0.38s ease, max-height 0.38s ease, margin 0.38s ease, padding 0.38s ease';
      indicator.style.opacity = '0';
      indicator.style.transform = 'translateY(-6px)';
      indicator.style.maxHeight = '0px';
      indicator.style.margin = '0px';
      indicator.style.paddingTop = '0px';
      indicator.style.paddingBottom = '0px';
      indicator.style.overflow = 'hidden';

      setTimeout(() => {
        indicator.remove();
      }, 400);
    }, 750);
  }

  function hidePromptGeneratingIndicator() {
    if (promptProgressTimer) {
      clearInterval(promptProgressTimer);
      promptProgressTimer = null;
    }
    const indicator = byId('prompt-generating-indicator');
    if (indicator) indicator.remove();
  }

  function addChatFeedMessage(msg) {
    const container = byId('chat-messages');
    if (!container) return;
    container.querySelector('.script-empty-state')?.remove();
    const item = document.createElement('div');
    if (msg.role === 'assistant') {
      finishPromptGeneratingIndicator();
      item.className = 'llm-item speech-track';
      item.dataset.speechText = msg.text || '';
      item.textContent = msg.text || '';
    } else if (msg.role === 'user') {
      // Chỉ cập nhật hiển thị ở widget cột bên trái, không chèn vào feed câu Live
      updateActivePromptDisplay(msg.text || '');
      showPromptGeneratingIndicator();
      return;
    } else if (msg.role === 'optimized_prompt' || msg.role === 'refined_prompt') {
      // Chỉ cập nhật modal kịch bản ở cột bên trái
      updateOptimizedPromptDisplay(msg.text || '');
      return;
    } else if (msg.role === 'error') {
      hidePromptGeneratingIndicator();
      item.className = 'llm-error';
      item.textContent = msg.text || 'Không thể kết nối LLM.';
    } else {
      item.className = 'msg-item system';
      item.textContent = msg.text || '';
    }

    const indicator = byId('prompt-generating-indicator');
    if (indicator && indicator.parentNode === container) {
      container.insertBefore(item, indicator);
    } else {
      container.appendChild(item);
    }

    trimFeed(container);
    const active = container.querySelector('.speech-track.active');
    if (active) {
      chatScrollController ? chatScrollController.scrollToElement(active) : keepSpeechHighlightVisible(active);
    } else {
      chatScrollController ? chatScrollController.scrollToBottom() : (container.scrollTop = container.scrollHeight);
    }
  }

  function removeCommentCardWithAnimation(card) {
    if (!card || card.dataset.removing === 'true') return;
    card.dataset.removing = 'true';
    card.classList.remove('active');
    card.classList.add('speech-complete');

    // Smooth visual exit
    card.style.transition = 'opacity .42s cubic-bezier(0.4, 0, 0.2, 1), transform .42s cubic-bezier(0.4, 0, 0.2, 1), max-height .42s cubic-bezier(0.4, 0, 0.2, 1), margin .42s cubic-bezier(0.4, 0, 0.2, 1), padding .42s cubic-bezier(0.4, 0, 0.2, 1)';
    card.style.opacity = '0';
    card.style.transform = 'translateX(28px) scale(0.96)';
    card.style.pointerEvents = 'none';

    const currentHeight = card.offsetHeight;
    if (currentHeight > 0) {
      card.style.maxHeight = `${currentHeight}px`;
    }
    requestAnimationFrame(() => {
      card.style.maxHeight = '0px';
      card.style.marginTop = '0px';
      card.style.marginBottom = '0px';
      card.style.paddingTop = '0px';
      card.style.paddingBottom = '0px';
      card.style.borderWidth = '0px';
      card.style.overflow = 'hidden';
    });

    setTimeout(() => {
      card.remove();
      eventScrollController?.updateButtonState();
      const container = byId('event-messages');
      if (container) {
        const remaining = container.querySelectorAll('.comment-item');
        if (remaining.length === 0 && !container.querySelector('.comments-empty-state')) {
          const emptyDiv = document.createElement('div');
          emptyDiv.className = 'comments-empty-state';
          emptyDiv.textContent = 'Đang chờ bình luận TikTok và phản hồi từ AI...';
          container.appendChild(emptyDiv);
        }
      }
    }, 450);
  }

  function addEventFeedMessage(msg) {
    const container = byId('event-messages');
    if (!container) return;
    container.querySelector('.comments-empty-state')?.remove();
    if (msg.role === 'assistant') {
      const cards = [...container.querySelectorAll('.comment-item')];
      let card = cards.reverse().find((candidate) => !candidate.querySelector('.comment-ai-reply') && !candidate.dataset.removing);
      if (!card) {
        card = document.createElement('div');
        card.className = 'comment-item';
        card.innerHTML = '<div class="comment-viewer-info"><span class="viewer-name">TikTok Live</span><span>Phản hồi mới</span></div>';
        container.appendChild(card);
      }
      const reply = document.createElement('div');
      reply.className = 'comment-ai-reply speech-track';
      reply.dataset.speechText = msg.text || '';
      reply.textContent = msg.text || '';
      card.appendChild(reply);
      // Giữ nguyên bình luận trong danh sách, tuyệt đối không xóa mất
    } else {
      const raw = String(msg.text || '');
      const separator = raw.indexOf(':');
      const viewer = separator > 0 ? raw.slice(0, separator).trim() : 'Người xem TikTok';
      const content = separator > 0 ? raw.slice(separator + 1).trim() : raw;
      const card = document.createElement('div');
      card.className = 'comment-item';

      const info = document.createElement('div');
      info.className = 'comment-viewer-info';

      const name = document.createElement('span');
      name.className = 'viewer-name';
      name.textContent = viewer;

      const action = document.createElement('span');
      action.textContent = 'vừa tương tác';
      info.append(name, action);

      const body = document.createElement('div');
      body.className = 'comment-content';
      body.textContent = content;

      card.append(info, body);
      container.appendChild(card);
    }
    trimFeed(container, 500);

    const active = container.querySelector('.speech-track.active, .comment-item.active');
    if (active) {
      eventScrollController ? eventScrollController.scrollToElement(active) : keepSpeechHighlightVisible(active);
    } else {
      eventScrollController ? eventScrollController.scrollToBottom() : (container.scrollTop = container.scrollHeight);
    }
  }

  const normalizeSpeechText = (text) => String(text || '').normalize('NFC').toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, ' ').trim();

  function calculateWordOverlap(textA, textB) {
    const normA = normalizeSpeechText(textA);
    const normB = normalizeSpeechText(textB);
    if (!normA || !normB) return 0;
    const wordsA = normA.split(/\s+/).filter((w) => w.length > 1);
    const wordsB = normB.split(/\s+/).filter((w) => w.length > 1);
    if (!wordsA.length || !wordsB.length) return 0;
    const setB = new Set(wordsB);
    let common = 0;
    for (const word of wordsA) {
      if (setB.has(word)) common++;
    }
    return common / Math.max(wordsA.length, wordsB.length);
  }

  function findSpeechElement(text, source = '') {
    const wanted = normalizeSpeechText(text);
    if (!wanted && source !== 'tiktok') return null;

    const root = source === 'tiktok'
      ? byId('event-messages')
      : source === 'live' ? byId('chat-messages') : document;
    const candidates = [...(root || document).querySelectorAll('.speech-track')].filter(
      (el) => !el.classList.contains('read') && !el.closest('.comment-item')?.classList.contains('read')
    );

    if (wanted) {
      // 1. Exact match
      const exact = candidates.find((el) => normalizeSpeechText(el.dataset.speechText) === wanted);
      if (exact) return exact;

      // 2. Substring inclusion
      const substr = candidates.find((el) => {
        const c = normalizeSpeechText(el.dataset.speechText);
        return c.includes(wanted) || wanted.includes(c);
      });
      if (substr) return substr;

      // 3. Word overlap (fuzzy match >= 35%)
      let bestCandidate = null;
      let bestScore = 0;
      for (const el of candidates) {
        const score = calculateWordOverlap(wanted, el.dataset.speechText);
        if (score > bestScore) {
          bestScore = score;
          bestCandidate = el;
        }
      }
      if (bestCandidate && bestScore >= 0.35) {
        return bestCandidate;
      }
    }

    // 4. FIFO fallback for TikTok comments
    if (source === 'tiktok') {
      const eventContainer = byId('event-messages');
      if (eventContainer) {
        const fifoCandidate = eventContainer.querySelector(
          '.comment-item:not(.read) .speech-track'
        );
        if (fifoCandidate) return fifoCandidate;
      }
    }

    return null;
  }

  function speechVisualTarget(element) {
    return element?.closest('.comment-item') || element;
  }

  function keepSpeechHighlightVisible(target, force = false) {
    const container = target?.closest('.log-stream');
    if (!container) return;
    const containerRect = container.getBoundingClientRect();
    const targetRect = target.getBoundingClientRect();
    const fullyVisible = !force && (targetRect.top >= containerRect.top + 20 && targetRect.bottom <= containerRect.bottom - 20);
    if (fullyVisible) return;
    const centeredTop = container.scrollTop
      + (targetRect.top - containerRect.top)
      - Math.max(0, (container.clientHeight - targetRect.height) / 2);
    container.scrollTo({ top: Math.max(0, centeredTop), behavior: 'smooth' });
  }

  function activateSpeech(data) {
    const element = findSpeechElement(data.text, data.source);
    const target = speechVisualTarget(element);
    const sequence = Number(data.sequence) || 0;
    const isNewSpeech = sequence !== state.activeSpeechSequence || target !== state.activeSpeechElement;
    if (state.activeSpeechElement && state.activeSpeechElement !== target) {
      state.activeSpeechElement.classList.remove('active');
      state.activeSpeechElement.classList.add('read');
      state.activeSpeechElement.querySelector?.('.comment-ai-reply')?.classList.remove('active');
      state.activeSpeechElement.querySelector?.('.comment-ai-reply')?.classList.add('read');
    }
    state.activeSpeechElement = target;
    state.activeSpeechSequence = sequence;
    state.activeSpeechSource = data.source || '';
    target?.classList.remove('read');
    target?.classList.add('active');
    if (element && element !== target) {
      element.classList.remove('read');
      element.classList.add('active');
    }
    // Căn giữa câu đang nói: nếu người dùng đang chủ động cuộn xem lịch sử thì không ép cuộn
    const streamContainer = target?.closest('.log-stream');
    if (streamContainer?.id === 'event-messages' && eventScrollController) {
      eventScrollController.onSpeechActivated(target);
    } else if (streamContainer?.id === 'chat-messages' && chatScrollController) {
      chatScrollController.onSpeechActivated(target);
    } else {
      keepSpeechHighlightVisible(target, isNewSpeech);
    }
    byId('active-sentence-card')?.classList.add('speaking');
    byId('speech-wave')?.classList.add('speaking');
    setText('reading-state', 'ĐANG PHÁT');
    setText('reading-text', data.text || 'Đang phát âm thanh...');
    setText('reading-num', String(data.sequence || 0).padStart(2, '0'));
    const duration = Math.max(.1, Number(data.duration) || .1);
    const elapsed = Math.max(0, Date.now() / 1000 - (Number(data.startedAt) || Date.now() / 1000));
    const percent = Math.max(0, Math.min(99, Math.round(elapsed * 100 / duration)));
    if (byId('reading-progress-bar')) byId('reading-progress-bar').style.width = `${percent}%`;
    setText('reading-progress-percent', `${percent}%`);
  }

  function completeSpeech(data) {
    const sequence = Number(data.completedSequence) || 0;
    if (!sequence || sequence <= state.completedSpeechSequence) return;
    state.completedSpeechSequence = sequence;

    let target = null;
    if (state.activeSpeechSequence === sequence && state.activeSpeechElement) {
      target = state.activeSpeechElement;
    } else {
      const found = findSpeechElement(data.completedText, data.completedSource);
      target = speechVisualTarget(found);
    }

    // Special fallback for TikTok comment completion
    const isTikTok = data.completedSource === 'tiktok' || state.activeSpeechSource === 'tiktok';
    if (!target && isTikTok) {
      const container = byId('event-messages');
      target = container?.querySelector('.comment-item.active')
        || container?.querySelector('.comment-item:not(.read)');
    }

    if (target) {
      target.classList.remove('active');
      target.classList.add('read');
      const card = target.closest?.('.comment-item') || (target.classList?.contains('comment-item') ? target : null);
      if (card) {
        card.classList.remove('active');
        card.classList.add('read');
        card.querySelectorAll('.speech-track, .comment-ai-reply').forEach((el) => {
          el.classList.remove('active');
          el.classList.add('read');
        });
      }
    } else if (isTikTok) {
      const lingeringComment = byId('event-messages')?.querySelector('.comment-item.active');
      if (lingeringComment) {
        lingeringComment.classList.remove('active');
        lingeringComment.classList.add('read');
      }
    }

    state.activeSpeechElement = null;
    state.activeSpeechSource = '';
    byId('active-sentence-card')?.classList.remove('speaking');
    byId('speech-wave')?.classList.remove('speaking');
    if (byId('reading-progress-bar')) byId('reading-progress-bar').style.width = '100%';
    setText('reading-progress-percent', '100%');
    setText('reading-state', 'ĐÃ ĐỌC XONG');

    eventScrollController?.updateButtonState();
    chatScrollController?.updateButtonState();
  }


  async function pollSpeechStatus() {
    const sessionId = byId('sessionid')?.value;
    if (!sessionId || sessionId === '0') return;
    try {
      const payload = await api('/speech_status', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ sessionid: String(sessionId) }),
      });
      const data = payload.data || {};
      completeSpeech(data);
      if (data.status === 'speaking' && data.text) {
        activateSpeech(data);
      }
    } catch (_) {
      // Bỏ qua lỗi tạm thời khi avatar đang khởi động hoặc đổi mẫu
    }
  }

  async function pollMessages(endpoint, containerId, formatter) {
    const sessionId = byId('sessionid').value;
    if (!sessionId || sessionId === '0') return;
    try {
      const payload = await api(endpoint, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ sessionid: String(sessionId) }),
      });
      (payload.data || []).forEach((msg) => formatter(msg, containerId));
    } catch (_) { }
  }

  function startPollingLogs() {
    stopPollingLogs();
    const pollChat = () => pollMessages('/chat_messages', 'chat-messages', addChatFeedMessage);
    const pollEvents = () => pollMessages('/event_messages', 'event-messages', addEventFeedMessage);
    state.chatPoll = setInterval(pollChat, 1200);
    state.eventPoll = setInterval(pollEvents, 1200);
    state.speechPoll = setInterval(pollSpeechStatus, 450);
    pollChat();
    pollEvents();
    pollSpeechStatus();
  }

  function stopPollingLogs() {
    clearInterval(state.chatPoll);
    clearInterval(state.eventPoll);
    clearInterval(state.speechPoll);
    state.chatPoll = null;
    state.eventPoll = null;
    state.speechPoll = null;
    state.activeSpeechElement?.classList.remove('active');
    state.activeSpeechElement = null;
    state.activeSpeechSequence = 0;
    byId('active-sentence-card')?.classList.remove('speaking');
    byId('speech-wave')?.classList.remove('speaking');
  }

  // Check balance before starting
  function requireActiveBalance() {
    if (state.remaining <= 0) {
      showAppAlert('Hết số dư Live VIP', 'Tài khoản Live VIP của bạn đã hết thời gian sử dụng (00:00:00). Vui lòng nạp xu để bắt đầu.');
      openPriceModal();
      return false;
    }
    return true;
  }

  // Heartbeat & Billing Session
  async function heartbeat() {
    try {
      const payload = await api('/api/billing/session/heartbeat', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: '{}',
      });
      state.heartbeatFailures = 0;
      state.elapsed = Number(payload.sessionElapsedSeconds) || state.elapsed;
      state.remaining = getVipBalance(payload);
      updateUiDisplays();

      if (payload.action === 'stop_live' || payload.status === 'expired' || state.remaining <= 0) {
        await stopLive(false);
        openExpiredModal();
      }
    } catch (error) {
      if (error.isForcedLogout) return;

      // Stop immediately if unauthorized or forbidden
      if (error.status === 401 || error.status === 403) {
        await stopLive(false);
        return;
      }

      state.heartbeatFailures = (state.heartbeatFailures || 0) + 1;
      console.warn(`[Heartbeat] Lỗi kiểm tra phiên tạm thời (lần ${state.heartbeatFailures}/5):`, error.message);

      // Only terminate session if failed 5 consecutive times (about 50s disconnect)
      if (state.heartbeatFailures >= 5) {
        await stopLive(false);
        showAppAlert('Lỗi kiểm tra phiên', 'Mất kết nối với máy chủ quản lý phiên sau nhiều lần thử lại. Vui lòng kiểm tra đường truyền mạng.');
      }
    }
  }

  async function startLive() {
    const btnStart = byId('start');
    btnStart.disabled = true;
    try {
      if (!requireResources()) return;
      if (!requireActiveBalance()) {
        return;
      }

      // Automatically restore or recreate Avatar Live window if closed
      await ensureAvatarWindow();

      if (state.profile?.activeSession?.status === 'active' && state.profile.activeSession.serviceType === SERVICE_TYPE) {
        await api('/api/billing/session/end', {
          method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}',
        });
      }

      const rawTiktok = byId('tiktok-username')?.value || '';
      const cleanTiktok = parseTikTokUsername(rawTiktok);

      const payload = await api('/api/billing/session/start', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          serviceType: SERVICE_TYPE,
          deviceId: 'live-vip-desktop',
          appVersion: '1.0.2',
          tiktokChannel: cleanTiktok || undefined,
          channel: cleanTiktok || undefined
        }),
      });

      state.elapsed = Number(payload.sessionElapsedSeconds) || 0;
      state.remaining = getVipBalance(payload);
      state.startedAt = payload.startedAt || new Date().toISOString();
      state.heartbeatFailures = 0;
      state.running = true;

      setWebRtcConnected(false, 'Đang kết nối...');
      byId('start').classList.add('hidden');
      byId('stop').classList.remove('hidden');
      byId('avatar-picker-btn')?.classList.add('disabled-live');
      byId('avatar-picker-dropdown')?.classList.add('hidden');
      byId('avatar-hover-card')?.classList.add('hidden');

      updateUiDisplays();
      startLocalTimer();
      startPollingLogs();

      const hbInterval = Math.max(5, Number(payload.heartbeatIntervalSeconds) || 10);
      state.billingTimer = setInterval(heartbeat, hbInterval * 1000);

      try {
        await startAvatarWindowStream();
      } catch (webrtcErr) {
        await stopLive(true);
        throw webrtcErr;
      }

      // TỰ ĐỘNG GỌI LLM PHÁT KỊCH BẢN BÁN HÀNG NGAY KHI BẮT ĐẦU PHIÊN LIVE
      const scriptToSpeak = (state.activeOptimizedPrompt || state.activePromptTopic || byId('chat-message')?.value || '').trim();
      if (scriptToSpeak) {
        try {
          const liveSessionId = String(byId('sessionid')?.value || '').trim();
          updateActivePromptDisplay(state.activePromptTopic || scriptToSpeak.slice(0, 100));
          showPromptGeneratingIndicator();
          await api('/human', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
              text: scriptToSpeak,
              type: 'chat',
              interrupt: true,
              continuous: true,
              tts: { ref_file: state.speechVoice, speed: state.speechSpeed },
              sessionid: liveSessionId
            }),
          });
        } catch (promptErr) {
          console.warn('Auto start prompt speech failed:', promptErr);
        }
      }
    } catch (err) {
      if (err.isForcedLogout) return;
      if (!byId('price-modal').classList.contains('open')) showAppAlert('Không thể bắt đầu', err.message);
    } finally {
      btnStart.disabled = false;
    }
  }

  async function stopLive(callApi = true) {
    stopTimers();
    stopPollingLogs();
    hidePromptGeneratingIndicator();
    state.running = false;

    const liveSessionId = byId('sessionid')?.value;
    if (liveSessionId && liveSessionId !== '0') {
      try {
        await api('/interrupt_talk', {
          method: 'POST', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ sessionid: String(liveSessionId) }),
        });
      } catch (_) { }
    }
    try { sendAvatarCommand('stop'); } catch (_) { }

    if (callApi) {
      try {
        await api('/api/billing/session/end', {
          method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}',
        });
      } catch (_) { }
    }

    if (state.tiktokConnected) {
      api('/tiktok_disconnect', { method: 'POST' }).catch(() => { });
      api('/api/billing/tiktok-channel', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ action: 'disconnect' })
      }).catch(() => { });
      setTikTokButtonState(false);
    }

    byId('stop').classList.add('hidden');
    byId('start').classList.remove('hidden');
    byId('avatar-picker-btn')?.classList.remove('disabled-live');
    byId('sessionid').value = '0';
    setWebRtcConnected(false, 'Offline');

    state.elapsed = 0;
    state.startedAt = null;
    await loadProfile().catch(() => updateUiDisplays());
  }

  // ─── Custom Avatar Picker & Hover Snapshot Preview ─────────────────────────
  let currentAvatarsList = [];
  let currentCamerasMap = {};
  let avatarPickerInitialized = false;
  let cameraControlsInitialized = false;

  function updateCameraMonitorState(avatarId) {
    const isCam = avatarId && (avatarId.toLowerCase().startsWith('webcam') || avatarId.toLowerCase() === 'camera' || avatarId.toLowerCase() === 'livecam');
    const panel = byId('camera-monitor-panel');
    const streamImg = byId('raw-camera-stream');
    const overlay = byId('camera-stream-loading');
    const activeName = byId('cam-active-name');

    if (!panel) return;

    if (isCam) {
      panel.classList.remove('hidden');
      if (overlay) {
        overlay.classList.remove('hidden');
        const text = overlay.querySelector('.camera-loading-text');
        if (text) text.textContent = 'Đang kết nối Camera trực tiếp...';
      }
      if (streamImg) {
        streamImg.onload = () => {
          if (overlay) overlay.classList.add('hidden');
        };
        streamImg.onerror = () => {
          if (overlay) {
            overlay.classList.remove('hidden');
            const text = overlay.querySelector('.camera-loading-text');
            if (text) text.textContent = 'Chưa nhận tín hiệu Camera. Bấm "Làm mới Cam" nếu cần.';
          }
        };
        streamImg.src = `/api/camera/stream?t=${Date.now()}`;
      }
      if (activeName) {
        activeName.textContent = 'Webcam thời gian thực (Cam gốc)';
      }
    } else {
      panel.classList.add('hidden');
      if (streamImg) {
        streamImg.src = '';
      }
      fetch('/api/camera/release', { method: 'POST' }).catch(() => { });
    }
  }

  function setupCameraMonitorControls() {
    if (cameraControlsInitialized) return;
    cameraControlsInitialized = true;

    byId('btn-refresh-camera')?.addEventListener('click', (e) => {
      e.stopPropagation();
      const streamImg = byId('raw-camera-stream');
      const overlay = byId('camera-stream-loading');
      if (overlay) {
        overlay.classList.remove('hidden');
        const text = overlay.querySelector('.camera-loading-text');
        if (text) text.textContent = 'Đang làm mới kết nối Camera...';
      }
      if (streamImg) {
        streamImg.src = '';
        setTimeout(() => {
          streamImg.src = `/api/camera/stream?t=${Date.now()}`;
        }, 300);
      }
    });

    const toggleBtn = byId('btn-toggle-cam');
    const panel = byId('camera-monitor-panel');
    const header = byId('cam-panel-header');

    const togglePanel = (e) => {
      if (!panel) return;
      const isCollapsed = panel.classList.toggle('collapsed');
      if (toggleBtn) {
        toggleBtn.textContent = isCollapsed ? '▼' : '▲';
        toggleBtn.title = isCollapsed ? 'Mở rộng Camera' : 'Thu nhỏ Camera';
      }
    };

    toggleBtn?.addEventListener('click', (e) => {
      e.stopPropagation();
      togglePanel(e);
    });

    header?.addEventListener('click', (e) => {
      if (e.target.closest('#btn-refresh-camera') || e.target.closest('#btn-toggle-cam')) return;
      togglePanel(e);
    });
  }

  function getAvatarDisplayName(avatarId) {
    if (!avatarId) return '';
    const lower = avatarId.toLowerCase();
    if (lower === 'webcam' || lower === 'camera' || lower.startsWith('webcam')) {
      return '📷 Webcam Trực tiếp';
    }
    return avatarId;
  }

  async function selectAvatarById(avatarId, notifyBackend = true) {
    if (state.running) {
      showAppAlert('Đang trong phiên Live', 'Vui lòng DỪNG phiên Live trước khi đổi Mẫu Live để tránh làm méo hình ảnh.');
      return;
    }
    state.currentAvatar = avatarId;
    const select = byId('select-avatar');
    if (select) select.value = avatarId;

    const currentThumb = byId('avatar-current-thumb');
    const currentLabel = byId('avatar-current-label');
    if (currentThumb) {
      currentThumb.src = `/avatar_preview/${encodeURIComponent(avatarId)}`;
      currentThumb.style.display = 'block';
    }
    if (currentLabel) {
      currentLabel.textContent = getAvatarDisplayName(avatarId);
    }
    const carouselSelectedName = byId('avatar-carousel-selected-name');
    if (carouselSelectedName) {
      carouselSelectedName.textContent = getAvatarDisplayName(avatarId);
    }

    localStorage.setItem('slive-vip-selected-avatar', avatarId);

    document.querySelectorAll('.avatar-option-item').forEach((item) => {
      item.classList.toggle('active', item.dataset.avatarId === avatarId);
    });
    document.querySelectorAll('.avatar-carousel-card').forEach((card) => {
      const isActive = card.dataset.avatarId === avatarId;
      card.classList.toggle('active', isActive);
      if (isActive) {
        card.scrollIntoView({ behavior: 'smooth', block: 'nearest', inline: 'center' });
      }
    });

    setupCameraMonitorControls();
    updateCameraMonitorState(avatarId);

    // Cập nhật video mẫu live cho tab xem video ở giữa
    updateCenterAvatarPreview(avatarId);

    if (notifyBackend) {
      // Khi người dùng click chọn mẫu live, tự động chuyển sang tab xem video to ra hết phần giữa
      window.switchCenterTab('avatar-preview');

      try {
        const sessionId = String(byId('sessionid')?.value || '0');
        await api('/select_avatar', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ avatar_id: avatarId, sessionid: sessionId }),
        });
      } catch (err) {
        console.error('Error selecting avatar:', err);
      }
    }
  }

  // Cập nhật tab xem Video Mẫu Live toàn phần giữa
  function updateCenterAvatarPreview(avatarId) {
    const previewVideo = byId('center-avatar-preview-video');
    const webcamNotice = byId('center-preview-webcam-notice');
    const previewNameBadge = byId('preview-tab-avatar-name');
    const previewLabelName = byId('center-preview-label-name');
    const displayName = getAvatarDisplayName(avatarId);

    if (previewNameBadge) {
      const cleanName = displayName.replace(/^(Mẫu|Avatar)\s*/i, '').trim().toUpperCase();
      previewNameBadge.textContent = cleanName || 'MẪU';
    }
    if (previewLabelName) {
      previewLabelName.textContent = `MẪU LIVE: ${displayName}`;
    }

    const isWebcam = !avatarId || avatarId.toLowerCase().startsWith('webcam') || avatarId.toLowerCase() === 'camera';
    const fallbackImg = byId('center-avatar-preview-fallback-img');

    if (isWebcam) {
      if (previewVideo) {
        previewVideo.pause();
        previewVideo.style.display = 'none';
        previewVideo.removeAttribute('src');
      }
      if (fallbackImg) fallbackImg.style.display = 'none';
      if (webcamNotice) webcamNotice.classList.remove('hidden');
    } else {
      if (webcamNotice) webcamNotice.classList.add('hidden');
      const previewImgSrc = `/avatar_preview/${encodeURIComponent(avatarId)}`;

      if (fallbackImg) {
        fallbackImg.src = previewImgSrc;
        fallbackImg.style.display = 'block';
        fallbackImg.style.opacity = '1';
      }

      if (previewVideo) {
        previewVideo.style.display = 'block';
        previewVideo.poster = previewImgSrc;

        const targetSrc = `/avatar_video/${encodeURIComponent(avatarId)}`;
        if (previewVideo.getAttribute('src') !== targetSrc) {
          previewVideo.src = targetSrc;
          previewVideo.load();
        }

        previewVideo.onplaying = () => {
          if (fallbackImg) fallbackImg.style.opacity = '0';
        };
        previewVideo.onerror = () => {
          if (fallbackImg) {
            fallbackImg.style.display = 'block';
            fallbackImg.style.opacity = '1';
          }
        };

        previewVideo.play().catch(() => { });
      }
    }
  }

  // Chuyển đổi tab ở cột giữa: Điều hành phiên Live <-> Xem Video Mẫu Live
  window.switchCenterTab = function (tabName) {
    const btnControl = byId('tab-btn-live-control');
    const btnPreview = byId('tab-btn-avatar-preview');
    const contentControl = byId('center-tab-content-control');
    const contentPreview = byId('center-tab-content-preview');
    const videoEl = byId('center-avatar-preview-video');

    if (tabName === 'avatar-preview') {
      btnPreview?.classList.add('active');
      btnControl?.classList.remove('active');
      contentControl?.classList.add('hidden');
      contentPreview?.classList.remove('hidden');
      if (videoEl && videoEl.style.display !== 'none' && videoEl.src) {
        videoEl.play().catch(() => { });
      }
    } else {
      btnControl?.classList.add('active');
      btnPreview?.classList.remove('active');
      contentControl?.classList.remove('hidden');
      contentPreview?.classList.add('hidden');
    }
  };

  // Bật/tắt âm thanh video preview
  window.togglePreviewVideoAudio = function () {
    const videoEl = byId('center-avatar-preview-video');
    const textEl = byId('preview-audio-text');
    const iconEl = byId('preview-audio-icon');
    if (!videoEl) return;
    videoEl.muted = !videoEl.muted;
    if (textEl) {
      textEl.textContent = videoEl.muted ? 'Âm thanh: Tắt' : 'Âm thanh: Bật';
    }
    if (iconEl) {
      if (videoEl.muted) {
        iconEl.innerHTML = '<polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><line x1="23" y1="9" x2="17" y2="15"/><line x1="17" y1="9" x2="23" y2="15"/>';
      } else {
        iconEl.innerHTML = '<polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><path d="M19.07 4.93a10 10 0 0 1 0 14.14M15.54 8.46a5 5 0 0 1 0 7.07"/>';
      }
    }
  };

  // Phát lại video preview từ đầu
  window.replayAvatarPreviewVideo = function () {
    const videoEl = byId('center-avatar-preview-video');
    if (videoEl) {
      videoEl.currentTime = 0;
      videoEl.play().catch(() => { });
    }
  };

  function setupCustomAvatarPicker() {
    if (avatarPickerInitialized) return;
    avatarPickerInitialized = true;

    const pickerBtn = byId('avatar-picker-btn');
    const dropdown = byId('avatar-picker-dropdown');
    const hoverCard = byId('avatar-hover-card');

    pickerBtn?.addEventListener('click', (e) => {
      e.stopPropagation();
      if (state.running) {
        showAppAlert('Đang trong phiên Live', 'Vui lòng DỪNG phiên Live trước khi đổi Mẫu Live để tránh làm méo hình ảnh.');
        return;
      }
      dropdown?.classList.toggle('hidden');
      if (dropdown?.classList.contains('hidden')) {
        hoverCard?.classList.add('hidden');
      }
    });

    document.addEventListener('click', (e) => {
      if (!e.target.closest('#custom-avatar-picker')) {
        dropdown?.classList.add('hidden');
        hoverCard?.classList.add('hidden');
      }
    });

    dropdown?.addEventListener('mouseleave', () => {
      hoverCard?.classList.add('hidden');
    });
  }

  // Avatar Management Modal & Dropdown
  async function loadAvatars() {
    setupCustomAvatarPicker();
    try {
      const response = await fetch('/avatars_list', {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}',
      });
      const payload = await response.json();
      const rawList = payload.data || [];
      // Đưa Webcam / Camera xuống CUỐI CÙNG danh sách mẫu live theo yêu cầu
      const nonCam = rawList.filter((a) => !String(a).toLowerCase().startsWith('webcam') && String(a).toLowerCase() !== 'camera');
      const camItems = rawList.filter((a) => String(a).toLowerCase().startsWith('webcam') || String(a).toLowerCase() === 'camera');
      const list = [...nonCam, ...camItems];
      const cameras = payload.cameras || [];
      currentAvatarsList = list;
      currentCamerasMap = {};
      if (Array.isArray(cameras)) {
        cameras.forEach((c) => {
          if (c && c.id) currentCamerasMap[c.id] = c;
        });
      }

      const select = byId('select-avatar');
      const optionsContainer = byId('avatar-options-list');
      if (select) select.innerHTML = '';
      if (optionsContainer) optionsContainer.innerHTML = '';

      const saved = localStorage.getItem('slive-vip-selected-avatar') || (list.includes('mau10') ? 'mau10' : (list.includes('mau1') ? 'mau1' : (nonCam[0] || list[0] || 'mau10')));

      const hoverCard = byId('avatar-hover-card');
      const hoverImg = byId('avatar-hover-img');
      const hoverTitle = byId('avatar-hover-title');
      const hoverSub = byId('avatar-hover-sub');

      list.forEach((avatar) => {
        const displayName = getAvatarDisplayName(avatar);
        const isCam = avatar.toLowerCase().startsWith('webcam') || avatar.toLowerCase() === 'camera';

        if (select) {
          const opt = document.createElement('option');
          opt.value = avatar;
          opt.textContent = displayName;
          if (avatar === saved) opt.selected = true;
          select.appendChild(opt);
        }

        if (optionsContainer) {
          const item = document.createElement('div');
          item.className = `avatar-option-item ${avatar === saved ? 'active' : ''}`.trim();
          item.dataset.avatarId = avatar;

          const thumb = document.createElement('img');
          thumb.className = 'avatar-option-thumb';
          thumb.src = `/avatar_preview/${encodeURIComponent(avatar)}`;
          thumb.alt = avatar;
          thumb.onerror = () => { thumb.style.display = 'none'; };

          const name = document.createElement('span');
          name.textContent = displayName;

          item.append(thumb, name);

          item.addEventListener('mouseenter', () => {
            if (!hoverCard) return;
            hoverImg.src = `/avatar_preview/${encodeURIComponent(avatar)}`;
            if (isCam) {
              hoverTitle.textContent = '📷 Webcam Trực tiếp';
              hoverSub.textContent = 'Tự động kết nối camera khả dụng';
            } else {
              hoverTitle.textContent = avatar;
              hoverSub.textContent = 'Ảnh snapshot giây đầu tiên';
            }

            const dropdownEl = byId('avatar-picker-dropdown');
            const anchorRect = dropdownEl ? dropdownEl.getBoundingClientRect() : item.getBoundingClientRect();

            hoverCard.classList.remove('hidden');

            const cardHeight = hoverCard.offsetHeight || 260;
            const cardWidth = hoverCard.offsetWidth || 160;

            // Neo cố định theo mép trên của dropdown: không bao giờ bị chạy tuột xuống dưới khi hover các mục cuối
            let topPos = anchorRect.top;
            const maxTop = window.innerHeight - cardHeight - 16;
            if (topPos > maxTop) {
              topPos = Math.max(12, maxTop);
            }
            if (topPos < 12) topPos = 12;

            let leftPos = anchorRect.right + 10;
            if (leftPos + cardWidth > window.innerWidth - 10) {
              leftPos = Math.max(10, anchorRect.left - cardWidth - 10);
            }

            hoverCard.style.top = `${topPos}px`;
            hoverCard.style.left = `${leftPos}px`;
          });

          item.addEventListener('click', () => {
            hoverCard?.classList.add('hidden');
            byId('avatar-picker-dropdown')?.classList.add('hidden');
            selectAvatarById(avatar, true);
          });

          optionsContainer.appendChild(item);
        }
      });

      // Populate Avatar Horizontal Carousel Slider
      const carouselTrack = byId('avatar-carousel-track');
      if (carouselTrack) {
        carouselTrack.innerHTML = '';
        list.forEach((avatar) => {
          const card = document.createElement('div');
          card.className = `avatar-carousel-card ${avatar === saved ? 'active' : ''}`.trim();
          card.dataset.avatarId = avatar;
          card.title = getAvatarDisplayName(avatar);

          const thumb = document.createElement('img');
          thumb.className = 'avatar-carousel-thumb';
          thumb.src = `/avatar_preview/${encodeURIComponent(avatar)}`;
          thumb.alt = avatar;
          thumb.onerror = () => { thumb.style.display = 'none'; };

          const label = document.createElement('div');
          label.className = 'avatar-carousel-label';
          label.textContent = getAvatarDisplayName(avatar).replace(/^(Mẫu|Avatar)\s*/i, '');

          card.append(thumb, label);
          card.addEventListener('mouseenter', () => {
            const hoverCard = byId('avatar-hover-card');
            const hoverImg = byId('avatar-hover-img');
            const hoverTitle = byId('avatar-hover-title');
            const hoverSub = byId('avatar-hover-sub');
            if (!hoverCard || !hoverImg) return;

            hoverImg.src = `/avatar_preview/${encodeURIComponent(avatar)}`;
            if (hoverTitle) hoverTitle.textContent = getAvatarDisplayName(avatar);
            if (hoverSub) hoverSub.textContent = 'Khung hình mẫu AI Live VIP';

            hoverCard.classList.remove('hidden');

            const rect = card.getBoundingClientRect();
            const cardHeight = hoverCard.offsetHeight || 300;
            const cardWidth = hoverCard.offsetWidth || 220;

            let topPos = rect.bottom + 8;
            if (topPos + cardHeight > window.innerHeight - 10) {
              topPos = Math.max(10, rect.top - cardHeight - 8);
            }

            // Center hover preview directly under the hovered avatar thumbnail card
            let leftPos = rect.left + (rect.width / 2) - (cardWidth / 2);
            if (leftPos < 10) {
              leftPos = 10;
            } else if (leftPos + cardWidth > window.innerWidth - 10) {
              leftPos = window.innerWidth - cardWidth - 10;
            }

            hoverCard.style.top = `${topPos}px`;
            hoverCard.style.left = `${leftPos}px`;
          });

          card.addEventListener('mouseleave', () => {
            byId('avatar-hover-card')?.classList.add('hidden');
          });

          card.addEventListener('click', () => {
            if (state.running) {
              showAppAlert('Đang trong phiên Live', 'Vui lòng DỪNG phiên Live trước khi đổi Mẫu Live để tránh làm méo hình ảnh.');
              return;
            }
            selectAvatarById(avatar, true);
          });
          carouselTrack.appendChild(card);
        });

        carouselTrack.addEventListener('scroll', updateAvatarCarouselButtons);
        window.addEventListener('resize', updateAvatarCarouselButtons);
        setTimeout(updateAvatarCarouselButtons, 120);
      }

      if (saved) {
        selectAvatarById(saved, false);
      }
    } catch (_) { }
  }

  // Smart Avatar Carousel Navigation & State
  function updateAvatarCarouselButtons() {
    const track = byId('avatar-carousel-track');
    const prevBtn = byId('btn-avatar-prev');
    const nextBtn = byId('btn-avatar-next');
    if (!track || !prevBtn || !nextBtn) return;

    const maxScroll = track.scrollWidth - track.clientWidth;
    if (maxScroll <= 4) {
      prevBtn.style.opacity = '0';
      prevBtn.style.pointerEvents = 'none';
      nextBtn.style.opacity = '0';
      nextBtn.style.pointerEvents = 'none';
      return;
    }

    if (track.scrollLeft <= 5) {
      prevBtn.style.opacity = '0';
      prevBtn.style.pointerEvents = 'none';
    } else {
      prevBtn.style.opacity = '1';
      prevBtn.style.pointerEvents = 'auto';
    }

    if (track.scrollLeft >= maxScroll - 5) {
      nextBtn.style.opacity = '0';
      nextBtn.style.pointerEvents = 'none';
    } else {
      nextBtn.style.opacity = '1';
      nextBtn.style.pointerEvents = 'auto';
    }
  }

  // Profile Dropdown Menu Handlers
  window.scrollAvatarCarousel = function (direction) {
    const track = byId('avatar-carousel-track');
    if (!track) return;
    track.scrollBy({ left: 180 * direction, behavior: 'smooth' });
    setTimeout(updateAvatarCarouselButtons, 220);
  };

  window.openAvatarGuideModal = function () {
    const modal = byId('avatar-guide-modal');
    if (modal) {
      modal.style.display = 'flex';
      modal.classList.add('open');
    }
  };

  window.closeAvatarGuideModal = function () {
    const modal = byId('avatar-guide-modal');
    if (modal) {
      modal.classList.remove('open');
      modal.style.display = 'none';
    }
  };

  window.openVoiceGuideModal = function () {
    const modal = byId('voice-guide-modal');
    if (modal) {
      modal.style.display = 'flex';
      modal.classList.add('open');
    }
  };

  window.closeVoiceGuideModal = function () {
    const modal = byId('voice-guide-modal');
    if (modal) {
      modal.classList.remove('open');
      modal.style.display = 'none';
    }
  };

  window.toggleProfileMenu = function (e) {
    if (e) e.stopPropagation();
    const container = byId('profile-dropdown-container');
    const menu = byId('profile-dropdown-menu');
    if (!container || !menu) return;
    const isOpen = container.classList.contains('open');
    if (isOpen) {
      container.classList.remove('open');
      menu.classList.add('hidden');
    } else {
      container.classList.add('open');
      menu.classList.remove('hidden');
      const username = byId('top-username')?.textContent || 'Tài khoản Live VIP';
      const menuUsername = byId('menu-display-username');
      if (menuUsername) menuUsername.textContent = username;
    }
  };

  window.closeProfileMenu = function () {
    const container = byId('profile-dropdown-container');
    const menu = byId('profile-dropdown-menu');
    if (container) container.classList.remove('open');
    if (menu) menu.classList.add('hidden');
  };

  document.addEventListener('click', (e) => {
    const container = byId('profile-dropdown-container');
    if (container && !container.contains(e.target)) {
      window.closeProfileMenu();
    }
  });

  window.submitTopicPrompt = function () {
    const form = byId('chat-form');
    if (form) {
      form.dispatchEvent(new Event('submit', { cancelable: true, bubbles: true }));
    }
  };

  window.openAvatarModal = function () {
    if (!requireResources()) return;
    byId('avatar-modal').classList.add('open');
    refreshAvatarTasks();
    if (state.avatarTaskPoll) clearInterval(state.avatarTaskPoll);
    state.avatarTaskPoll = setInterval(refreshAvatarTasks, 3000);
  };

  window.closeAvatarModal = function () {
    byId('avatar-modal').classList.remove('open');
    if (state.avatarTaskPoll) {
      clearInterval(state.avatarTaskPoll);
      state.avatarTaskPoll = null;
    }
    loadAvatars();
  };

  window.handleVideoFileSelect = function (input) {
    const nameEl = byId('videoFileName');
    nameEl.textContent = input.files[0]?.name || 'Chưa chọn video';
  };

  window.refreshAvatarTasks = async function () {
    const container = byId('avatarTaskList');
    if (!container) return;
    try {
      const res = await fetch('/api/avatar/tasks');
      const data = await res.json();
      const tasks = data.data?.tasks || [];

      if (!tasks.length) {
        container.innerHTML = '<div style="color:#64748b; font-size:12px; text-align:center; padding:16px;">Chưa có tác vụ avatar. Tạo tác vụ mới ở cột bên trái.</div>';
        return;
      }

      container.innerHTML = tasks.map((task) => {
        const status = String(task.status || 'pending').toLowerCase();
        const progress = Math.max(0, Math.min(100, Number(task.progress) || 0));
        const date = task.start_time ? new Date(task.start_time * 1000).toLocaleString('vi-VN') : '—';
        return `
          <div class="task-card">
            <div class="task-card-head">
              <div><strong>${escapeHtml(task.avatar_id)}</strong></div>
              <span class="status-badge ${safeStatus(status)}">${statusLabelVi(status)}</span>
            </div>
            <div style="font-size:10px; color:#64748b; display:flex; justify-content:space-between; gap:6px;">
              <span style="overflow:hidden; text-overflow:ellipsis; white-space:nowrap; max-width:75%;" title="${escapeHtml(task.stage || date)}">${escapeHtml(task.stage || date)}</span>
              <span style="font-weight:600;">${progress}%</span>
            </div>
            <div class="task-progress-bg">
              <div class="task-progress-fill" style="width:${progress}%"></div>
            </div>
            ${task.error_msg ? `<div style="font-size:10.5px; color:var(--danger);">${escapeHtml(task.error_msg)}</div>` : ''}
            ${status === 'pending' ? `<button class="btn btn-red" style="height:24px; padding:0 8px; font-size:10px; align-self:flex-end;" onclick="deleteAvatarTask('${escapeHtml(task.task_id)}')">Hủy tác vụ</button>` : ''}
          </div>
        `;
      }).join('');
    } catch (err) {
      container.innerHTML = `<div style="color:var(--danger); font-size:12px; text-align:center;">Lỗi tải tác vụ: ${escapeHtml(err.message)}</div>`;
    }
  };

  window.deleteAvatarTask = async function (taskId) {
    if (!confirm('Hủy tác vụ đang chờ này?')) return;
    try {
      const res = await fetch(`/api/avatar/task/${encodeURIComponent(taskId)}`, { method: 'DELETE' });
      const data = await res.json();
      if (!res.ok || data.code !== 0) throw new Error(data.msg || 'Không thể hủy tác vụ');
      refreshAvatarTasks();
    } catch (err) {
      showAppAlert('Lỗi tác vụ', err.message);
    }
  };

  byId('avatarTaskForm')?.addEventListener('submit', async (e) => {
    e.preventDefault();
    const btn = byId('submitAvatarTaskBtn');
    const notice = byId('avatarTaskNotice');
    btn.disabled = true;
    btn.textContent = 'ĐANG TẢI LÊN & XỬ LÝ...';
    notice.style.color = 'var(--primary)';
    notice.textContent = 'Đang tải video và tạo tác vụ...';

    try {
      const form = byId('avatarTaskForm');
      const res = await fetch('/api/avatar/task', { method: 'POST', body: new FormData(form) });
      const result = await res.json();

      if (!res.ok || result.code !== 0) throw new Error(result.msg || 'Tạo tác vụ thất bại.');

      notice.style.color = 'var(--success)';
      notice.textContent = `Đã nhận tác vụ #${result.data.task_id}.`;
      form.reset();
      byId('videoFileName').textContent = 'Chưa chọn video';
      refreshAvatarTasks();
      loadAvatars();
    } catch (err) {
      notice.style.color = 'var(--danger)';
      notice.textContent = err.message || 'Lỗi gửi tác vụ.';
    } finally {
      btn.disabled = false;
      btn.textContent = '↑ TẢI LÊN & TẠO AVATAR';
    }
  });

  // User History / Account Modal (Strict Exclusivity Between Tabs)
  window.openUserHistoryModal = function () {
    byId('user-history-modal').classList.add('open');
    switchHistoryTab('profile');
  };

  window.closeUserHistoryModal = function () {
    byId('user-history-modal').classList.remove('open');
  };

  window.switchHistoryTab = function (tab) {
    const tabs = ['profile', 'invoices', 'sessions'];
    tabs.forEach((t) => {
      const btn = byId(`tab-${t}-btn`);
      const content = byId(`tab-${t}-content`);
      if (btn) btn.classList.toggle('active', t === tab);
      if (content) {
        content.classList.toggle('active', t === tab);
        content.style.display = (t === tab) ? 'block' : 'none';
      }
    });

    if (tab === 'profile') renderUserProfileInfo();
    if (tab === 'invoices') loadUserInvoices();
    if (tab === 'sessions') loadUserSessions();
  };

  async function loadUserInvoices() {
    const tbody = byId('user-invoices-tbody');
    if (!tbody) return;
    tbody.innerHTML = '<tr><td colspan="5" style="text-align: center; padding: 15px; color: #64748b;">Đang tải lịch sử giao dịch...</td></tr>';
    try {
      const payload = await api('/api/billing/invoices');
      const rawInvoices = payload.invoices || [];

      // Strict Frontend Filter: ONLY serviceType === 'live_face'
      const vipInvoices = rawInvoices.filter((inv) => inv.serviceType === 'live_face');
      state.loadedInvoices = vipInvoices;

      if (!vipInvoices.length) {
        tbody.innerHTML = '<tr><td colspan="5" style="text-align: center; padding: 15px; color: #64748b;">Chưa có giao dịch Live VIP.</td></tr>';
        return;
      }

      tbody.innerHTML = vipInvoices.map((inv) => {
        let statusText = 'Chờ thanh toán';
        let statusColor = 'var(--amber)';
        if (inv.status === 'completed') {
          statusText = 'Thành công';
          statusColor = 'var(--success)';
        } else if (inv.status === 'cancelled') {
          statusText = 'Đã hủy';
          statusColor = 'var(--danger)';
        } else if (inv.status === 'failed') {
          statusText = 'Thất bại';
          statusColor = 'var(--danger)';
        }

        const mins = Math.floor((inv.secondsChanged || 0) / 60);
        const dateStr = formatDate(inv.createdAt);

        return `
          <tr onclick="handleInvoiceClick('${inv._id}')" style="border-bottom: 1px solid rgba(255,255,255,0.05); cursor: pointer;" onmouseover="this.style.background='rgba(255,255,255,0.04)'" onmouseout="this.style.background='none'">
            <td style="padding: 8px; font-family: var(--font-cyber); font-weight: bold; color: #fff;">${escapeHtml(inv.code)}</td>
            <td style="padding: 8px; color: #94a3b8;">${dateStr}</td>
            <td style="padding: 8px; font-weight: bold; color: var(--success);">${(inv.amountVnd || 0).toLocaleString('vi-VN')} xu</td>
            <td style="padding: 8px; color: var(--primary); font-weight: bold;">+${mins} phút</td>
            <td style="padding: 8px;"><span style="color: ${statusColor}; font-weight: bold;">${statusText}</span></td>
          </tr>
        `;
      }).join('');
    } catch (err) {
      tbody.innerHTML = `<tr><td colspan="5" style="text-align: center; padding: 15px; color: var(--danger);">Lỗi tải hóa đơn: ${escapeHtml(err.message)}</td></tr>`;
    }
  }

  window.handleInvoiceClick = function (invoiceId) {
    const invoice = state.loadedInvoices.find((inv) => inv._id === invoiceId);
    if (!invoice) return;

    closeUserHistoryModal();

    if (invoice.status === 'pending') {
      showInvoiceCheckout(invoice);
    } else {
      showInvoiceDetail(invoice);
    }
  };

  function showInvoiceCheckout(invoice) {
    state.activeInvoiceId = invoice._id;
    applyInvoicePaymentToQrModal(invoice, state.profile);
    byId('qr-loading-spinner').style.display = 'none';
    byId('qr-modal').classList.add('open');
    startInvoicePolling(invoice._id);
  }

  function showInvoiceDetail(invoice) {
    setText('det-code', invoice.code);
    setText('det-time', formatDate(invoice.createdAt));
    setText('det-amount', formatMoney(invoice.amountVnd));

    const mins = Math.floor((invoice.secondsChanged || 0) / 60);
    setText('det-hours', `+${mins} phút`);

    const statusEl = byId('det-status');
    if (statusEl) {
      if (invoice.status === 'completed') {
        statusEl.textContent = 'Thành công';
        statusEl.style.color = 'var(--success)';
      } else if (invoice.status === 'cancelled') {
        statusEl.textContent = 'Đã hủy';
        statusEl.style.color = 'var(--danger)';
      } else {
        statusEl.textContent = 'Chờ thanh toán';
        statusEl.style.color = 'var(--amber)';
      }
    }

    byId('invoice-detail-modal').classList.add('open');
  }

  window.closeInvoiceDetailModal = function () {
    byId('invoice-detail-modal').classList.remove('open');
    openUserHistoryModal();
  };

  async function loadUserSessions() {
    const tbody = byId('user-sessions-tbody');
    if (!tbody) return;
    tbody.innerHTML = '<tr><td colspan="4" style="text-align: center; padding: 15px; color: #64748b;">Đang tải lịch sử phiên...</td></tr>';
    try {
      const payload = await api('/api/billing/sessions');
      const rawSessions = payload.sessions || [];

      // Strict Frontend Filter: ONLY serviceType === 'live_face'
      const vipSessions = rawSessions.filter((ses) => ses.serviceType === 'live_face');

      if (!vipSessions.length) {
        tbody.innerHTML = '<tr><td colspan="4" style="text-align: center; padding: 15px; color: #64748b;">Chưa có phiên Live VIP.</td></tr>';
        return;
      }

      tbody.innerHTML = vipSessions.map((sess) => {
        const startStr = formatDate(sess.startedAt);
        const endStr = sess.endedAt ? formatDate(sess.endedAt) : 'Đang chạy';

        const mins = Math.floor((sess.totalUsedSeconds || sess.sessionElapsedSeconds || 0) / 60);
        const secs = (sess.totalUsedSeconds || sess.sessionElapsedSeconds || 0) % 60;
        const durationStr = sess.status === 'active' ? 'Đang cập nhật' : `${mins} phút ${secs} giây`;

        let statusText = 'Đang chạy';
        let statusColor = 'var(--primary)';
        if (sess.status === 'ended') {
          statusText = 'Hoàn thành';
          statusColor = '#94a3b8';
        } else if (sess.status === 'expired') {
          statusText = 'Hết hạn';
          statusColor = 'var(--danger)';
        }

        return `
          <tr style="border-bottom: 1px solid rgba(255,255,255,0.05);">
            <td style="padding: 8px; color: #fff;">${startStr}</td>
            <td style="padding: 8px; color: #94a3b8;">${endStr}</td>
            <td style="padding: 8px; font-weight: bold; color: var(--primary);">${durationStr}</td>
            <td style="padding: 8px;"><span style="color: ${statusColor}; font-weight: bold;">${statusText}</span></td>
          </tr>
        `;
      }).join('');
    } catch (err) {
      tbody.innerHTML = `<tr><td colspan="4" style="text-align: center; padding: 15px; color: var(--danger);">Lỗi tải phiên: ${escapeHtml(err.message)}</td></tr>`;
    }
  }

  // Price Modal Packages (Full 6 Packages: 50k, 100k, 150k, 200k, 250k, 500k)
  function updateJulyPromoCountdown() {
    const countdown = byId('august-promo-countdown') || byId('july-promo-countdown');
    if (!countdown) return;

    const now = new Date();
    const promoYear = now.getMonth() > 7 ? now.getFullYear() + 1 : now.getFullYear();
    const promoEnd = new Date(promoYear, 7, 31, 23, 59, 59);
    const totalSeconds = Math.floor(Math.max(0, promoEnd.getTime() - now.getTime()) / 1000);
    const pad = (value) => String(value).padStart(2, '0');

    setText('promo-days', pad(Math.floor(totalSeconds / 86400)));
    setText('promo-hours', pad(Math.floor((totalSeconds % 86400) / 3600)));
    setText('promo-minutes', pad(Math.floor((totalSeconds % 3600) / 60)));
    setText('promo-seconds', pad(totalSeconds % 60));
  }

  function startJulyPromoCountdown() {
    updateJulyPromoCountdown();
    clearInterval(state.julyPromoTimer);
    state.julyPromoTimer = setInterval(updateJulyPromoCountdown, 1000);
  }

  function calculatePackageDurations() {
    const rate = state.rate || 20000;
    const amounts = [50000, 100000, 150000, 200000, 250000, 500000];
    const ids = ['50k', '100k', '150k', '200k', '250k', '500k'];

    amounts.forEach((amt, idx) => {
      const hours = Math.floor(amt / rate);
      const minutes = Math.floor(((amt / rate) % 1) * 60);
      let desc = '~';
      if (hours > 0) desc += `${hours} giờ `;
      if (minutes > 0) desc += `${minutes} phút`;
      if (hours === 0 && minutes === 0) desc += '0 giờ';

      setText(`price-desc-${ids[idx]}`, desc);
    });
  }

  window.openPriceModal = function () {
    state.currentSelectedAmount = 100000;
    selectPrice(100000);
    calculatePackageDurations();
    startJulyPromoCountdown();
    setText('price-modal-rate', `${state.rate.toLocaleString('vi-VN')} xu/giờ`);
    byId('price-modal').classList.add('open');
  };

  window.closePriceModal = function () {
    byId('price-modal').classList.remove('open');
  };

  function updatePromptModalCharCount() {
    const editArea = byId('modal-prompt-edit-textarea');
    const counter = byId('modal-prompt-char-count');
    if (editArea && counter) {
      const text = (editArea.value || '').trim();
      const words = text ? text.split(/\s+/).filter(Boolean).length : 0;
      counter.textContent = `${text.length} ký tự (${words} từ)`;
    }
  }

  window.openPromptModal = function () {
    const editArea = byId('modal-prompt-edit-textarea');
    if (editArea) {
      if (state.activeOptimizedPrompt) {
        editArea.value = state.activeOptimizedPrompt;
      } else if (byId('chat-message')?.value?.trim()) {
        editArea.value = byId('chat-message').value.trim();
      }
      editArea.disabled = false;
      updatePromptModalCharCount();
    }
    byId('prompt-details-modal')?.classList.add('open');
  };

  window.closePromptModal = function () {
    byId('prompt-details-modal')?.classList.remove('open');
  };

  window.handleOptimizePromptClick = async function () {
    const rawInput = (byId('chat-message')?.value || '').trim();
    const promptText = rawInput || state.activePromptTopic || state.activeOptimizedPrompt || '';

    if (!promptText) {
      showAppAlert('Chưa có nội dung', 'Vui lòng nhập chủ đề hoặc nội dung cơ bản về sản phẩm vào ô kịch bản trước khi tối ưu.');
      byId('chat-message')?.focus();
      return;
    }

    state.activePromptTopic = promptText;
    const modalTopic = byId('modal-prompt-topic');
    if (modalTopic) modalTopic.textContent = promptText;

    const modal = byId('prompt-details-modal');
    modal?.classList.add('open');

    // Nếu đã có bản tối ưu cho prompt này và người dùng chưa thay đổi ô chat thì mở xem ngay
    if (state.activeOptimizedPrompt && !rawInput) {
      const editArea = byId('modal-prompt-edit-textarea');
      if (editArea) {
        editArea.value = state.activeOptimizedPrompt;
        editArea.disabled = false;
      }
      updatePromptModalCharCount();
      return;
    }

    await reoptimizePromptFromModal();
  };

  window.reoptimizePromptFromModal = async function () {
    const topicText = (byId('modal-prompt-topic')?.textContent || state.activePromptTopic || byId('chat-message')?.value || '').trim();
    if (!topicText || topicText === 'Chưa có chủ đề nào được gửi') {
      showAppAlert('Chưa có chủ đề', 'Vui lòng nhập nội dung hoặc chủ đề cơ bản trước.');
      return;
    }

    const editArea = byId('modal-prompt-edit-textarea');
    const saveBtn = byId('btn-save-modal-prompt');
    const reoptBtn = byId('btn-reoptimize-prompt');

    if (editArea) {
      editArea.value = '⏳ Đang kết nối trí tuệ nhân tạo (AI) để phân tích chủ đề và xây dựng kịch bản bán hàng tối ưu chuẩn 9 bước... Vui lòng đợi trong giây lát...';
      editArea.disabled = true;
    }
    if (saveBtn) saveBtn.disabled = true;
    if (reoptBtn) reoptBtn.disabled = true;

    try {
      const res = await api('/api/prompt/optimize', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ prompt: topicText })
      });

      const optimizedText = (res.data && res.data.optimized) ? res.data.optimized : topicText;
      if (editArea) {
        editArea.value = optimizedText;
        editArea.disabled = false;
      }
      if (saveBtn) saveBtn.disabled = false;
      if (reoptBtn) reoptBtn.disabled = false;
      updateOptimizedPromptDisplay(optimizedText);
      updatePromptModalCharCount();
    } catch (err) {
      console.warn('Lỗi gọi optimize prompt:', err);
      if (editArea) {
        editArea.value = topicText;
        editArea.disabled = false;
      }
      if (saveBtn) saveBtn.disabled = false;
      if (reoptBtn) reoptBtn.disabled = false;
      updatePromptModalCharCount();
      showAppAlert('Thông báo AI', 'Không thể kết nối dịch vụ AI tối ưu kịch bản: ' + (err.message || 'Lỗi mạng') + '. Bạn có thể trực tiếp viết và chỉnh sửa kịch bản trong khung rồi bấm Lưu.');
    }
  };

  window.saveOptimizedPromptFromModal = async function () {
    const editArea = byId('modal-prompt-edit-textarea');
    const editedText = (editArea?.value || '').trim();
    if (!editedText || editedText.startsWith('⏳ Đang kết nối')) {
      showAppAlert('Nội dung trống', 'Vui lòng chờ AI tạo kịch bản hoặc nhập nội dung kịch bản trước khi lưu.');
      return;
    }

    const saveBtn = byId('btn-save-modal-prompt');
    if (saveBtn) {
      saveBtn.disabled = true;
      saveBtn.innerHTML = '<span>⏳ Đang lưu...</span>';
    }

    try {
      state.activeOptimizedPrompt = editedText;
      updateOptimizedPromptDisplay(editedText);

      const topicText = (byId('modal-prompt-topic')?.textContent || '').trim();
      const displayTopic = (topicText && topicText !== 'Chưa có chủ đề nào được gửi') ? topicText : (editedText.slice(0, 80) + '...');
      updateActivePromptDisplay(displayTopic);

      await api('/api/prompt/save', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          prompt: editedText,
          sessionid: String(byId('sessionid')?.value || '0')
        })
      });

      closePromptModal();
      showAppAlert('Đã lưu kịch bản', 'Kịch bản bán hàng đã sẵn sàng! Bây giờ bạn chỉ cần bấm nút "BẮT ĐẦU PHIÊN LIVE", AI sẽ tự động khởi động cửa sổ Avatar và phát kịch bản ngay lập tức.');
    } catch (err) {
      showAppAlert('Lỗi lưu kịch bản', err.message || 'Không thể lưu kịch bản.');
    } finally {
      if (saveBtn) {
        saveBtn.disabled = false;
        saveBtn.innerHTML = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"/><polyline points="17 21 17 13 7 13 7 21"/><polyline points="7 3 7 8 15 8"/></svg><span>💾 LƯU KỊCH BẢN</span>';
      }
    }
  };

  window.copyOptimizedPrompt = function () {
    const editArea = byId('modal-prompt-edit-textarea');
    const text = editArea?.value || state.activeOptimizedPrompt || byId('modal-prompt-content')?.textContent || '';
    if (!text) return;
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(text).then(() => {
        const btn = byId('btn-copy-prompt');
        if (btn) {
          const original = btn.textContent;
          btn.textContent = '✓ Đã chép';
          setTimeout(() => { btn.textContent = original; }, 1800);
        }
      }).catch(() => {
        showAppAlert('Thông báo', 'Không thể sao chép kịch bản.');
      });
    }
  };

  window.selectPrice = function (amount) {
    state.currentSelectedAmount = amount;
    byId('custom-price-input').value = '';
    setText('custom-price-hours-desc', '');

    document.querySelectorAll('.price-btn').forEach((btn) => {
      btn.classList.toggle('active', btn.id === `pbtn-${amount}`);
    });
  };

  window.calculateCustomHours = function () {
    const val = byId('custom-price-input').value;
    document.querySelectorAll('.price-btn').forEach((btn) => btn.classList.remove('active'));

    if (!val || val <= 0) {
      state.currentSelectedAmount = null;
      setText('custom-price-hours-desc', '');
      return;
    }

    const amount = parseInt(val, 10);
    state.currentSelectedAmount = amount;
    const rate = state.rate || 20000;

    const hours = Math.floor(amount / rate);
    const minutes = Math.floor(((amount / rate) % 1) * 60);
    let desc = 'Quy đổi: ';
    if (hours > 0) desc += `${hours} giờ `;
    if (minutes > 0) desc += `${minutes} phút`;
    if (hours === 0 && minutes === 0) desc += '0 giờ';

    setText('custom-price-hours-desc', desc);
  };

  window.confirmPriceSelection = async function () {
    const amount = state.currentSelectedAmount;
    if (!amount || amount < 50000) {
      showAppAlert('Cảnh báo', 'Vui lòng chọn hoặc nhập số xu nạp tối thiểu 50.000 xu (50.000 VNĐ).');
      return;
    }

    closePriceModal();

    byId('qr-modal').classList.add('open');
    byId('qr-loading-spinner').style.display = 'flex';
    byId('qr-image').style.display = 'none';
    setText('polling-status-msg', '🕒 Đang tạo hóa đơn từ hệ thống...');

    try {
      const payload = await api('/api/billing/invoice/create', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ serviceType: SERVICE_TYPE, amountVnd: amount }),
      });

      const invoice = payload.invoice;

      if (invoice.serviceType !== SERVICE_TYPE) {
        throw new Error('Hóa đơn không thuộc dịch vụ Live VIP');
      }

      state.activeInvoiceId = invoice._id;

      applyInvoicePaymentToQrModal(invoice, state.profile);
      byId('qr-loading-spinner').style.display = 'none';
      startInvoicePolling(invoice._id);

    } catch (err) {
      showAppAlert('Lỗi tạo hóa đơn', err.message || 'Lỗi khởi tạo hóa đơn.');
      hideQrModal();
    }
  };

  function buildTransferQrUrl(bankDetails, invoice) {
    if (!bankDetails || !bankDetails.accountNumber) return '';
    if (!/^[0-9]{6}$/.test(bankDetails.bankBin || '') || !bankDetails.accountName || !invoice.transferContent) return '';
    return 'https://api.vietqr.io/image/' + bankDetails.bankBin + '-' + encodeURIComponent(bankDetails.accountNumber) +
      '-qr_only.jpg?accountName=' + encodeURIComponent(bankDetails.accountName) +
      '&amount=' + encodeURIComponent(invoice.amountVnd) + '&addInfo=' + encodeURIComponent(invoice.transferContent);
  }

  function applyInvoicePaymentToQrModal(invoice, profile = {}) {
    const bankDetails = invoice.paymentSnapshot || {};
    const qrImage = byId('qr-image');

    const rateSnapshot = invoice.hourlyRateSnapshot || state.rate || 20000;
    setText('modal-rate', `${rateSnapshot.toLocaleString('vi-VN')} xu/giờ`);
    setText('modal-bank-name', bankDetails.bankName || 'Chưa xác định');
    setText('modal-bank-acc', bankDetails.accountNumber || '--');
    setText('modal-bank-owner', bankDetails.accountName || '--');
    setText('modal-amount', `${(invoice.amountVnd || 0).toLocaleString('vi-VN')} VNĐ (${formatMoney(invoice.amountVnd)})`);
    setText('modal-bank-memo', invoice.transferContent || invoice.code || '--');

    const qrUrl = buildTransferQrUrl(bankDetails, invoice);
    if (qrUrl) {
      qrImage.src = qrUrl;
      qrImage.style.display = 'block';
    } else {
      qrImage.removeAttribute('src');
      qrImage.style.display = 'none';
    }

    setText('polling-status-msg', '🕒 Hệ thống đang kiểm tra thanh toán (Tự động kích hoạt trong vòng 20 giây sau khi nhận tiền)...');
  }

  // Invoice Polling & Check logic
  function stopInvoicePolling({ clearStoredId = false } = {}) {
    if (state.invoicePollInterval) {
      clearInterval(state.invoicePollInterval);
      state.invoicePollInterval = null;
    }
    if (clearStoredId) {
      localStorage.removeItem('live_face_active_pending_invoice_id');
      state.activeInvoiceId = null;
    }
  }

  function hideQrModal() {
    // Only hide the QR modal UI element, NEVER stop background polling!
    byId('qr-modal')?.classList.remove('open');
  }

  function showPaymentSuccess(invoice) {
    hideQrModal();
    byId('cancel-confirm-modal')?.classList.remove('open');

    setText('success-det-code', invoice.code || '--');
    setText('success-det-time', new Date().toLocaleString('vi-VN'));
    setText('success-det-amount', formatMoney(invoice.amountVnd));

    const mins = Math.floor((invoice.secondsChanged || 0) / 60);
    setText('success-det-hours', `+${mins} phút`);

    byId('payment-success-modal')?.classList.add('open');

    // Immediate profile load to update balance & rate
    loadProfile().then(() => {
      // Schedule follow-up loadProfile 1.5s later to guarantee freshness
      setTimeout(loadProfile, 1500);
    });
  }

  async function checkActivePendingInvoice() {
    if (state.invoiceCheckInFlight) return;
    const pendingId = localStorage.getItem('live_face_active_pending_invoice_id') || state.activeInvoiceId;
    if (!pendingId) return;

    state.invoiceCheckInFlight = true;

    try {
      const payload = await api(`/api/invoice/status/${pendingId}`);
      const inv = payload.invoice || {};
      const status = payload.status || inv.status;
      const balanceCreditedAt = payload.balanceCreditedAt || inv.balanceCreditedAt;

      if (inv.serviceType && inv.serviceType !== SERVICE_TYPE) {
        stopInvoicePolling({ clearStoredId: true });
        return;
      }

      if (status === 'completed') {
        if (!balanceCreditedAt) {
          setText('polling-status-msg', '🕒 Đã xác nhận thanh toán, đang cộng thời gian...');
        } else {
          stopInvoicePolling({ clearStoredId: true });
          showPaymentSuccess(inv);
        }
      } else if (status === 'cancelled') {
        stopInvoicePolling({ clearStoredId: true });
        hideQrModal();
        showAppAlert('Hóa đơn đã bị hủy', 'Hóa đơn nạp xu đã được đánh dấu hủy.');
        loadProfile();
      }
    } catch (err) {
      console.error('Background invoice check error:', err);
    } finally {
      state.invoiceCheckInFlight = false;
    }
  }

  function startInvoicePolling(invoiceId) {
    if (!invoiceId) return;
    localStorage.setItem('live_face_active_pending_invoice_id', invoiceId);
    state.activeInvoiceId = invoiceId;

    if (state.invoicePollInterval) {
      clearInterval(state.invoicePollInterval);
      state.invoicePollInterval = null;
    }

    // Inspect status IMMEDIATELY on first call
    checkActivePendingInvoice();

    // Schedule background polling every 2 seconds
    state.invoicePollInterval = setInterval(checkActivePendingInvoice, 2000);
  }

  function restorePendingInvoicePolling() {
    const pendingId = localStorage.getItem('live_face_active_pending_invoice_id');
    if (pendingId) {
      state.activeInvoiceId = pendingId;
      startInvoicePolling(pendingId);
    }
  }

  window.closeQrModal = function () {
    // ONLY hide the QR modal overlay from screen; DO NOT stop background polling!
    hideQrModal();
    loadProfile();
  };

  window.closePaymentSuccessModal = function () {
    byId('payment-success-modal')?.classList.remove('open');
  };

  window.cancelPendingInvoice = function () {
    if (!state.activeInvoiceId) return;
    byId('cancel-confirm-modal')?.classList.add('open');
  };

  window.closeCancelConfirmModal = function () {
    byId('cancel-confirm-modal')?.classList.remove('open');
  };

  window.executeCancelPendingInvoice = async function () {
    const pendingId = state.activeInvoiceId || localStorage.getItem('live_face_active_pending_invoice_id');
    if (!pendingId) {
      closeCancelConfirmModal();
      return;
    }

    const btnExec = byId('btn-execute-cancel-invoice');
    if (btnExec) btnExec.disabled = true;

    try {
      const payload = await api(`/api/invoice/cancel/${pendingId}`, { method: 'POST' });
      closeCancelConfirmModal();

      if (payload.success) {
        stopInvoicePolling({ clearStoredId: true });
        hideQrModal();
        showAppAlert('Đã hủy hóa đơn', 'Đã hủy hóa đơn nạp xu thành công.');
        loadProfile();
      } else if (payload.isCompleted || payload.invoice?.status === 'completed') {
        const inv = payload.invoice || {};
        if (inv.balanceCreditedAt) {
          stopInvoicePolling({ clearStoredId: true });
          showPaymentSuccess(inv);
        } else {
          showAppAlert('Hóa đơn đã được thanh toán', 'Hóa đơn đã hoàn thành, hệ thống đang cập nhật số dư.');
        }
      } else {
        showAppAlert('Lỗi', payload.error || 'Hủy hóa đơn thất bại.');
      }
    } catch (err) {
      console.error('Cancel invoice error:', err);
      showAppAlert('Lỗi hủy hóa đơn', err.message || 'Không thể hủy hóa đơn.');
      closeCancelConfirmModal();
    } finally {
      if (btnExec) btnExec.disabled = false;
    }
  };

  // Logout Modal
  window.openLogoutModal = function () {
    byId('logout-modal')?.classList.add('open');
  };

  window.closeLogoutModal = function () {
    byId('logout-modal')?.classList.remove('open');
  };

  window.executeLogout = async function () {
    if (state.running) {
      await stopLive(true);
    }
    stopProfilePolling();
    await disconnectPresence();
    stopInvoicePolling({ clearStoredId: false });
    try {
      await fetch('/api/auth/logout', { method: 'POST' });
    } catch (_) { }
    window.location.href = '/login.html';
  };

  function parseTikTokUsername(value) {
    const raw = (value || '').trim();
    if (!raw) return '';
    let cleaned = raw.split('?')[0].split('#')[0].replace(/\/+$/g, '');
    cleaned = cleaned.replace(/\/live\/?$/i, '');
    const atMatch = cleaned.match(/@([A-Za-z0-9_.]+)/);
    if (atMatch && atMatch[1]) {
      cleaned = atMatch[1];
    } else if (cleaned.includes('/')) {
      cleaned = cleaned.split('/').pop();
    }
    cleaned = cleaned.replace(/^@+/, '').trim();
    if (!/^[A-Za-z0-9_.]{2,24}$/.test(cleaned)) return '';
    return cleaned;
  }

  function setTikTokButtonState(connected, username = '') {
    state.tiktokConnected = connected;
    if (username) {
      state.tiktokUsername = username.replace(/^@+/, '');
    }
    const btn = byId('btn-connect-tiktok');
    const input = byId('tiktok-username');
    const banner = byId('tiktok-error-banner');
    if (!btn) return;
    if (connected) {
      btn.textContent = '■ NGẮT KẾT NỐI';
      btn.classList.add('btn-tiktok-disconnect');
      btn.classList.remove('btn-green');
      if (banner) banner.style.display = 'none';
      if (input) {
        input.disabled = true;
        if (username) input.value = username.startsWith('@') ? username : `@${username}`;
      }
    } else {
      btn.textContent = 'KẾT NỐI';
      btn.classList.remove('btn-tiktok-disconnect');
      btn.classList.add('btn-green');
      if (input) input.disabled = false;
    }
  }

  async function checkTikTokStatus() {
    try {
      const res = await fetch('/tiktok_status');
      const payload = await res.json();
      const connected = Boolean(payload.code === 0 && payload.data?.connected);
      const errorMsg = payload.data?.error || '';
      const username = payload.data?.username || '';
      const banner = byId('tiktok-error-banner');

      if (connected) {
        setTikTokButtonState(true, username);
        if (banner) banner.style.display = 'none';
      } else {
        const wasConnected = state.tiktokConnected;
        setTikTokButtonState(false);

        if (errorMsg) {
          if (banner) {
            banner.innerHTML = `<span>❌ <strong>Lỗi TikTok Live:</strong> ${escapeHtml(errorMsg)}</span>`;
            banner.style.display = 'flex';
          }
          addLogMessage('event-messages', `❌ LỖI TIKTOK LIVE: ${errorMsg}`, 'system');
          showAppAlert(
            'Lỗi kết nối TikTok Live',
            `${errorMsg}\n\nTiến trình kết nối TikTok đã bị ngắt. Vui lòng kiểm tra lại tài khoản và bấm nút "KẾT NỐI" lại.`
          );
        } else if (wasConnected) {
          if (banner) {
            banner.innerHTML = '<span>⚠️ <strong>Cảnh báo:</strong> Mất kết nối TikTok Live. Vui lòng bấm KẾT NỐI lại.</span>';
            banner.style.display = 'flex';
          }
          addLogMessage('event-messages', '⚠️ Mất kết nối TikTok Live. Vui lòng bấm KẾT NỐI lại.', 'system');
        }
      }
    } catch (_) { }
  }

  // Setup Event Bindings
  function bindEvents() {
    byId('start')?.addEventListener('click', startLive);
    byId('stop')?.addEventListener('click', () => stopLive(true));
    byId('btn-optimize-prompt')?.addEventListener('click', handleOptimizePromptClick);
    byId('modal-prompt-edit-textarea')?.addEventListener('input', updatePromptModalCharCount);

    byId('btn_select_avatar')?.addEventListener('click', async () => {
      if (state.running) {
        return showAppAlert('Đang trong phiên Live', 'Vui lòng DỪNG phiên Live trước khi đổi Mẫu Live để tránh làm méo hình ảnh.');
      }
      const avatar = byId('select-avatar').value;
      if (!avatar) return showAppAlert('Thông báo', 'Chưa có avatar để chọn.');
      selectAvatarById(avatar);
    });

    byId('chat-form')?.addEventListener('submit', async (e) => {
      e.preventDefault();
      const text = byId('chat-message').value.trim();
      if (!text) return;

      if (!state.running) {
        showAppAlert('Chưa bắt đầu Live', 'Vui lòng bấm nút “BẮT ĐẦU” trước khi gửi nội dung cho nhân vật AI.');
        return;
      }

      const liveSessionId = String(byId('sessionid')?.value || '').trim();

      try {
        updateActivePromptDisplay(text);
        showPromptGeneratingIndicator();
        await api('/human', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            text,
            type: 'chat',
            interrupt: true,
            continuous: true,
            tts: { ref_file: state.speechVoice, speed: state.speechSpeed },
            sessionid: liveSessionId
          }),
        });
        byId('chat-message').value = '';
      } catch (err) {
        hidePromptGeneratingIndicator();
        showAppAlert('Không thể gửi nội dung', err.message || 'Vui lòng kiểm tra trạng thái Avatar Live rồi thử lại.');
      }
    });

    byId('btn-connect-tiktok')?.addEventListener('click', async () => {
      if (state.tiktokConnected) {
        try {
          await api('/tiktok_disconnect', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' });
          api('/api/billing/tiktok-channel', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ action: 'disconnect' })
          }).catch(() => { });
          setTikTokButtonState(false);
          addLogMessage('event-messages', 'Hệ thống: Đã ngắt kết nối TikTok Live.', 'system');
        } catch (err) {
          showAppAlert('Lỗi ngắt kết nối TikTok', err.message);
        }
        return;
      }

      const rawVal = byId('tiktok-username').value.trim();
      const cleanUser = parseTikTokUsername(rawVal);
      if (!cleanUser) {
        return showAppAlert('Cảnh báo', 'Hãy nhập chính xác username hoặc link TikTok Live (ví dụ: @tenkenh hoặc link live).');
      }
      const btn = byId('btn-connect-tiktok');
      const banner = byId('tiktok-error-banner');
      if (banner) banner.style.display = 'none';
      if (btn) btn.disabled = true;
      try {
        const payload = await api('/tiktok_connect', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ username: cleanUser, sessionid: String(byId('sessionid').value) }),
        });
        const verifiedUsername = payload.data?.username || cleanUser;
        setTikTokButtonState(true, verifiedUsername);

        // Đồng bộ kênh TikTok lên backend trung tâm & trang admin ngay lập tức
        api('/api/billing/tiktok-channel', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            tiktokChannel: verifiedUsername,
            channel: verifiedUsername
          })
        }).catch((e) => console.warn('[Sync TikTok Channel Error]', e));

        addLogMessage(
          'event-messages',
          `Hệ thống: Đang kết nối @${verifiedUsername} Live, bắt đầu nhận sự kiện và bình luận...`,
          'system'
        );
      } catch (err) {
        if (banner) {
          banner.innerHTML = `<span>❌ <strong>Lỗi kết nối:</strong> ${escapeHtml(err.message)}</span>`;
          banner.style.display = 'flex';
        }
        showAppAlert('Lỗi kết nối TikTok', err.message);
      } finally {
        if (btn) btn.disabled = false;
      }
    });

    // Backdrop click handlers for playlist modals
    ['edit-product-modal', 'custom-confirm-modal', 'extension-guide-modal'].forEach((id) => {
      const el = byId(id);
      if (el) {
        el.addEventListener('click', (e) => {
          if (e.target === el) {
            if (id === 'edit-product-modal') closeEditProductModal();
            else if (id === 'custom-confirm-modal') closeCustomConfirm(false);
            else if (id === 'extension-guide-modal') closeExtensionGuideModal();
          }
        });
      }
    });

    checkTikTokStatus();
    setInterval(checkTikTokStatus, 2500);
    setupVoiceInput();
  }

  function setupVoiceInput() {
    const SpeechRecognition = window.SpeechRecognition || window.webkitSpeechRecognition;
    const recognition = SpeechRecognition ? new SpeechRecognition() : null;

    if (recognition) {
      recognition.lang = 'vi-VN';
      recognition.onresult = (evt) => {
        byId('chat-message').value = Array.from(evt.results).map((r) => r[0].transcript).join(' ');
      };
    }

    let recorder = null;
    let recording = false;
    const btn = byId('voice-record-btn');

    if (!btn) return;

    const begin = async (e) => {
      e.preventDefault();
      if (recording) return;
      try {
        const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
        recorder = new MediaRecorder(stream);
        recorder.start();
        recording = true;
        btn.classList.add('recording');
        recognition?.start();
      } catch (err) { showAppAlert('Lỗi micro', err.message); }
    };

    const end = () => {
      if (!recording) return;
      recording = false;
      recorder?.stop();
      recorder?.stream.getTracks().forEach((track) => track.stop());
      recognition?.stop();
      btn.classList.remove('recording');
    };

    btn.addEventListener('mousedown', begin);
    btn.addEventListener('touchstart', begin, { passive: false });
    ['mouseup', 'mouseleave', 'touchend'].forEach((evtName) => btn.addEventListener(evtName, end));
  }

  // Window Global Handlers for WebRTC (client.js)
  window.onWebRTCConnected = () => setWebRtcConnected(true, 'Online');
  window.onWebRTCDisconnected = () => setWebRtcConnected(false, state.running ? 'Mất kết nối' : 'Offline');

  window.addEventListener('beforeunload', () => {
    stopProfilePolling();
    stopPresenceHeartbeat();
    stopInvoicePolling({ clearStoredId: false });
    navigator.sendBeacon(
      '/api/billing/presence/disconnect',
      new Blob([JSON.stringify({ clientId: presenceClientId })], { type: 'application/json' }),
    );
    if (!state.running) return;
    navigator.sendBeacon('/api/billing/session/end', new Blob(['{}'], { type: 'application/json' }));
  });

  let availableUpdate = null;
  window.closeUpdateModal = () => byId('update-modal')?.classList.remove('open');

  window.onUpdateProgress = (data) => {
    const container = byId('update-progress-container');
    if (container) container.style.display = 'block';
    const bar = byId('update-progress-bar');
    if (bar) bar.style.width = `${Math.min(100, Math.max(0, data.percent))}%`;
    setText('update-progress-percent', `${data.percent}%`);
    setText('update-speed', `Tốc độ: ${data.speedMb} MB/s`);
    setText('update-size', `${data.doneMb} MB / ${data.totalMb} MB`);
    const etaText = data.etaSec <= 0 ? 'Sắp xong' : `Còn: ~${data.etaSec}s`;
    setText('update-eta', etaText);
  };

  // Hàm đọc dữ liệu phiên bản THỰC TẾ 100% từ hệ thống bản build
  async function getRealAppVersionInfo() {
    // 1. Thử gọi Desktop API khi chạy trong pywebview
    try {
      if (window.pywebview?.api?.get_app_version) {
        const res = await window.pywebview.api.get_app_version();
        if (res?.success && res.version) {
          return { currentVersion: String(res.version).trim() };
        }
      }
    } catch (_) { }

    // 2. Thử gọi Backend API /api/app/version (backend đọc trực tiếp file version.json thật trên máy)
    try {
      const resp = await fetch('/api/app/version?t=' + Date.now());
      if (resp.ok) {
        const json = await resp.json();
        const data = json?.data || json;
        if (data?.currentVersion || data?.version) {
          return {
            currentVersion: String(data.currentVersion || data.version).trim(),
            latestVersion: data.latestVersion || data.version,
            available: Boolean(data.available),
            notes: data.notes || '',
            url: data.url || ''
          };
        }
      }
    } catch (_) { }

    // 3. Thử đọc file tĩnh /version.json
    try {
      const vResp = await fetch('/version.json?t=' + Date.now());
      if (vResp.ok) {
        const vData = await vResp.json();
        if (vData?.version) {
          return { currentVersion: String(vData.version).trim() };
        }
      }
    } catch (_) { }

    return null;
  }

  window.openUpdateModal = async () => {
    byId('update-modal')?.classList.add('open');
    setText('update-status', 'Đang kết nối kiểm tra dữ liệu thật từ bản build...');
    const notesEl = byId('update-notes');
    if (notesEl) {
      notesEl.textContent = 'Đang tải thông tin...';
      notesEl.style.display = 'block';
    }
    const container = byId('update-progress-container');
    if (container) container.style.display = 'none';
    const action = byId('update-action');
    if (action) {
      action.disabled = true;
      action.textContent = 'CẬP NHẬT NGAY';
    }
    const later = byId('update-later');
    if (later) later.style.display = 'inline-block';

    // 1. Lấy phiên bản thực tế của bản build
    const realInfo = await getRealAppVersionInfo();
    const curVer = realInfo?.currentVersion || state.appVersion;

    if (curVer) {
      state.appVersion = curVer;
      setText('top-app-version', `v${curVer}`);
    }

    // 2. Kiểm tra thông tin cập nhật
    try {
      let result = null;
      if (window.pywebview?.api?.check_for_update) {
        result = await window.pywebview.api.check_for_update();
      } else if (realInfo?.latestVersion) {
        result = { success: true, ...realInfo };
      } else {
        try {
          const resp = await fetch('/api/app/version?t=' + Date.now());
          if (resp.ok) {
            const data = await resp.json();
            if (data?.code === 0 && data?.data) {
              result = { success: true, ...data.data };
            }
          }
        } catch (_) { }
      }

      if (result?.success) {
        availableUpdate = result;
        const cur = result.currentVersion || curVer;
        if (cur) setText('top-app-version', `v${cur}`);

        if (!result.available) {
          setText('update-status', `Phiên bản bản build đang chạy: v${cur} (Bản mới nhất)`);
          if (notesEl) {
            notesEl.textContent = `Hệ thống Slive VIP đang vận hành phiên bản thật v${cur}.\nKhông có bản vá vi sai nào mới hơn trên máy chủ.`;
          }
          return;
        }

        setText('update-status', `Có phiên bản mới v${result.latestVersion || result.version} (Bản hiện tại: v${cur})`);
        if (notesEl) {
          notesEl.textContent = result.notes || 'Bản vá cập nhật tính năng mới và cải thiện độ ổn định hệ thống.';
        }
        if (action && window.pywebview?.api?.install_update) {
          action.disabled = false;
        } else if (action) {
          action.disabled = true;
          action.textContent = 'CẬP NHẬT TRÊN DESKTOP';
        }
        if (result.mandatory && byId('update-later')) byId('update-later').style.display = 'none';
      } else if (curVer) {
        setText('update-status', `Phiên bản bản build đang chạy: v${curVer}`);
        if (notesEl) {
          notesEl.textContent = `Dữ liệu thật từ hệ thống: Phiên bản v${curVer}.\n(Đang mở qua trình duyệt web. Để tự động tải bản vá 1-click, vui lòng mở ứng dụng SliveVip.exe trên máy tính).`;
        }
      } else {
        setText('update-status', 'Không tìm thấy dữ liệu phiên bản của bản build.');
        if (notesEl) {
          notesEl.textContent = 'Không thể đọc file version.json trên thư mục ứng dụng.';
        }
      }
    } catch (err) {
      if (curVer) {
        setText('update-status', `Phiên bản bản build đang chạy: v${curVer}`);
        if (notesEl) {
          notesEl.textContent = `Phiên bản: v${curVer}\nLưu ý: Không thể kết nối máy chủ kiểm tra cập nhật (${err.message}).`;
        }
      } else {
        setText('update-status', `Lỗi kiểm tra phiên bản: ${err.message}`);
      }
    }
  };

  window.installAvailableUpdate = async () => {
    if (!availableUpdate) return;
    const action = byId('update-action');
    const later = byId('update-later');
    if (action) {
      action.disabled = true;
      action.textContent = 'ĐANG TẢI...';
    }
    if (later) later.style.display = 'none';
    const container = byId('update-progress-container');
    if (container) container.style.display = 'block';
    setText('update-progress-label', 'Đang tải bản cập nhật...');
    setText('update-status', 'Đang tải và kiểm tra an toàn. Vui lòng không tắt ứng dụng...');

    try {
      const result = await window.pywebview.api.install_update(availableUpdate);
      if (!result?.success) {
        throw new Error(result?.error || 'Cập nhật thất bại.');
      }
      setText('update-progress-label', 'Hoàn tất 100%!');
      const bar = byId('update-progress-bar');
      if (bar) bar.style.width = '100%';
      setText('update-progress-percent', '100%');

      if (result.is_patch) {
        const statusEl = byId('update-status');
        if (statusEl) {
          statusEl.innerHTML = `
            <div style="background:rgba(34, 197, 94, 0.12); border:1px solid rgba(34, 197, 94, 0.4); border-radius:10px; padding:12px 14px; margin-top:8px;">
              <div style="color:#4ade80; font-weight:800; font-size:14.5px; display:flex; align-items:center; gap:6px;">
                <span>✅</span> Cập nhật bản vá ${escapeHtml(result.version ? 'v' + result.version : '')} thành công!
              </div>
              <div style="color:#cbd5e1; font-size:12.5px; margin-top:6px; line-height:1.5;">
                Đã cài đặt bản vá vào máy. Vui lòng <strong>khởi động lại ứng dụng</strong> để áp dụng toàn bộ giao diện và tính năng mới.
              </div>
            </div>
          `;
        }
        if (later) {
          later.style.display = 'inline-block';
          later.textContent = 'ĐÓNG & TỰ KHỞI ĐỘNG SAU';
          later.onclick = () => {
            if (window._restartTimer) clearInterval(window._restartTimer);
            closeUpdateModal();
          };
        }
        if (action) {
          action.disabled = false;
          let count = 5;
          action.textContent = `🚀 KHỞI ĐỘNG LẠI (${count}s)`;
          action.onclick = () => {
            if (window._restartTimer) clearInterval(window._restartTimer);
            restartAppNow();
          };
          window._restartTimer = setInterval(() => {
            count--;
            if (count <= 0) {
              clearInterval(window._restartTimer);
              restartAppNow();
            } else {
              action.textContent = `🚀 KHỞI ĐỘNG LẠI (${count}s)`;
            }
          }, 1000);
        }
      } else {
        setText('update-status', '✅ Đã tải xong bộ cài mới. Đang khởi chạy trình cập nhật...');
      }
    } catch (err) {
      if (action) {
        action.disabled = false;
        action.textContent = 'THỬ LẠI';
      }
      if (later) later.style.display = 'inline-block';
      setText('update-status', `❌ ${err.message}`);
    }
  };

  window.restartAppNow = async function () {
    const action = byId('update-action');
    if (action) {
      action.disabled = true;
      action.textContent = 'ĐANG KHỞI ĐỘNG LẠI...';
    }
    if (window.pywebview?.api?.restart_app) {
      try {
        await window.pywebview.api.restart_app();
      } catch (e) {
        window.location.reload();
      }
    } else {
      window.location.reload();
    }
  };

  async function initializeDesktopVersion() {
    const info = await getRealAppVersionInfo();
    if (info?.currentVersion) {
      state.appVersion = info.currentVersion;
      setText('top-app-version', `v${info.currentVersion}`);
    }

    try {
      const api = window.pywebview?.api;
      if (api?.check_for_update) {
        const updateResult = await api.check_for_update();
        if (updateResult?.success && updateResult.available) {
          availableUpdate = updateResult;
          window.openUpdateModal();
        }
      }
    } catch (_) { }
  }

  // ==========================================
  // PLAYLIST & TIKTOK AUTO-PIN CONTROLLER
  // ==========================================
  let playlistState = { products: [], config: {}, runtime: {} };
  let playlistPollingInterval = null;
  let localPlaylistTimerInterval = null;
  let lastObservedActiveProductId = null;
  let confirmCallback = null;

  const recentRotationIndices = [];
  const recentDirectPinIndices = [];

  function getSmartNonRepeatingIndex(arrayLength, recentList, historyLimit = 6) {
    if (arrayLength <= 1) return 0;
    const effectiveLimit = Math.min(historyLimit, Math.max(1, Math.floor(arrayLength / 2)));
    const recentSet = new Set(recentList.slice(-effectiveLimit));
    const candidates = [];
    for (let i = 0; i < arrayLength; i++) {
      if (!recentSet.has(i)) candidates.push(i);
    }
    const pool = candidates.length > 0 ? candidates : Array.from({ length: arrayLength }, (_, i) => i);
    const chosen = pool[Math.floor(Math.random() * pool.length)];
    recentList.push(chosen);
    if (recentList.length > 20) {
      recentList.splice(0, recentList.length - 20);
    }
    return chosen;
  }

  function cleanProductTitleForSpeech(title) {
    if (!title) return 'sản phẩm';
    let cleaned = title
      .replace(/\[[^\]]*\]/g, ' ')
      .replace(/\([^\)]*\)/g, ' ')
      .replace(/【[^】]*】/g, ' ')
      .replace(/\{[^\}]*\}/g, ' ')
      .replace(/^#?\s*(?:sản\s*phẩm|sp|mẫu|deal|mã)?\s*#?\s*\d+[\s:.\-]+/i, '')
      .replace(/\b(?:chính\s*hãng|cao\s*cấp|hot\s*deal|sale\s*sốc|freeship(?:\s*toàn\s*quốc)?|xả\s*kho|siêu\s*rẻ|giảm\s*giá|chất\s*lượng)\b/gi, ' ')
      .replace(/[#*_\-~|]/g, ' ')
      .replace(/\s+/g, ' ')
      .trim();
    const words = cleaned.split(' ').filter(Boolean).slice(0, 6).join(' ');
    return words || 'món này';
  }

  function cleanScriptGreeting(text) {
    if (!text) return '';
    return text
      .replace(/^(?:dạ\s+)?(?:em\s+)?(?:xin\s+)?chào\s+(?:cả\s+nhà|mọi\s+người|quý\s+vị|các\s+bác|anh\s+chị|mình|tất\s+cả\s+mọi\s+người)(?:\s+nhé|\s+nha|\s+ạ)?(?:[,.!?\-–—\s]+)/i, '')
      .replace(/^(?:chào\s+mừng\s+(?:cả\s+nhà|mọi\s+người|các\s+bạn|quý\s+vị)\s+(?:đã\s+)?(?:đến\s+với|ghé\s+thăm)\s+(?:phiên|buổi)\s+live(?:stream)?(?:\s+hôm\s+nay)?)(?:[,.!?\-–—\s]+)/i, '')
      .replace(/^(?:hello|hi)\s+(?:cả\s+nhà|mọi\s+người|các\s+bạn)(?:[,.!?\-–—\s]+)/i, '')
      .replace(/^(?:sản\s*phẩm\s+số\s+#?\d+|deal\s+số\s+#?\d+|siêu\s+phẩm\s+số\s+#?\d+|mẫu\s+số\s+#?\d+)\s*(?:là|:|\-)?\s*/i, '')
      .trim();
  }

  function buildTransitionSentence(prevProd, nextProd, context = 'auto') {
    if (!nextProd) return '';
    const config = playlistState.config || {};
    if (config.transition_phrase === false) return '';
    const nextTitle = cleanProductTitleForSpeech(nextProd.title);

    // Ngữ cảnh 1: Chuyển vòng xoay giữa 2 sản phẩm (có sản phẩm trước đó khác sản phẩm hiện tại)
    if (prevProd && prevProd.title && prevProd.id !== nextProd.id && context !== 'pin') {
      const prevTitle = cleanProductTitleForSpeech(prevProd.title);
      const rotationTemplates = [
        // Giao lưu & Chiều lòng khách
        `Nãy giờ nhiều bác nhắn tin hỏi em món này quá trời, nên thôi em xin phép cho lên sóng luôn nha, chính là em ${nextTitle} đây cả nhà ơi!`,
        `Chiều lòng các bác luôn nè, món trước ai chốt xong rồi thì giữ đơn, giờ em chuyển qua ngay em ${nextTitle} theo yêu cầu của mọi người đây ạ!`,
        `Em thấy trong phần bình luận nhiều chị dặn phải lên món này sớm, dạ vâng em nghe lời cả nhà, lên liền em ${nextTitle} cho nóng nhé!`,
        `Dạ ai nãy giờ đang đợi ${nextTitle} thì nhìn lên cùng em nha, vừa xong món trước là em ưu tiên lên liền cho mọi người đây ạ!`,
        `Nhiều bác khen món trước nhưng vẫn ngóng em này đúng không ạ? Dạ em lên ngay ${nextTitle} cho cả nhà mình đây rồi nhé!`,

        // Trải nghiệm thực tế & Khen chất lượng
        `Món vừa rồi đã mê thì qua tới em ${nextTitle} này chỉ có mê hơn thôi cả nhà ơi, hàng về tay bao xịn xò chuẩn chỉnh từng chi tiết luôn!`,
        `Ai vừa chốt đơn trước xong thì hoan hỉ nha, còn bây giờ cùng em nghía qua ${nextTitle}, em này em tự tin chấm điểm mười trên mười luôn á!`,
        `Tiếp theo là một em chân ái của nhà em luôn nè cả nhà, chính là ${nextTitle}, dùng một lần là bảo đảm không muốn đổi sang loại khác đâu ạ!`,
        `Món trước xịn một thì em ${nextTitle} này xịn mười nha các bác, riêng em này nhận hàng về cứ mở ra kiểm tra thoải mái cho em nhé!`,
        `Bác nào kỹ tính thích đồ bền đẹp thì nhìn ngay qua em ${nextTitle} này nha, chất lượng vượt xa ngoài mong đợi của mọi người luôn đấy ạ!`,

        // Deal bất ngờ & Khan hiếm
        `Món trước em xin phép tạm dừng ở đây nha, giờ em lên nhanh ${nextTitle}, mã này số lượng trong kho còn ít nên ai ưng là bấm tay chốt sớm nè!`,
        `Trời ơi qua tới em ${nextTitle} này thì em xin được deal hời chấn động luôn cả nhà ơi, đúng phiên live hôm nay mới có giá này thôi á!`,
        `Các bác tranh thủ món trước nhé, còn bây giờ nhìn ngay vào ${nextTitle} giúp em, em này được trợ giá sâu bất ngờ luôn cả nhà ơi!`,
        `Dạ ${nextTitle} phiên này em để giá tri ân siêu mềm luôn ạ, số lượng mở bán có hạn nên mọi người tranh thủ chớp cơ hội liền nha!`,
        `Suất ưu đãi cho món trước gần hết rồi, giờ em mở tiếp deal khủng cho em ${nextTitle}, ai nhanh tay thì còn chậm tay là tiếc hùi hụi nha!`,

        // Chuyển cảnh mềm mại & Nhịp nhàng
        `Rồi cả nhà ơi, tạm khép lại món cũ nha, giờ cùng em lướt sang siêu phẩm tiếp theo chính là em ${nextTitle} xuất sắc không kém đây ạ!`,
        `Tiếp tục phiên live sôi động hôm nay nhé cả nhà, cùng em khám phá liền em ${nextTitle} cực kỳ đáng đồng tiền bát gạo này nha!`,
        `Dạ em qua món mới liền cho nóng đây cả nhà mình ơi, nghía sang em ${nextTitle} này bảo đảm cả nhà sẽ thích mê ngay từ cái nhìn đầu tiên luôn!`,
        `Cả nhà săn món trước xong chưa ạ, cùng em đến với tâm điểm tiếp theo của hôm nay nha, chính là em ${nextTitle} siêu tiện lợi này đây ạ!`,
        `Chuyển món luôn cho nóng cả nhà ơi, em ${nextTitle} này phiên live nào lên sóng cũng hết hàng sớm nhất luôn á!`
      ];
      const chosenIdx = getSmartNonRepeatingIndex(rotationTemplates.length, recentRotationIndices, 6);
      return rotationTemplates[chosenIdx];
    }

    // Ngữ cảnh 2: Ghim sản phẩm trực tiếp (khi không có sản phẩm trước hoặc bấm ghim riêng lẻ)
    const directPinTemplates = [
      // Giao lưu & Chiều lòng khách
      `Ui nãy giờ có mấy bác hỏi em này quá trời luôn, đây nha cả nhà ơi, em lên liền em ${nextTitle} theo yêu cầu của mọi người nè!`,
      `Chiều lòng cả nhà mình luôn nha, em đưa ngay em ${nextTitle} lên live đây, ai ưng mắt thì vào xem cận cảnh liền nhé!`,
      `Dạ em thấy mọi người quan tâm em này nhiều ghê, thôi thì lên sóng luôn cho nóng, chính là em ${nextTitle} đây cả nhà ơi!`,
      `Các bác ơi, món mà ai cũng mong chờ hôm nay đã lên rồi nè, chính là em ${nextTitle} cực kỳ hot đây ạ!`,
      `Em chiều ý các chị đẹp luôn nha, lên ngay ${nextTitle} cho cả nhà cùng ngắm, ưng là chốt liền tay chứ đừng chần chừ nha!`,

      // Trải nghiệm thực tế & Khen chất
      `Riêng với em ${nextTitle} này thì em bao chất lượng cho cả nhà luôn nha, nhận hàng về tay mở ra kiểm tra ưng ý mới thanh toán ạ!`,
      `Riêng em ${nextTitle} này chính tay em đã trải nghiệm rồi cả nhà ơi, dùng mê dã man nên hôm nay tự tin khoe với tất cả mọi người luôn!`,
      `Bác nào kỹ tính thích hàng chuẩn xịn thì vào xem ngay em ${nextTitle} này nhé, xuất sắc từ đường kim mũi chỉ đến chất liệu luôn ạ!`,
      `Với em ${nextTitle} này thì em nói thật, không chốt đợt này là tiếc hùi hụi luôn cả nhà ơi, chất lượng vượt xa tầm giá luôn á!`,
      `Ai đang tìm một món vừa bền vừa đẹp thì nghía ngay qua ${nextTitle} nha, khách nhà em mua xong ai cũng quay lại khen nức nở!`,

      // Deal hời & Giới hạn
      `Trời ơi tin được không cả nhà ơi, hôm nay em xin được đúng một ít mã trợ giá sâu cho ${nextTitle} thôi, chớp cơ hội liền tay nha!`,
      `Deal này siêu hời luôn á mọi người ơi, bình thường không bao giờ có giá này đâu, ai đang canh ${nextTitle} thì tranh thủ liền nha!`,
      `Suất ưu đãi cho em ${nextTitle} mở ra có hạn thôi cả nhà ơi, ai nhanh tay thì kịp, chậm một nhịp là hết sạch mã đấy ạ!`,
      `Dạ hôm nay em mở quà bất ngờ cho phiên live nè, em ${nextTitle} này giảm giá chấn động luôn, mọi người lướt qua xem liền nha!`,
      `Ai ưng ${nextTitle} thì nhanh tay thêm vào giỏ liền kẻo hết nhé cả nhà, số lượng ưu đãi hôm nay em còn rất ít thôi ạ!`,

      // Dẫn dắt lôi cuốn & Tự nhiên
      `Cả nhà nhìn lên live cùng em nha, đây chính là tâm điểm hôm nay của nhà em nè, em ${nextTitle} siêu hot đã có mặt rồi đây!`,
      `Lên sóng liền một siêu phẩm chân ái cho cả nhà đây ạ, chính là ${nextTitle}, món này bảo đảm làm xiêu lòng tất cả các bác hôm nay luôn!`,
      `Em lên liền món siêu tiện lợi cho cả nhà mình đây, nhìn qua em ${nextTitle} này là thấy mê liền rồi đúng không cả nhà!`,
      `Bác nào đang xem live thì dừng lại vài giây ngắm em ${nextTitle} này với em nha, xuất sắc không có chỗ nào để chê luôn á!`,
      `Mở màn cho deal đỉnh tiếp theo nha cả nhà ơi, em ${nextTitle} này xứng đáng là món không thể thiếu trong giỏ hàng hôm nay của mọi người luôn!`
    ];
    const chosenIdx = getSmartNonRepeatingIndex(directPinTemplates.length, recentDirectPinIndices, 6);
    return directPinTemplates[chosenIdx];
  }

  async function fetchPlaylistState() {
    try {
      const res = await fetch('/api/playlist');
      if (!res.ok) return;
      const json = await res.json();
      const data = json.data || json;
      if (data && (json.success || json.code === 0)) {
        playlistState = data;
        renderPlaylistUI();

        const runtime = playlistState.runtime || {};
        const products = playlistState.products || [];

        // Detect if active product switched (via TikTok pin, auto-rotation, or manual pin):
        if (runtime.active_product_id) {
          if (lastObservedActiveProductId && lastObservedActiveProductId !== runtime.active_product_id) {
            const prevProd = products.find(p => p.id === lastObservedActiveProductId);
            lastObservedActiveProductId = runtime.active_product_id;
            const prod = products.find(p => p.id === runtime.active_product_id);
            if (prod) {
              const switchReason = runtime.last_switch_reason || 'manual';
              console.log(`[Product Switch] Active product switched to #${prod.index}: ${prod.title} (Reason: ${switchReason})`);

              // Nếu chuyển sản phẩm do khách bình luận hỏi, live.py đã tự trả lời và ghim cho khách
              // Không ngắt lời hoặc phát script lặp lại đè lên câu trả lời của AI
              if (switchReason === 'comment') {
                const displayScript = (prod.script && prod.script.trim()) ? prod.script.trim() : (prod.title || 'Sản phẩm hot');
                updateActivePromptDisplay(displayScript);
                return;
              }

              const transitionText = buildTransitionSentence(prevProd, prod);
              const rawScript = (prod.script && prod.script.trim()) ? prod.script.trim() : (prod.title || 'Sản phẩm hot');
              const cleanedScript = cleanScriptGreeting(rawScript);
              const scriptToSpeak = transitionText ? `${transitionText} ${cleanedScript}` : cleanedScript;

              const chatInput = byId('chat-message');
              if (chatInput) {
                chatInput.value = scriptToSpeak;
              }
              updateActivePromptDisplay(scriptToSpeak);

              // CHỈ tự động phát nếu đây là do Tự Động Xoay Vòng (rotation) chạy theo giờ,
              // KHÔNG tự động phát khi người dùng ghim bằng tay hoặc extension ghim mà chưa bấm nút nói!
              const isAutoRotation = switchReason === 'rotation' && !!(playlistState.config || {}).auto_rotation;
              if (isAutoRotation && state.running) {
                const liveSessionId = String(byId('sessionid')?.value || '').trim();
                showPromptGeneratingIndicator();
                try {
                  const useInstant = (playlistState.config || {}).instant_transition !== false;
                  const immediateSentence = useInstant ? ((transitionText && transitionText.trim()) || (cleanedScript.split(/[.!?]/)[0] || '').trim()) : '';
                  await api('/human', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({
                      text: cleanedScript || scriptToSpeak,
                      immediate_speech: immediateSentence,
                      type: 'chat',
                      interrupt: true,
                      continuous: true,
                      skip_refine: Boolean(cleanedScript && cleanedScript.length > 50),
                      tts: { ref_file: state.speechVoice, speed: state.speechSpeed },
                      sessionid: liveSessionId
                    }),
                  });
                  if (chatInput) chatInput.value = '';
                } catch (err) {
                  hidePromptGeneratingIndicator();
                }
              }
              const icon = switchReason === 'tiktok_pin' ? '📌' : '🔄';
              showPlaylistToast(`Đã chuyển sang SP #${prod.index}: ${prod.title.substring(0, 25)}...`, icon);
            }
          } else if (!lastObservedActiveProductId) {
            lastObservedActiveProductId = runtime.active_product_id;
          }
        }
      }
    } catch (e) {
      console.error("[Playlist State Fetch Error]", e);
    }
  }

  function checkAutoRotationOnSentenceEnd() {
    const runtime = playlistState.runtime || {};
    const config = playlistState.config || {};
    const products = playlistState.products || [];

    if (config.auto_rotation && runtime.rotation_running && runtime.remaining_seconds <= 0 && products.length > 1) {
      console.log("[Auto-Rotation] Sentence ended and timer expired. Gracefully transitioning to next product!");
      nextPlaylistProduct();
      return true;
    }
    return false;
  }

  function updateSmoothLocalTimer() {
    const runtime = playlistState.runtime || {};
    const config = playlistState.config || {};
    const activeTimerEl = byId('pl-active-timer-badge');
    if (!activeTimerEl) return;

    const isRunning = (runtime.rotation_running || !!config.auto_rotation);

    if (isRunning && runtime.remaining_seconds > 0) {
      runtime.remaining_seconds = Math.max(0, runtime.remaining_seconds - 1);
      const m = Math.floor(runtime.remaining_seconds / 60);
      const s = runtime.remaining_seconds % 60;
      activeTimerEl.textContent = `⏳ Còn ${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
      activeTimerEl.style.color = 'var(--primary)';
    } else if (isRunning && runtime.remaining_seconds <= 0) {
      const isSpeaking = byId('reading-state')?.textContent?.includes('ĐANG ĐỌC');
      if (isSpeaking) {
        activeTimerEl.textContent = '⏳ Đang đọc hết câu...';
        activeTimerEl.style.color = '#f59e0b';
      } else {
        activeTimerEl.textContent = '⏳ Đang chuyển SP...';
        activeTimerEl.style.color = '#00f0ff';
        checkAutoRotationOnSentenceEnd();
      }
    } else {
      activeTimerEl.textContent = 'ĐANG DỪNG';
      activeTimerEl.style.color = '#94a3b8';
    }
  }

  function renderPlaylistUI() {
    const runtime = playlistState.runtime || {};
    const config = playlistState.config || {};
    const products = playlistState.products || [];

    // Topbar extension indicator dot
    const topbarDot = byId('topbar-tiktok-ext-dot');
    if (topbarDot) {
      if (runtime.extension_connected) {
        topbarDot.style.background = '#10b981';
        topbarDot.style.boxShadow = '0 0 6px #10b981';
      } else {
        topbarDot.style.background = '#ff0055';
        topbarDot.style.boxShadow = '0 0 6px #ff0055';
      }
    }

    // Modal Status Bar
    const extDot = byId('pl-ext-status-dot');
    const extText = byId('pl-ext-status-text');
    if (extDot && extText) {
      if (runtime.extension_connected) {
        extDot.style.background = '#10b981';
        extDot.style.boxShadow = '0 0 8px #10b981';
        extText.textContent = `TikTok Helper: ĐÃ KẾT NỐI (Tab Live Dashboard) | Pinned: #${runtime.current_pinned_index || '-'}`;
        extText.style.color = '#10b981';
      } else {
        extDot.style.background = '#ff0055';
        extDot.style.boxShadow = '0 0 8px #ff0055';
        extText.textContent = 'TikTok Helper: Chưa kết nối (Hãy mở tab TikTok Live Dashboard và cài Extension)';
        extText.style.color = '#cbd5e1';
      }
    }

    // Switches
    const rotToggle = byId('pl-toggle-rotation');
    if (rotToggle) rotToggle.checked = !!config.auto_rotation;

    const commentPinToggle = byId('pl-toggle-comment-pin');
    if (commentPinToggle) commentPinToggle.checked = config.auto_pin_on_comment !== false;

    const autoRepinToggle = byId('pl-toggle-auto-repin');
    if (autoRepinToggle) autoRepinToggle.checked = config.auto_repin_30s !== false;

    const repinIntervalSelect = byId('pl-recheck-pin-interval');
    if (repinIntervalSelect && config.recheck_pin_interval) {
      repinIntervalSelect.value = String(config.recheck_pin_interval);
    }

    const durSelect = byId('pl-default-duration');
    if (durSelect && config.default_duration) {
      durSelect.value = String(config.default_duration);
    }

    const instantTransToggle = byId('pl-toggle-instant-transition');
    if (instantTransToggle) instantTransToggle.checked = config.instant_transition !== false;

    // Active Live Product Card
    const activeTitleEl = byId('pl-active-title');
    const activePriceEl = byId('pl-active-price');
    const activeIndexEl = byId('pl-active-index');
    const activeTimerEl = byId('pl-active-timer-badge');
    const activePinStatusEl = byId('pl-active-pin-status-badge');

    if (activeTitleEl && activePriceEl && activeIndexEl) {
      if (products.length > 0 && runtime.active_product_id) {
        const activeProd = products.find(p => p.id === runtime.active_product_id);
        if (activeProd) {
          activeTitleEl.textContent = `#${activeProd.index} - ${activeProd.title}`;
          activePriceEl.textContent = activeProd.price || 'Chưa cập nhật giá';
          activeIndexEl.textContent = `Giỏ hàng: #${activeProd.index}`;
        } else {
          activeTitleEl.textContent = runtime.active_product_title || 'Đang phát sản phẩm';
        }

        // Cập nhật huy hiệu trạng thái ghim thực tế trên TikTok
        if (activePinStatusEl) {
          const activeTitle = (activeProd ? activeProd.title : runtime.active_product_title || '').trim().toLowerCase();
          const pinnedTitle = (runtime.current_pinned_title || '').trim().toLowerCase();
          const isPinnedNow = pinnedTitle && (activeTitle.includes(pinnedTitle) || pinnedTitle.includes(activeTitle.substring(0, 15)));

          if (isPinnedNow) {
            activePinStatusEl.textContent = '🟢 ĐÃ GHIM TIKTOK';
            activePinStatusEl.style.background = 'rgba(16, 185, 129, 0.15)';
            activePinStatusEl.style.color = '#10b981';
            activePinStatusEl.title = `TikTok đang ghim: ${runtime.current_pinned_title}`;
          } else {
            activePinStatusEl.textContent = '⏳ CHƯA GHIM (SẼ TỰ GHIM LẠI)';
            activePinStatusEl.style.background = 'rgba(251, 191, 36, 0.15)';
            activePinStatusEl.style.color = '#fbbf24';
            activePinStatusEl.title = 'Sản phẩm này chưa được ghim trên TikTok hoặc TikTok đã hạ ghim sau 30s. Hệ thống sẽ tự động ghim lại!';
          }
        }
      } else {
        activeTitleEl.textContent = 'Chưa có sản phẩm (Bấm LẤY SẢN PHẨM TỪ TIKTOK hoặc Thêm thủ công)';
        activePriceEl.textContent = '-- đ';
        activeIndexEl.textContent = 'Giỏ hàng: 0 SP';
        if (activePinStatusEl) {
          activePinStatusEl.textContent = '--';
          activePinStatusEl.style.background = 'rgba(255, 255, 255, 0.05)';
          activePinStatusEl.style.color = '#94a3b8';
        }
      }
    }

    // Mirror to Center Column PINNED PRODUCT Card
    const pinnedBlock = byId('center-pinned-product-block') || document.querySelector('.center-block.pinned-product-block');
    const mainPinnedTitle = byId('main-pinned-title');
    const mainPinnedPrice = byId('main-pinned-price');
    const mainPinnedStock = byId('main-pinned-stock');
    const mainPinnedIndex = byId('main-pinned-index');
    const mainPinnedStatus = byId('main-pinned-status');
    const mainPinnedImg = byId('main-pinned-img');

    if (pinnedBlock) pinnedBlock.style.display = '';

    const activeProd = (products.length > 0 && runtime.active_product_id)
      ? products.find(p => p.id === runtime.active_product_id)
      : (products.length > 0 ? products[0] : null);

    const emptyRow = byId('main-pinned-empty');
    const pinnedCard = byId('main-pinned-card') || document.querySelector('.pinned-product-card');

    if (activeProd) {
      if (emptyRow) emptyRow.style.display = 'none';
      if (pinnedCard) pinnedCard.style.display = 'flex';
      if (mainPinnedTitle) mainPinnedTitle.textContent = activeProd.title || 'Sản phẩm đang phát';
      if (mainPinnedPrice) mainPinnedPrice.textContent = activeProd.price || '-- đ';
      if (mainPinnedStock) mainPinnedStock.textContent = activeProd.stock ? `Kho: ${activeProd.stock}` : 'Kho: Còn hàng';
      if (mainPinnedIndex) mainPinnedIndex.textContent = `Giỏ #${activeProd.index || 1}`;
      if (mainPinnedImg) {
        mainPinnedImg.src = activeProd.image || activeProd.thumbnail || 'favicon.png';
      }
      if (mainPinnedStatus && activePinStatusEl) {
        mainPinnedStatus.textContent = activePinStatusEl.textContent;
        mainPinnedStatus.style.background = activePinStatusEl.style.background;
        mainPinnedStatus.style.color = activePinStatusEl.style.color;
      }
    } else {
      // Khi chưa có sản phẩm: chỉ hiện dòng "Chưa có sản phẩm"
      if (emptyRow) emptyRow.style.display = 'block';
      if (pinnedCard) pinnedCard.style.display = 'none';
      if (mainPinnedStatus) {
        mainPinnedStatus.textContent = 'CHƯA GHIM';
        mainPinnedStatus.style.background = 'rgba(255, 255, 255, 0.06)';
        mainPinnedStatus.style.color = '#94a3b8';
      }
    }

    if (activeTimerEl) {
      const isRunning = products.length > 0 && (runtime.rotation_running || !!config.auto_rotation);
      if (isRunning && runtime.remaining_seconds > 0) {
        const m = Math.floor(runtime.remaining_seconds / 60);
        const s = runtime.remaining_seconds % 60;
        activeTimerEl.textContent = `⏳ Còn ${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
        activeTimerEl.style.color = 'var(--primary)';
      } else if (isRunning) {
        const isSpeaking = byId('reading-state')?.textContent?.includes('ĐANG ĐỌC');
        if (isSpeaking) {
          activeTimerEl.textContent = '⏳ Đang đọc hết câu...';
          activeTimerEl.style.color = '#f59e0b';
        } else {
          activeTimerEl.textContent = '⏳ Đang chuyển SP...';
          activeTimerEl.style.color = '#00f0ff';
        }
      } else {
        activeTimerEl.textContent = products.length === 0 ? 'CHƯA CÓ SP' : 'ĐANG DỪNG';
        activeTimerEl.style.color = '#94a3b8';
      }
    }

    // Total count
    const countEl = byId('pl-total-count');
    if (countEl) countEl.textContent = products.length;

    // Product Table
    const tbody = byId('pl-table-body');
    if (!tbody) return;

    if (products.length === 0) {
      if (tbody.dataset.renderedSignature !== 'empty') {
        tbody.dataset.renderedSignature = 'empty';
        tbody.innerHTML = `
          <tr>
            <td colspan="6" style="padding: 36px 16px; text-align: center; color: var(--text-muted);">
              <div style="font-size: 32px; margin-bottom: 8px;">🛒</div>
              <div style="font-size: 13.5px; font-weight: 600; color: #cbd5e1; margin-bottom: 6px;">Giỏ hàng hiện đang trống</div>
              <div style="font-size: 12px; color: #94a3b8; line-height: 1.5;">
                Bấm nút <strong style="color: var(--primary);">"📥 LẤY SẢN PHẨM TỪ TIKTOK"</strong> ở góc trên để tự động nạp giỏ hàng từ buổi Live,<br>
                hoặc bấm nút <strong>"➕ Thêm thủ công"</strong> nếu bạn muốn tự tạo kịch bản bán hàng.
              </div>
            </td>
          </tr>
        `;
      }
      return;
    }

    const currentSignature = JSON.stringify({
      prods: products.map(p => ({ id: p.id, index: p.index, title: p.title, price: p.price, duration: p.duration, script: p.script, is_pinned: p.is_pinned })),
      active: runtime.active_product_id
    });
    if (tbody.dataset.renderedSignature !== currentSignature) {
      tbody.dataset.renderedSignature = currentSignature;
      tbody.innerHTML = products.map((p) => {
        const isActive = p.id === runtime.active_product_id;
        const durMin = Math.round((p.duration || 180) / 60);

        let statusBadge = `<span style="background: rgba(255,255,255,0.04); color: #94a3b8; padding: 2px 5px; border-radius: 3px; font-size: 10.5px;">Chờ</span>`;
        if (isActive && p.is_pinned) {
          statusBadge = `<span style="background: rgba(16,185,129,0.18); color: var(--success); font-weight: bold; padding: 2px 5px; border-radius: 3px; font-size: 10.5px; border: 1px solid var(--success); box-shadow: 0 0 6px rgba(16,185,129,0.3);">🟢 ĐANG NÓI & GHIM</span>`;
        } else if (isActive) {
          statusBadge = `<span style="background: rgba(16,185,129,0.18); color: var(--success); font-weight: bold; padding: 2px 5px; border-radius: 3px; font-size: 10.5px; border: 1px solid var(--success); box-shadow: 0 0 6px rgba(16,185,129,0.3);">🟢 ĐANG NÓI</span>`;
        } else if (p.is_pinned) {
          statusBadge = `<span style="background: rgba(0,240,255,0.15); color: #00f0ff; font-weight: bold; padding: 2px 5px; border-radius: 3px; font-size: 10.5px; border: 1px solid #00f0ff;">📌 ĐANG GHIM</span>`;
        }

        return `
          <tr style="border-bottom: 1px solid rgba(255,255,255,0.05); background: ${isActive ? 'rgba(0, 240, 255, 0.08)' : 'transparent'};">
            <td style="padding: 6px 4px; text-align: center; font-weight: bold; color: var(--primary); font-family: var(--font-cyber);">#${p.index}</td>
            <td style="padding: 6px 8px; overflow: hidden; min-width: 0;">
              <div style="font-weight: 600; color: #fff; line-height: 1.25; margin-bottom: 2px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;" title="${escapeHtml(p.title)}">${escapeHtml(p.title)}</div>
              <div style="font-size: 10.5px; color: var(--text-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis;" title="${escapeHtml(p.script || '')}">
                ${escapeHtml(p.script || 'Chưa có kịch bản')}
              </div>
            </td>
            <td style="padding: 6px 6px; color: var(--success); font-weight: 600; white-space: nowrap;">${escapeHtml(p.price || '-')}</td>
            <td style="padding: 6px 4px; text-align: center; color: var(--text-main); white-space: nowrap;">${durMin}p</td>
            <td style="padding: 6px 4px; text-align: center;">${statusBadge}</td>
            <td style="padding: 6px 6px; text-align: right;">
              <div style="display: flex; gap: 4px; justify-content: flex-end; align-items: center;">
                <button type="button" class="btn-cyan" data-action="pin" data-id="${escapeHtml(p.id)}" title="Chỉ ghim lên TikTok & nạp kịch bản vào ô nhập (Chưa nói)" style="height: 25px; padding: 0 7px; font-size: 11px; border-radius: 4px; font-weight: bold; white-space: nowrap; display: inline-flex; align-items: center; gap: 3px; cursor: pointer;">
                  <span>📌</span> <span>Ghim</span>
                </button>
                <button type="button" class="btn-green" data-action="speak" data-id="${escapeHtml(p.id)}" title="Đọc ngay kịch bản của sản phẩm này" style="height: 25px; padding: 0 7px; font-size: 11px; border-radius: 4px; font-weight: bold; white-space: nowrap; display: inline-flex; align-items: center; gap: 3px; cursor: pointer;">
                  <span>🔊</span> <span>Đọc</span>
                </button>
                <button type="button" class="btn-gray" data-action="edit" data-id="${escapeHtml(p.id)}" title="Sửa kịch bản" style="height: 25px; width: 26px; padding: 0; font-size: 12px; border-radius: 4px; display: inline-flex; align-items: center; justify-content: center; cursor: pointer;">
                  ✏️
                </button>
                <button type="button" class="btn-pink" data-action="delete" data-id="${escapeHtml(p.id)}" title="Xóa sản phẩm" style="height: 25px; width: 26px; padding: 0; font-size: 12px; border-radius: 4px; display: inline-flex; align-items: center; justify-content: center; cursor: pointer;">
                  🗑️
                </button>
              </div>
            </td>
          </tr>
        `;
      }).join('');
    }

    // Ensure robust delegated click handling for row action buttons
    if (!tbody.dataset.actionBound) {
      tbody.dataset.actionBound = "1";
      tbody.addEventListener('click', (e) => {
        const btn = e.target.closest('[data-action]');
        if (!btn) return;
        e.preventDefault();
        e.stopPropagation();
        const action = btn.dataset.action;
        const prodId = btn.dataset.id;
        if (!prodId) return;
        if (action === 'pin') {
          switchToProductAndPlay(prodId, true, false);
        } else if (action === 'speak') {
          const autoPinOn = !!(playlistState.config || {}).auto_pin_tiktok;
          switchToProductAndPlay(prodId, autoPinOn, true);
        } else if (action === 'play') {
          switchToProductAndPlay(prodId, true, false);
        } else if (action === 'edit') {
          openEditProductModal(prodId);
        } else if (action === 'delete') {
          deleteProductItem(prodId);
        }
      });
    }
  }

  function showPlaylistToast(msg, icon = '✨') {
    const toast = byId('pl-toast-box');
    const iconEl = byId('pl-toast-icon');
    const msgEl = byId('pl-toast-msg');
    if (toast && msgEl) {
      if (iconEl) iconEl.textContent = icon;
      msgEl.textContent = msg;
      toast.style.display = 'block';
      toast.style.opacity = '1';
      setTimeout(() => {
        toast.style.opacity = '0';
        setTimeout(() => { toast.style.display = 'none'; }, 300);
      }, 2600);
    }
  }

  function showCustomConfirm(title, message, onConfirm, options = {}) {
    confirmCallback = onConfirm;
    const modal = byId('custom-confirm-modal');
    const titleEl = byId('custom-confirm-title');
    const msgEl = byId('custom-confirm-msg');
    const iconEl = byId('custom-confirm-icon');
    const okBtn = byId('custom-confirm-ok-btn');
    const cancelBtn = byId('custom-confirm-cancel-btn');

    if (modal && titleEl && msgEl) {
      titleEl.textContent = title || 'XÁC NHẬN';
      msgEl.textContent = message || 'Bạn có chắc chắn muốn thực hiện hành động này?';
      if (iconEl) iconEl.textContent = options.icon || '⚠️';
      const iconBox = byId('custom-confirm-icon-box');
      if (options.danger) {
        okBtn.style.background = 'linear-gradient(135deg, #ef4444, #dc2626)';
        okBtn.style.boxShadow = '0 0 18px rgba(239, 68, 68, 0.45)';
        if (iconBox) {
          iconBox.style.background = 'rgba(239, 68, 68, 0.12)';
          iconBox.style.borderColor = 'rgba(239, 68, 68, 0.45)';
          iconBox.style.boxShadow = '0 0 24px rgba(239, 68, 68, 0.25)';
        }
      } else {
        okBtn.style.background = 'linear-gradient(135deg, #00f0ff, #0284c7)';
        okBtn.style.boxShadow = '0 0 18px rgba(0, 240, 255, 0.35)';
        if (iconBox) {
          iconBox.style.background = 'rgba(245, 158, 11, 0.12)';
          iconBox.style.borderColor = 'rgba(245, 158, 11, 0.4)';
          iconBox.style.boxShadow = '0 0 24px rgba(245, 158, 11, 0.25)';
        }
      }
      if (okBtn) {
        okBtn.textContent = options.confirmText || 'Xác nhận';
      }
      if (cancelBtn) {
        cancelBtn.textContent = options.cancelText || 'Hủy bỏ';
      }
      modal.style.display = 'flex';
      modal.classList.add('open');
    } else {
    if (window.confirm(message)) {
      if (onConfirm) onConfirm();
    }
  }
}

  function closeCustomConfirm(confirmed = false) {
  const modal = byId('custom-confirm-modal');
  if (modal) {
    modal.style.display = 'none';
    modal.classList.remove('open');
  }
  if (confirmed && confirmCallback) {
    confirmCallback();
  }
  confirmCallback = null;
}

async function requestSyncCartFromTikTok() {
  const btn = byId('btn-modal-sync-cart');
  const originalHtml = btn ? btn.innerHTML : '';
  if (btn) {
    btn.disabled = true;
    btn.innerHTML = '<span>⏳</span> Đang lấy...';
  }

  const isExtConnected = playlistState.runtime && playlistState.runtime.extension_connected;
  if (!isExtConnected) {
    showPlaylistToast('Chưa kết nối Extension! Hãy mở tab Quản lý Live trên TikTok Shop.', '⚠️');
    if (btn) {
      btn.disabled = false;
      btn.innerHTML = originalHtml;
    }
    return;
  }

  const reqStartTime = (Date.now() / 1000) - 0.5;
  showPlaylistToast('Đang quét giỏ hàng từ TikTok Shop Live...', '⏳');

  try {
    await fetch('/api/tiktok/request_sync_cart', { method: 'POST' });

    let attempts = 0;
    const maxAttempts = 15;
    let syncDone = false;

    const checkSyncTimer = setInterval(async () => {
      attempts++;
      try {
        const sRes = await fetch('/api/playlist');
        const sData = await sRes.json();
        const stateData = sData.data || sData;
        if (stateData) {
          playlistState = stateData;
          renderPlaylistUI();
          const runtime = stateData.runtime || {};
          const lastSync = runtime.last_sync_time || 0;

          if (lastSync >= reqStartTime) {
            syncDone = true;
            clearInterval(checkSyncTimer);
            const count = runtime.last_synced_count !== undefined ? runtime.last_synced_count : (stateData.products || []).length;
            if (count > 0) {
              showPlaylistToast(`Đã đồng bộ thành công ${count} sản phẩm từ TikTok Live!`, '✅');
            } else {
              showPlaylistToast('Không tìm thấy sản phẩm trên bảng giỏ hàng TikTok Live.', '⚠️');
            }
            if (btn) {
              btn.disabled = false;
              btn.innerHTML = originalHtml;
            }
            return;
          }
        }
      } catch (err) {
        console.warn("[Polling sync error]", err);
      }

      if (attempts >= maxAttempts) {
        clearInterval(checkSyncTimer);
        if (!syncDone) {
          showPlaylistToast('Không nhận được phản hồi từ TikTok Live. Hãy kiểm tra giỏ hàng đang mở.', '⚠️');
          if (btn) {
            btn.disabled = false;
            btn.innerHTML = originalHtml;
          }
        }
      }
    }, 350);

  } catch (e) {
    console.error("[Request Sync Cart Error]", e);
    showPlaylistToast('Lỗi khi gửi yêu cầu đồng bộ: ' + e, '⚠️');
    if (btn) {
      btn.disabled = false;
      btn.innerHTML = originalHtml;
    }
  }
}

async function switchToProductAndPlay(productId, triggerPin = true, autoSpeak = false) {
  try {
    const prevId = lastObservedActiveProductId;
    lastObservedActiveProductId = productId;
    const res = await fetch('/api/playlist/select', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ id: productId, trigger_pin: triggerPin })
    });
    const json = await res.json();
    const prod = json.product || (json.data && json.data.product);
    if (prod) {
      const prevProd = (playlistState.products || []).find(p => p.id === prevId);
      const transitionText = buildTransitionSentence(prevProd, prod);
      if (transitionText) {
        window.pendingTransitionSentence = transitionText;
      }

      const rawScript = (prod.script && prod.script.trim()) ? prod.script.trim() : (prod.title || 'Sản phẩm hot');
      const scriptContent = cleanScriptGreeting(rawScript);
      const speechToPlay = transitionText ? `${transitionText} ${scriptContent}` : scriptContent;
      const chatInput = byId('chat-message');
      if (chatInput) {
        chatInput.value = speechToPlay;
      }

      // Always update the active prompt UI display
      updateActivePromptDisplay(speechToPlay);

      if (autoSpeak) {
        if (state.running) {
          const liveSessionId = String(byId('sessionid')?.value || '').trim();
          showPromptGeneratingIndicator();
          try {
            const useInstant = (playlistState.config || {}).instant_transition !== false;
            const immediateSentence = useInstant ? ((transitionText && transitionText.trim()) || (scriptContent.split(/[.!?]/)[0] || '').trim()) : '';
            await api('/human', {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({
                text: scriptContent || speechToPlay,
                immediate_speech: immediateSentence,
                type: 'chat',
                interrupt: true,
                continuous: true,
                skip_refine: Boolean(scriptContent && scriptContent.length > 50),
                tts: { ref_file: state.speechVoice, speed: state.speechSpeed },
                sessionid: liveSessionId
              }),
            });
            if (chatInput) chatInput.value = '';
          } catch (err) {
            hidePromptGeneratingIndicator();
            console.error('[Avatar speak error]', err);
          }
          showPlaylistToast(`Đang đọc kịch bản SP #${prod.index}...`, '⚡');
        } else {
          showPlaylistToast(`Đã nạp kịch bản SP #${prod.index} & Ghim TikTok! (Bấm "BẮT ĐẦU" để Avatar đọc live)`, '✅');
        }
      } else {
        showPlaylistToast(`Đã ghim SP #${prod.index} lên TikTok & nạp kịch bản vào ô nhập! (Bấm "GỬI NỘI DUNG" để nói)`, '📌');
      }

      const tb = byId('pl-table-body');
      if (tb) tb.dataset.renderedSignature = '';
      await fetchPlaylistState();

      // Lên lịch tải lại sau 1.6s và 3.2s khi Extension hoàn tất ghim và gửi lại vị trí mới từ TikTok
      setTimeout(async () => {
        const t = byId('pl-table-body');
        if (t) t.dataset.renderedSignature = '';
        await fetchPlaylistState();
      }, 1600);
      setTimeout(async () => {
        const t = byId('pl-table-body');
        if (t) t.dataset.renderedSignature = '';
        await fetchPlaylistState();
      }, 3200);
    }
  } catch (e) {
    console.error("[Switch Product and Play Error]", e);
    showPlaylistToast("Lỗi khi nạp kịch bản: " + e, "⚠️");
  }
}

function openPlaylistModal() {
  const modal = byId('playlist-modal');
  if (modal) {
    modal.style.display = 'flex';
    modal.classList.add('open');
  }
  fetchPlaylistState();
}

function closePlaylistModal() {
  const modal = byId('playlist-modal');
  if (modal) {
    modal.style.display = 'none';
    modal.classList.remove('open');
  }
}

async function nextPlaylistProduct() {
  try {
    const res = await fetch('/api/playlist/next', { method: 'POST' });
    const json = await res.json();
    const prod = json.product || (json.data && json.data.product);
    if (prod) {
      switchToProductAndPlay(prod.id, true, false);
    }
  } catch (e) {
    console.error("[Next Product Error]", e);
  }
}

async function pinCurrentActiveProduct() {
  const runtime = playlistState.runtime || {};
  if (!runtime.active_product_id) {
    showPlaylistToast("Vui lòng chọn sản phẩm trước khi ghim.", "⚠️");
    return;
  }
  try {
    await switchToProductAndPlay(runtime.active_product_id, true, false);
  } catch (e) {
    showPlaylistToast("Lỗi khi ghim sản phẩm: " + e, "⚠️");
  }
}

async function onToggleRotation(enabled) {
  try {
    await fetch('/api/playlist/toggle_rotation', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ enabled: enabled })
    });
    await fetch('/api/playlist/config', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ auto_pin_tiktok: enabled })
    });
    showPlaylistToast(enabled ? "Đã BẬT tự động chuyển kịch bản & ghim TikTok" : "Đã TẮT tự động xoay vòng", "⚙️");
    await fetchPlaylistState();
  } catch (e) {
    console.error("[Toggle Rotation Error]", e);
  }
}

async function onToggleCommentPin(enabled) {
  try {
    await fetch('/api/playlist/config', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ auto_pin_on_comment: enabled })
    });
    showPlaylistToast(
      enabled ? "Đã BẬT tự động Ghim & đổi kịch bản khi khách hỏi SP!" : "Đã TẮT tự động Ghim khi khách hỏi SP",
      enabled ? "🎯" : "ℹ️"
    );
    await fetchPlaylistState();
  } catch (e) {
    console.error("[Toggle Comment Pin Error]", e);
  }
}

async function onDurationChange(val) {
  try {
    await fetch('/api/playlist/config', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ default_duration: parseInt(val, 10) })
    });
    showPlaylistToast(`Đã đặt thời lượng: ${Math.round(val / 60)} phút/SP`, "⏳");
    await fetchPlaylistState();
  } catch (e) {
    console.error("[Change Duration Error]", e);
  }
}

async function onToggleAutoRepin(enabled) {
  try {
    await fetch('/api/playlist/config', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ auto_repin_30s: enabled })
    });
    showPlaylistToast(
      enabled ? "Đã BẬT tự động kiểm tra và ghim lại mỗi 30s" : "Đã TẮT tự động ghim lại 30s",
      enabled ? "🔄" : "ℹ️"
    );
    await fetchPlaylistState();
  } catch (e) {
    console.error("[Toggle Auto Repin Error]", e);
  }
}

async function onRepinIntervalChange(val) {
  try {
    await fetch('/api/playlist/config', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ recheck_pin_interval: parseInt(val, 10) })
    });
    showPlaylistToast(`Đã đặt chu kỳ kiểm tra ghim: ${val}s`, "⏱️");
    await fetchPlaylistState();
  } catch (e) {
    console.error("[Change Repin Interval Error]", e);
  }
}

async function onToggleInstantTransition(enabled) {
  try {
    await fetch('/api/playlist/config', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ instant_transition: enabled })
    });
    showPlaylistToast(
      enabled ? "Đã BẬT phát câu mở đầu tức thì (Zero-Latency)!" : "Đã TẮT phát câu mở đầu tức thì",
      enabled ? "⚡" : "ℹ️"
    );
    await fetchPlaylistState();
  } catch (e) {
    console.error("[Toggle Instant Transition Error]", e);
  }
}

function openEditProductModal(editId = null) {
  const modal = byId('edit-product-modal');
  const titleEl = byId('edit-prod-modal-title');
  const form = byId('edit-product-form');
  if (!modal || !form) return;

  if (editId) {
    const prod = (playlistState.products || []).find(p => p.id === editId);
    if (prod) {
      if (titleEl) titleEl.textContent = `✏️ Sửa sản phẩm #${prod.index}: ${prod.title}`;
      if (byId('ep-id')) byId('ep-id').value = prod.id || '';
      if (byId('ep-index')) byId('ep-index').value = prod.index || 1;
      if (byId('ep-title')) byId('ep-title').value = prod.title || '';
      if (byId('ep-price')) byId('ep-price').value = prod.price || '';
      if (byId('ep-link')) byId('ep-link').value = prod.link || '';
      if (byId('ep-duration')) byId('ep-duration').value = prod.duration || 180;
      if (byId('ep-script')) byId('ep-script').value = prod.script || '';
      if (byId('ep-autopin')) byId('ep-autopin').checked = prod.auto_pin !== false;
      if (byId('ep-enabled')) byId('ep-enabled').checked = prod.enabled !== false;
    }
  } else {
    if (titleEl) titleEl.textContent = "➕ Thêm sản phẩm vào giỏ hàng";
    form.reset();
    if (byId('ep-id')) byId('ep-id').value = '';
    if (byId('ep-index')) byId('ep-index').value = (playlistState.products || []).length + 1;
    const defDur = (playlistState.config && playlistState.config.default_duration) ? playlistState.config.default_duration : 180;
    if (byId('ep-duration')) byId('ep-duration').value = defDur;
    if (byId('ep-autopin')) byId('ep-autopin').checked = true;
    if (byId('ep-enabled')) byId('ep-enabled').checked = true;
  }
  modal.style.display = 'flex';
  modal.classList.add('open');
}

function closeEditProductModal() {
  const modal = byId('edit-product-modal');
  if (modal) {
    modal.style.display = 'none';
    modal.classList.remove('open');
  }
}

async function saveProductForm() {
  const id = byId('ep-id')?.value;
  const index = parseInt(byId('ep-index')?.value, 10) || 1;
  const title = byId('ep-title')?.value.trim();
  const price = byId('ep-price')?.value.trim();
  const link = byId('ep-link')?.value.trim();
  const duration = parseInt(byId('ep-duration')?.value, 10) || 180;
  const script = byId('ep-script')?.value.trim();
  const auto_pin = byId('ep-autopin')?.checked ?? true;
  const enabled = byId('ep-enabled')?.checked ?? true;

  if (!title) {
    showPlaylistToast("Vui lòng nhập tên sản phẩm.", "⚠️");
    return;
  }

  const item = {
    id: id || undefined,
    index: index,
    title: title,
    price: price,
    link: link,
    duration: duration,
    script: script,
    auto_pin: auto_pin,
    enabled: enabled
  };

  try {
    if (id) {
      const list = (playlistState.products || []).map(p => p.id === id ? { ...p, ...item } : p);
      await fetch('/api/playlist/save', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ products: list })
      });
    } else {
      await fetch('/api/playlist/add', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(item)
      });
    }
    closeEditProductModal();
    showPlaylistToast("Đã lưu sản phẩm thành công!", "✅");
    const tbody = byId('pl-table-body');
    if (tbody) tbody.dataset.renderedSignature = '';
    await fetchPlaylistState();
  } catch (e) {
    showPlaylistToast("Lỗi khi lưu sản phẩm: " + e, "⚠️");
  }
}

async function deleteProductItem(id) {
  const prod = (playlistState.products || []).find(p => p.id === id);
  const name = prod ? `#${prod.index} - ${prod.title}` : 'sản phẩm này';

  showCustomConfirm(
    "XÁC NHẬN XÓA SẢN PHẨM",
    `Bạn có chắc chắn muốn xóa ${name} khỏi giỏ hàng live không?`,
    async () => {
      try {
        const res = await fetch('/api/playlist/delete', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ id: id })
        });
        const data = await res.json();
        if (data && data.success) {
          showPlaylistToast("Đã xóa sản phẩm khỏi giỏ hàng", "🗑️");
        } else {
          showPlaylistToast("Không thể xóa sản phẩm", "⚠️");
        }
        const tbody = byId('pl-table-body');
        if (tbody) tbody.dataset.renderedSignature = '';
        await fetchPlaylistState();
      } catch (e) {
        showPlaylistToast("Lỗi khi xóa sản phẩm: " + e, "⚠️");
      }
    }
  );
}

async function clearAllPlaylistProducts() {
  const prods = playlistState.products || [];
  if (prods.length === 0) {
    showPlaylistToast("Giỏ hàng hiện tại đang trống.", "ℹ️");
    return;
  }
  showCustomConfirm(
    "XÁC NHẬN XÓA TOÀN BỘ GIỎ HÀNG",
    `Bạn có chắc chắn muốn xóa toàn bộ ${prods.length} sản phẩm trong giỏ hàng không?`,
    async () => {
      try {
        await fetch('/api/playlist/clear', { method: 'POST' });
        showPlaylistToast("Đã xóa sạch giỏ hàng!", "🗑️");
        const tbody = byId('pl-table-body');
        if (tbody) tbody.dataset.renderedSignature = '';
        await fetchPlaylistState();
      } catch (e) {
        showPlaylistToast("Lỗi khi xóa giỏ hàng: " + e, "⚠️");
      }
    }
  );
}


async function generateAiScriptForProduct() {
  const title = byId('ep-title').value.trim();
  const price = byId('ep-price').value.trim();
  const link = byId('ep-link').value.trim();
  const btn = byId('btn-ai-gen-script');

  if (!title && !link) {
    showPlaylistToast("Vui lòng nhập Tên hoặc Link sản phẩm trước.", "⚠️");
    return;
  }

  const originalHtml = btn ? btn.innerHTML : '';
  if (btn) {
    btn.disabled = true;
    btn.innerHTML = '⏳ Đang viết...';
  }

  try {
    const res = await fetch('/api/playlist/generate_script', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ title: title, price: price, link: link })
    });
    const data = await res.json();
    if ((data.success || data.code === 0) && data.script) {
      byId('ep-script').value = data.script;
      if (data.title && !byId('ep-title').value.trim()) {
        byId('ep-title').value = data.title;
      }
      if (data.price && !byId('ep-price').value.trim()) {
        byId('ep-price').value = data.price;
      }
      showPlaylistToast("AI đã tạo kịch bản thành công!", "✨");
    }
  } catch (e) {
    console.error("[AI Generate Script Error]", e);
    showPlaylistToast("Lỗi khi tạo kịch bản: " + e, "⚠️");
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.innerHTML = originalHtml;
    }
  }
}

async function fetchExtensionInfo() {
  try {
    const res = await fetch('/api/tiktok/extension_info');
    if (res.ok) {
      const data = await res.json();
      const info = data.data || data;
      if (info && info.path) {
        const pathEl = byId('ext-folder-path');
        if (pathEl) {
          pathEl.textContent = info.path;
          pathEl.setAttribute('data-path', info.path);
          if (!info.exists) {
            pathEl.style.color = '#ffaa00';
          } else {
            pathEl.style.color = '#00f0ff';
          }
        }
      }
    }
  } catch (e) {
    console.warn("[Extension Info Fetch Error]", e);
  }
}

async function openExtensionFolderOnDisk() {
  try {
    const res = await fetch('/api/tiktok/open_extension_folder', { method: 'POST' });
    const data = await res.json();
    if (data.success || data.code === 0) {
      showPlaylistToast("Đã mở thư mục Extension trong File Explorer!", "📂");
    } else {
      showPlaylistToast(data.error || data.msg || "Không tìm thấy thư mục Extension trên máy.", "⚠️");
    }
  } catch (e) {
    showPlaylistToast("Lỗi khi mở thư mục: " + e, "⚠️");
  }
}

function copyExtensionPath() {
  const pathEl = byId('ext-folder-path');
  const pathText = pathEl ? (pathEl.getAttribute('data-path') || pathEl.textContent.trim()) : '';
  if (pathText && !pathText.includes('Đang lấy')) {
    navigator.clipboard.writeText(pathText).then(() => {
      const btnText = byId('btn-copy-ext-text');
      if (btnText) {
        btnText.textContent = "Đã chép!";
        setTimeout(() => { btnText.textContent = "Sao chép"; }, 2000);
      }
      showPlaylistToast("Đã sao chép đường dẫn Extension vào Clipboard!", "📋");
    }).catch(() => {
      showPlaylistToast("Không thể tự động sao chép. Bạn hãy bôi đen dòng chữ để chép.", "⚠️");
    });
  } else {
    showPlaylistToast("Đang tải đường dẫn, vui lòng thử lại sau 1 giây.", "⏳");
  }
}


function startPlaylistStatePolling() {
  fetchPlaylistState();
  if (!playlistPollingInterval) {
    playlistPollingInterval = setInterval(fetchPlaylistState, 1500);
  }
  if (!localPlaylistTimerInterval) {
    localPlaylistTimerInterval = setInterval(updateSmoothLocalTimer, 1000);
  }
}


function openExtensionGuideModal() {
  const modal = byId('extension-guide-modal');
  if (modal) {
    modal.style.display = 'flex';
    modal.classList.add('open');
  }
  fetchExtensionInfo();
}

function closeExtensionGuideModal() {
  const modal = byId('extension-guide-modal');
  if (modal) {
    modal.style.display = 'none';
    modal.classList.remove('open');
  }
}

// Expose playlist functions to window
window.openPlaylistModal = openPlaylistModal;
window.closePlaylistModal = closePlaylistModal;
window.openEditProductModal = openEditProductModal;
window.closeEditProductModal = closeEditProductModal;
window.saveProductForm = saveProductForm;
window.deleteProductItem = deleteProductItem;
window.clearAllPlaylistProducts = clearAllPlaylistProducts;
window.nextPlaylistProduct = nextPlaylistProduct;
window.pinCurrentActiveProduct = pinCurrentActiveProduct;
window.requestSyncCartFromTikTok = requestSyncCartFromTikTok;
window.switchToProductAndPlay = switchToProductAndPlay;
window.onToggleRotation = onToggleRotation;
window.onToggleCommentPin = onToggleCommentPin;
window.onToggleAutoRepin = onToggleAutoRepin;
window.onRepinIntervalChange = onRepinIntervalChange;
window.onToggleInstantTransition = onToggleInstantTransition;
window.onDurationChange = onDurationChange;
window.generateAiScriptForProduct = generateAiScriptForProduct;
window.openExtensionGuideModal = openExtensionGuideModal;
window.closeExtensionGuideModal = closeExtensionGuideModal;
window.openExtensionFolderOnDisk = openExtensionFolderOnDisk;
window.copyExtensionPath = copyExtensionPath;
window.closeCustomConfirm = closeCustomConfirm;
window.showCustomConfirm = showCustomConfirm;

// Voice Clone & Management Modal exports
window.openVoiceModal = openVoiceModal;
window.closeVoiceModal = closeVoiceModal;
window.switchVoiceModalTab = switchVoiceModalTab;
window.setVoiceSourceMode = setVoiceSourceMode;
window.handleVoiceFileSelected = handleVoiceFileSelected;
window.toggleVoiceRecording = toggleVoiceRecording;
window.submitVoiceClone = submitVoiceClone;
window.testVoiceAudio = testVoiceAudio;
window.selectVoiceDirectly = selectVoiceDirectly;
window.deleteVoiceDirectly = deleteVoiceDirectly;
window.loadVoiceCatalog = loadVoiceCatalog;

// Initialization
bindEvents();
setupSpeechControls();
updateLiveIndicator('ready');
refreshResourceStatus({ autoOpen: true }).then(() => {
  if (state.resourcesInstalled) loadAvatars();
});
// Presence must start independently of the first profile request. A transient
// profile failure must not make an opened app invisible to the admin dashboard.
startPresenceHeartbeat();
loadProfile().then(() => {
  startProfilePolling();
  restorePendingInvoicePolling();
}).catch((error) => {
  console.error('Initial profile load failed:', error);
});
startPlaylistStatePolling();
initializeDesktopVersion();
window.setTimeout(initializeDesktopVersion, 4000);
}) ();

