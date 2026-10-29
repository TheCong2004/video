###############################################################################
#  配置解析 — CLI 参数 + YAML 配置
###############################################################################

import argparse
import json
import os


def str_or_int(value):
    """尝试转换为 int，失败则返回 str"""
    try:
        return int(value)
    except ValueError:
        return value


def parse_args():
    """解析命令行参数"""
    parser = argparse.ArgumentParser(description="LiveTalking Digital Human Server")

    # ─── 音频 ──────────────────────────────────────────────────────────
    parser.add_argument('--fps', type=int, default=25, help="video fps, must be 25")
    parser.add_argument('-l', type=int, default=10)
    parser.add_argument('-m', type=int, default=8)
    parser.add_argument('-r', type=int, default=10)

    # ─── 画面 ──────────────────────────────────────────────────────────
    # parser.add_argument('--W', type=int, default=450, help="GUI width")
    # parser.add_argument('--H', type=int, default=450, help="GUI height")

    # ─── 数字人模型 ────────────────────────────────────────────────────
    parser.add_argument('--model', type=str, default='wav2lip',
                        help="avatar model: musetalk/wav2lip/ultralight")
    parser.add_argument('--avatar_id', type=str, default='mau10',
                        help="avatar id in data/avatars")
    parser.add_argument('--batch_size', type=int, default=8, help="infer batch")
    parser.add_argument('--modelres', type=int, default=192)
    parser.add_argument('--modelfile', type=str, default='')

    # ─── 自定义动作和多形象 ────────────────────────────────────────────
    parser.add_argument('--customvideo_config', type=str, default='',
                        help="custom action json")

    # ─── TTS ───────────────────────────────────────────────────────────
    parser.add_argument('--tts', type=str, default='edgetts',
                        help="tts plugin: edgetts/gpt-sovits/cosyvoice/fishtts/f5api/tencent/doubao/indextts2/azuretts/qwentts")
    parser.add_argument('--REF_FILE', type=str, default="vi-VN-HoaiMyNeural",
                        help="参考文件名或语音模型ID")
    parser.add_argument('--REF_TEXT', type=str, default=None)
    parser.add_argument('--TTS_SERVER', type=str, default='http://127.0.0.1:9880')

    # ─── 传输 ─────────────────────────────────────────────────────────
    parser.add_argument('--transport', type=str, default='webrtc',
                        help="output: rtcpush/webrtc/rtmp/virtualcam")
    parser.add_argument('--push_url', type=str,
                        default='http://localhost:1985/rtc/v1/whip/?app=live&stream=livestream')
    parser.add_argument('--max_session', type=int, default=1)
    parser.add_argument('--listenport', type=int, default=8010,
                        help="web listen port")
    parser.add_argument('--camera_index', type=int, default=None,
                        help="Camera/Webcam device index (default: auto-detect)")

    opt = parser.parse_args()

    # ─── Load config.json defaults if available ───────────────────────
    cfg_file = os.path.join(os.getcwd(), 'config.json')
    if os.path.exists(cfg_file):
        try:
            with open(cfg_file, 'r', encoding='utf-8') as f:
                cfg_data = json.load(f)
                if 'port' in cfg_data and opt.listenport == 8010:
                    opt.listenport = int(cfg_data['port'])
                if 'model' in cfg_data and opt.model == 'wav2lip':
                    opt.model = str(cfg_data['model'])
                if 'avatar_id' in cfg_data and opt.avatar_id in ('mau10', 'wav2lip256_avatar1'):
                    opt.avatar_id = str(cfg_data['avatar_id'])
                if 'camera_index' in cfg_data and cfg_data['camera_index'] is not None:
                    opt.camera_index = int(cfg_data['camera_index'])
                if 'batch_size' in cfg_data:
                    opt.batch_size = int(cfg_data['batch_size'])
                if 'tts' in cfg_data and opt.tts == 'edgetts':
                    opt.tts = str(cfg_data['tts'])
                if 'transport' in cfg_data and opt.transport == 'webrtc':
                    opt.transport = str(cfg_data['transport'])
        except Exception:
            pass

    # ─── 后处理 ────────────────────────────────────────────────────────
    opt.customopt = []
    if opt.customvideo_config:
        with open(opt.customvideo_config, 'r') as f:
            opt.customopt = json.load(f)

    return opt
