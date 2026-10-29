# SPDX-License-Identifier: AGPL-3.0-or-later
"""Factory adapter: real media, continuous narration, timed captions, gentle motion.

Invoked by the owned Rust worker, never through a shell. No substitute speech or
stock images are generated when an upstream service fails.
"""
import json
import math
import pathlib
import subprocess
import sys

for stream in (sys.stdout, sys.stderr):
    if hasattr(stream, 'reconfigure'):
        stream.reconfigure(encoding='utf-8', errors='replace')


def run(args, timeout=600):
    result = subprocess.run([str(a) for a in args], stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            timeout=timeout, creationflags=0x08000000 if sys.platform == 'win32' else 0)
    if result.returncode:
        raise RuntimeError(result.stderr.decode('utf-8', errors='replace')[-3000:])
    return result.stdout


def probe(ffmpeg, path):
    executable = pathlib.Path(ffmpeg).with_name('ffprobe.exe' if sys.platform == 'win32' else 'ffprobe')
    return json.loads(run([executable, '-v', 'error', '-show_streams', '-show_format', '-of', 'json', path]))


def duration(info):
    value = float(info.get('format', {}).get('duration', 0))
    if not math.isfinite(value) or value <= 0 or value > 600:
        raise ValueError('Thời lượng âm thanh phải từ 0 đến 600 giây.')
    return value


def stamp(seconds):
    ms = round(seconds * 1000)
    return f'{ms//3600000:02}:{ms//60000%60:02}:{ms//1000%60:02},{ms%1000:03}'


def estimated_captions(text, seconds):
    words = text.split()
    rows = []
    for start in range(0, len(words), 10):
        end = min(start + 10, len(words))
        rows.append(f'{len(rows)+1}\n{stamp(seconds*start/len(words))} --> {stamp(seconds*end/len(words))}\n'
                    + ' '.join(words[start:end]) + '\n')
    return '\n'.join(rows)


def plan(paths, seconds, audio_seconds=0):
    if not paths or len(paths) > 100 or not math.isfinite(seconds) or not .5 <= seconds <= 600:
        raise ValueError('Chọn 1–100 tư liệu và thời lượng 0.5–600 giây.')
    total = max(len(paths) * seconds, audio_seconds + .35 if audio_seconds else 0)
    if total > 600:
        raise ValueError('Video Factory tối đa 600 giây; giảm tư liệu hoặc thời lượng.')
    # Even one product photo gets several camera beats rather than a frozen hold.
    count = max(len(paths), math.ceil(total / 4.5))
    return [(paths[i % len(paths)], total / count) for i in range(count)]


def render(config, root):
    root = pathlib.Path(root).resolve()
    ffmpeg = config['ffmpeg']
    paths = [pathlib.Path(p).resolve(strict=True) for p in config['paths']]
    w, h = config['size']
    if (w, h) not in [(720,1280), (1280,720), (720,720), (1080,1920)]:
        raise ValueError('Kích thước xuất không hợp lệ.')
    mode = config['audio_mode']
    text = config['script'].strip()
    if len(text) > 12000 or mode not in [0, 1, 2]:
        raise ValueError('Kịch bản quá dài hoặc chế độ âm thanh không hợp lệ.')
    audio = None
    audio_seconds = 0
    subtitle = root / 'captions.srt'
    if mode == 0:
        if not text:
            raise ValueError('Nhập kịch bản lời đọc trước khi dựng.')
        audio = root / 'narration.mp3'
        (root / 'script.txt').write_text(text, encoding='utf-8')
        voice = ['vi-VN-HoaiMyNeural', 'vi-VN-NamMinhNeural'][config['voice_index']]
        print('Đang tạo giọng Việt và phụ đề theo lời đọc…', flush=True)
        run([sys.executable, '-m', 'edge_tts', '--voice', voice,
             '--file', root / 'script.txt', '--write-media', audio,
             '--write-subtitles', subtitle], timeout=120)
        if not subtitle.is_file() or '-->' not in subtitle.read_text(encoding='utf-8-sig'):
            raise RuntimeError('Dịch vụ chưa trả phụ đề theo lời đọc; chưa xuất video.')
    elif mode == 1:
        audio = pathlib.Path(config['audio_path']).resolve(strict=True)
    if audio:
        info = probe(ffmpeg, audio)
        if not any(s['codec_type'] == 'audio' for s in info['streams']):
            raise ValueError('File giọng đọc không có âm thanh.')
        audio_seconds = duration(info)
        if mode == 1 and text:
            subtitle.write_text(estimated_captions(text, audio_seconds), encoding='utf-8')
    target = config.get('target_duration')
    if target is not None:
        target = float(target)
        if not math.isfinite(target) or not 1 <= target <= 600:
            raise ValueError('Thời lượng yêu cầu phải trong 1–600 giây.')
        if not audio or abs(audio_seconds - target) > max(1, min(2, target * .05)):
            raise ValueError(f'Lời đọc dài {audio_seconds:.1f}s, yêu cầu {target:.1f}s. Cần sửa kịch bản hoặc chọn file giọng phù hợp; không kéo dài bằng âm thanh im lặng.')
    shots = plan(paths, target / len(paths) if target is not None else config['seconds'],
                 0 if target is not None else audio_seconds)
    manifest = []
    source_audio = False
    stills = {'.png', '.jpg', '.jpeg', '.webp', '.bmp'}
    for i, (path, seconds) in enumerate(shots):
        print(f'Đang dựng cảnh {i+1}/{len(shots)}…', flush=True)
        image = path.suffix.lower() in stills
        info = probe(ffmpeg, path)
        has_audio = any(s['codec_type'] == 'audio' for s in info['streams']) and mode == 2
        source_audio |= has_audio
        frames = math.ceil(seconds * 30)
        z = f'1.0+0.06*on/{max(1,frames-1)}' if i % 2 == 0 else f'1.06-0.06*on/{max(1,frames-1)}'
        # The whole product stays visible over a soft full-frame background.
        # Start foreground at 92% so the 6% camera move does not clip the edges.
        if image:
            graph = (f'[0:v]format=rgba,split[bg][fg];[bg]scale={w}:{h}:force_original_aspect_ratio=increase,'
                     f'crop={w}:{h},boxblur=20:2[back];[fg]scale={int(w*.92)//2*2}:{int(h*.92)//2*2}:'
                     f'force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=black@0,'
                     f'format=rgba[front];[back][front]overlay=(W-w)/2:(H-h)/2,scale={w*2}:{h*2},'
                     f"zoompan=z='{z}':x='iw/2-iw/zoom/2':y='ih/2-ih/zoom/2':d={frames}:s={w}x{h}:fps=30")
            input_args = ['-i', path]
        else:
            graph = (f'[0:v]split[bg][fg];[bg]scale={w}:{h}:force_original_aspect_ratio=increase,'
                     f'crop={w}:{h},boxblur=20:2[back];[fg]scale={w}:{h}:force_original_aspect_ratio=decrease,'
                     f'setsar=1[front];[back][front]overlay=(W-w)/2:(H-h)/2,fps=30')
            input_args = ['-stream_loop', '-1', '-i', path]
        fade = min(.18, seconds / 4)
        graph += f',setsar=1,fade=t=in:d={fade},fade=t=out:st={seconds-fade}:d={fade},format=yuv420p[v]'
        args = [ffmpeg, '-hide_banner', '-nostdin', '-n', *input_args]
        if has_audio:
            graph += ';[0:a]aresample=48000,apad[a]'
        else:
            graph += ';anullsrc=r=48000:cl=stereo[a]'
        name = f'shot-{i:04}.mp4'
        run([*args, '-filter_complex', graph, '-map', '[v]', '-map', '[a]', '-t', seconds,
             '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '19', '-threads', '2',
             '-filter_complex_threads', '1', '-c:a', 'aac', '-ar', '48000', '-ac', '2', root / name])
        manifest.append({'path': str(path), 'duration': seconds, 'file': name, 'motion': 'push-in' if i%2==0 else 'pull-out'})
    if mode == 2 and not source_audio:
        raise ValueError('Tư liệu không có âm thanh. Chọn tạo giọng Việt hoặc file giọng đọc.')
    (root / 'assembly.txt').write_text(''.join(f"file '{s['file']}'\n" for s in manifest), encoding='utf-8')
    total = sum(s['duration'] for s in manifest)
    args = [ffmpeg, '-hide_banner', '-nostdin', '-n', '-f', 'concat', '-safe', '1', '-i', root / 'assembly.txt']
    if audio:
        args += ['-i', audio, '-map', '0:v:0', '-map', '1:a:0', '-af', 'loudnorm=I=-16:TP=-1.5:LRA=11,apad']
    else:
        args += ['-map', '0:v:0', '-map', '0:a:0']
    if subtitle.is_file():
        import shutil
        shutil.copyfile(config['font'], root / 'font.ttf')
        # FFmpeg subtitles paths are relative to this owned working directory.
        args += ['-vf', "subtitles=captions.srt:fontsdir=.:force_style='FontName=Hanken Grotesk,FontSize=20,Outline=2,Shadow=0,MarginV=32,Alignment=2'",
                 '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '19', '-threads', '2']
    else:
        args += ['-c:v', 'copy']
    args += ['-c:a', 'aac', '-b:a', '192k', '-t', total, '-movflags', '+faststart', root / 'film.mp4']
    run(args)
    (root / 'edit-manifest.json').write_text(json.dumps({'shots': manifest, 'audioMode': mode,
        'duration': total, 'captionTiming': 'speech-boundaries' if mode==0 else 'estimated' if text and audio else 'none'}, ensure_ascii=False, indent=2), encoding='utf-8')
    for shot in manifest:
        (root / shot['file']).unlink()
    print('Hoàn tất video có chuyển động và âm thanh.', flush=True)


if __name__ == '__main__':
    try:
        config_path = pathlib.Path(sys.argv[1]).resolve()
        render(json.loads(config_path.read_text(encoding='utf-8')), config_path.parent)
    except Exception as error:
        print(f'Không xuất video: {error}', file=sys.stderr)
        sys.exit(1)
