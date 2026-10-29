import math
import numpy as np
# import tensorflow as tf
from scipy import signal
from scipy.io import wavfile
from .hparams import hparams as hp

def load_wav(path, sr):
    source_sr, wav = wavfile.read(path)
    if wav.ndim > 1:
        wav = wav.mean(axis=1)
    if np.issubdtype(wav.dtype, np.integer):
        wav = wav.astype(np.float32) / max(abs(np.iinfo(wav.dtype).min), np.iinfo(wav.dtype).max)
    else:
        wav = wav.astype(np.float32)
    if source_sr != sr:
        divisor = math.gcd(int(source_sr), int(sr))
        wav = signal.resample_poly(wav, sr // divisor, source_sr // divisor).astype(np.float32)
    return wav

def save_wav(wav, path, sr):
    wav *= 32767 / max(0.01, np.max(np.abs(wav)))
    #proposed by @dsmiller
    wavfile.write(path, sr, wav.astype(np.int16))

def save_wavenet_wav(wav, path, sr):
    save_wav(wav, path, sr)

def preemphasis(wav, k, preemphasize=True):
    if preemphasize:
        return signal.lfilter([1, -k], [1], wav)
    return wav

def inv_preemphasis(wav, k, inv_preemphasize=True):
    if inv_preemphasize:
        return signal.lfilter([1], [1, -k], wav)
    return wav

def get_hop_size():
    hop_size = hp.hop_size
    if hop_size is None:
        assert hp.frame_shift_ms is not None
        hop_size = int(hp.frame_shift_ms / 1000 * hp.sample_rate)
    return hop_size

def linearspectrogram(wav):
    D = _stft(preemphasis(wav, hp.preemphasis, hp.preemphasize))
    S = _amp_to_db(np.abs(D)) - hp.ref_level_db
    
    if hp.signal_normalization:
        return _normalize(S)
    return S

def melspectrogram(wav):
    D = _stft(preemphasis(wav, hp.preemphasis, hp.preemphasize))
    S = _amp_to_db(_linear_to_mel(np.abs(D))) - hp.ref_level_db
    
    if hp.signal_normalization:
        return _normalize(S)
    return S

def _lws_processor():
    import lws
    return lws.lws(hp.n_fft, get_hop_size(), fftsize=hp.win_size, mode="speech")

def _stft(y):
    if hp.use_lws:
        return _lws_processor(hp).stft(y).T
    else:
        # Equivalent to librosa.stft(center=True), implemented directly with
        # NumPy so frozen customer builds do not depend on Numba's JIT runtime.
        n_fft = int(hp.n_fft)
        win_length = int(hp.win_size or n_fft)
        hop_length = int(get_hop_size())
        y = np.asarray(y, dtype=np.float32)
        y = np.pad(y, n_fft // 2, mode="constant")
        if y.size < n_fft:
            y = np.pad(y, (0, n_fft - y.size))
        frame_count = 1 + (y.size - n_fft) // hop_length
        shape = (frame_count, n_fft)
        strides = (y.strides[0] * hop_length, y.strides[0])
        frames = np.lib.stride_tricks.as_strided(y, shape=shape, strides=strides)
        window = signal.get_window("hann", win_length, fftbins=True).astype(np.float32)
        if win_length < n_fft:
            padded = np.zeros(n_fft, dtype=np.float32)
            offset = (n_fft - win_length) // 2
            padded[offset:offset + win_length] = window
            window = padded
        return np.fft.rfft(frames * window, n=n_fft, axis=1).T

##########################################################
#Those are only correct when using lws!!! (This was messing with Wavenet quality for a long time!)
def num_frames(length, fsize, fshift):
    """Compute number of time frames of spectrogram
    """
    pad = (fsize - fshift)
    if length % fshift == 0:
        M = (length + pad * 2 - fsize) // fshift + 1
    else:
        M = (length + pad * 2 - fsize) // fshift + 2
    return M


def pad_lr(x, fsize, fshift):
    """Compute left and right padding
    """
    M = num_frames(len(x), fsize, fshift)
    pad = (fsize - fshift)
    T = len(x) + 2 * pad
    r = (M - 1) * fshift + fsize - T
    return pad, pad + r
##########################################################
#Librosa correct padding
def librosa_pad_lr(x, fsize, fshift):
    return 0, (x.shape[0] // fshift + 1) * fshift - x.shape[0]

# Conversions
_mel_basis = None

def _linear_to_mel(spectogram):
    global _mel_basis
    if _mel_basis is None:
        _mel_basis = _build_mel_basis()
    return np.dot(_mel_basis, spectogram)

def _build_mel_basis():
    assert hp.fmax <= hp.sample_rate // 2
    # Slaney mel filter bank, matching librosa.filters.mel defaults.
    def hz_to_mel(frequencies):
        frequencies = np.asarray(frequencies, dtype=np.float64)
        mels = frequencies / (200.0 / 3.0)
        log_region = frequencies >= 1000.0
        mels[log_region] = 15.0 + np.log(frequencies[log_region] / 1000.0) / (np.log(6.4) / 27.0)
        return mels

    def mel_to_hz(mels):
        mels = np.asarray(mels, dtype=np.float64)
        frequencies = (200.0 / 3.0) * mels
        log_region = mels >= 15.0
        frequencies[log_region] = 1000.0 * np.exp((np.log(6.4) / 27.0) * (mels[log_region] - 15.0))
        return frequencies

    fft_frequencies = np.linspace(0, hp.sample_rate / 2, 1 + hp.n_fft // 2)
    mel_edges = mel_to_hz(np.linspace(hz_to_mel([hp.fmin])[0],
                                      hz_to_mel([hp.fmax])[0], hp.num_mels + 2))
    ramps = mel_edges[:, None] - fft_frequencies[None, :]
    weights = np.zeros((hp.num_mels, fft_frequencies.size), dtype=np.float32)
    for index in range(hp.num_mels):
        lower = -ramps[index] / (mel_edges[index + 1] - mel_edges[index])
        upper = ramps[index + 2] / (mel_edges[index + 2] - mel_edges[index + 1])
        weights[index] = np.maximum(0, np.minimum(lower, upper))
    weights *= (2.0 / (mel_edges[2:hp.num_mels + 2] - mel_edges[:hp.num_mels]))[:, None]
    return weights

def _amp_to_db(x):
    min_level = np.exp(hp.min_level_db / 20 * np.log(10))
    return 20 * np.log10(np.maximum(min_level, x))

def _db_to_amp(x):
    return np.power(10.0, (x) * 0.05)

def _normalize(S):
    if hp.allow_clipping_in_normalization:
        if hp.symmetric_mels:
            return np.clip((2 * hp.max_abs_value) * ((S - hp.min_level_db) / (-hp.min_level_db)) - hp.max_abs_value,
                           -hp.max_abs_value, hp.max_abs_value)
        else:
            return np.clip(hp.max_abs_value * ((S - hp.min_level_db) / (-hp.min_level_db)), 0, hp.max_abs_value)
    
    assert S.max() <= 0 and S.min() - hp.min_level_db >= 0
    if hp.symmetric_mels:
        return (2 * hp.max_abs_value) * ((S - hp.min_level_db) / (-hp.min_level_db)) - hp.max_abs_value
    else:
        return hp.max_abs_value * ((S - hp.min_level_db) / (-hp.min_level_db))

def _denormalize(D):
    if hp.allow_clipping_in_normalization:
        if hp.symmetric_mels:
            return (((np.clip(D, -hp.max_abs_value,
                              hp.max_abs_value) + hp.max_abs_value) * -hp.min_level_db / (2 * hp.max_abs_value))
                    + hp.min_level_db)
        else:
            return ((np.clip(D, 0, hp.max_abs_value) * -hp.min_level_db / hp.max_abs_value) + hp.min_level_db)
    
    if hp.symmetric_mels:
        return (((D + hp.max_abs_value) * -hp.min_level_db / (2 * hp.max_abs_value)) + hp.min_level_db)
    else:
        return ((D * -hp.min_level_db / hp.max_abs_value) + hp.min_level_db)
