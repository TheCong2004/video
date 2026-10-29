"""Real FFmpeg checks for the Factory adapter; no network in the default suite."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

REPO = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('factory_edit', REPO / 'src/crates/concat-studio/src/factory_edit.py')
edit = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(edit)


class Planning(unittest.TestCase):
    def test_voice_extends_visuals_and_single_image_gets_camera_beats(self):
        shots = edit.plan(['product.jpg'], 5, 13)
        self.assertEqual(len(shots), 3)
        self.assertAlmostEqual(sum(s for _, s in shots), 13.35)

    def test_invalid_duration_and_limits(self):
        for seconds in [float('nan'), float('inf'), 0, 601]:
            with self.assertRaises(ValueError):
                edit.plan(['a'], seconds)
        with self.assertRaises(ValueError):
            edit.plan([], 5)
        with self.assertRaises(ValueError):
            edit.plan(['a', 'b'], 600)

    def test_external_audio_captions_are_bounded(self):
        text = ' '.join(f'word{i}' for i in range(24))
        srt = edit.estimated_captions(text, 12)
        self.assertIn('00:00:00,000 --> 00:00:05,000', srt)
        self.assertIn('00:00:12,000', srt)
        self.assertEqual(srt.count('-->'), 3)


@unittest.skipUnless(os.environ.get('CONCAT_TEST_FFMPEG'), 'Set CONCAT_TEST_FFMPEG for real media tests')
class Media(unittest.TestCase):
    def test_explicit_duration_rejects_silent_padding_and_renders_matching_voice(self):
        ffmpeg=os.environ['CONCAT_TEST_FFMPEG']
        with tempfile.TemporaryDirectory(prefix='factory-edit-',dir=REPO/'.runtime') as temp:
            root=Path(temp)
            image=root/'product.png'
            voice=root/'voice.wav'
            edit.run([ffmpeg,'-v','error','-f','lavfi','-i','testsrc2=s=160x160','-frames:v',1,'-threads',1,image])
            edit.run([ffmpeg,'-v','error','-f','lavfi','-i','sine=frequency=440:duration=1',voice])
            config={'ffmpeg':ffmpeg,'paths':[str(image)],'size':[720,720],'seconds':.5,
                    'audio_mode':1,'voice_index':0,'audio_path':str(voice),'script':'','target_duration':180}
            with self.assertRaisesRegex(ValueError,'180.0s'):
                edit.render(config,root)
            self.assertFalse((root/'film.mp4').exists())
            config['target_duration']=1
            previous=Path.cwd()
            try:
                os.chdir(root)
                edit.render(config,root)
            finally:
                os.chdir(previous)
            self.assertAlmostEqual(edit.duration(edit.probe(ffmpeg,root/'film.mp4')),1,delta=.15)

    def test_tts_failure_never_creates_a_completed_silent_video(self):
        with tempfile.TemporaryDirectory(prefix='factory-edit-',dir=REPO/'.runtime') as temp:
            root=Path(temp)
            image=root/'product.png'
            image.write_bytes(b'owned fixture')
            config={'ffmpeg':os.environ['CONCAT_TEST_FFMPEG'],'paths':[str(image)],'size':[720,1280],
                    'seconds':5,'audio_mode':0,'voice_index':0,'script':'Lời đọc thật'}
            with patch.object(edit,'run',side_effect=RuntimeError('NoAudioReceived')):
                with self.assertRaisesRegex(RuntimeError,'NoAudioReceived'):
                    edit.render(config,root)
            self.assertFalse((root/'film.mp4').exists())

    def test_video_source_keeps_real_sound_and_motion(self):
        ffmpeg=os.environ['CONCAT_TEST_FFMPEG']
        with tempfile.TemporaryDirectory(prefix='factory-edit-',dir=REPO/'.runtime') as temp:
            root=Path(temp)
            clip=root/'source.mp4'
            edit.run([ffmpeg,'-v','error','-f','lavfi','-i','testsrc2=s=320x240:r=30',
                      '-f','lavfi','-i','sine=frequency=660:duration=1','-t',1,'-c:v','libx264',
                      '-threads',1,'-c:a','aac',clip])
            config={'ffmpeg':ffmpeg,'paths':[str(clip)],'size':[720,720],'seconds':1,
                    'audio_mode':2,'voice_index':0,'script':''}
            edit.render(config,root)
            output=root/'film.mp4'
            info=edit.probe(ffmpeg,output)
            self.assertTrue(any(s['codec_type']=='audio' for s in info['streams']))
            samples=edit.run([ffmpeg,'-v','error','-i',output,'-f','s16le','-ac',1,'-ar',8000,'-'])
            import array
            values=array.array('h',samples)
            self.assertGreater(max(abs(x) for x in values),100)

    def test_motion_full_product_continuous_voice_and_caption_burn_in(self):
        ffmpeg = os.environ['CONCAT_TEST_FFMPEG']
        with tempfile.TemporaryDirectory(prefix='factory-edit-', dir=REPO / '.runtime') as temp:
            root = Path(temp)
            # Grid makes camera movement measurable; a plain solid image would not.
            image = root / 'product.png'
            edit.run([ffmpeg, '-v', 'error', '-f', 'lavfi', '-i', 'testsrc2=s=400x400', '-frames:v', 1, '-threads', 1, image])
            voice = root / 'voice.wav'
            # Known fixture: two distinct frequency sections catch per-shot restarts.
            edit.run([ffmpeg, '-v', 'error', '-f', 'lavfi', '-i', 'sine=frequency=440:duration=2.5',
                      '-f', 'lavfi', '-i', 'sine=frequency=880:duration=2.5', '-filter_complex',
                      '[0:a][1:a]concat=n=2:v=0:a=1[a]', '-map', '[a]', voice])
            config = {'ffmpeg': ffmpeg, 'font': str(REPO/'src/crates/concat-text/fonts/HankenGrotesk-Regular.ttf'),
                      'paths': [str(image)], 'size': [720,1280], 'seconds': .5, 'script': 'Sản phẩm thật. Kiểm tra giọng và phụ đề.',
                      'audio_mode': 1, 'voice_index': 0, 'audio_path': str(voice)}
            previous = Path.cwd()
            try:
                os.chdir(root)
                edit.render(config, root)
            finally:
                os.chdir(previous)
            output = root/'film.mp4'
            info = edit.probe(ffmpeg, output)
            self.assertTrue(any(s['codec_type']=='audio' for s in info['streams']))
            self.assertAlmostEqual(edit.duration(info), 5.35, delta=.15)
            video = next(s for s in info['streams'] if s['codec_type']=='video')
            self.assertEqual((video['width'],video['height']), (720,1280))
            def frame(t):
                return edit.run([ffmpeg, '-v','error','-ss',t,'-i',output,'-frames:v',1,'-f','rawvideo','-pix_fmt','gray','-vf','scale=72:128','-'])
            a,b=frame(.5),frame(1.5)
            self.assertGreater(sum(abs(x-y) for x,y in zip(a,b))/len(a), 1)
            for position, frequency in [(1,440),(3.5,880)]:
                samples=edit.run([ffmpeg,'-v','error','-ss',position,'-i',output,'-t',.3,'-f','s16le','-ac',1,'-ar',8000,'-'])
                import array
                values=array.array('h',samples)
                crossings=sum(x<0<=y for x,y in zip(values,values[1:]))/.3
                self.assertAlmostEqual(crossings,frequency,delta=20)
            self.assertTrue((root/'captions.srt').exists())
            self.assertFalse((root/'shot-0000.mp4').exists())

    def test_image_cannot_be_reported_as_audible_using_source_mode(self):
        ffmpeg=os.environ['CONCAT_TEST_FFMPEG']
        with tempfile.TemporaryDirectory(prefix='factory-edit-',dir=REPO/'.runtime') as temp:
            root=Path(temp)
            image=root/'product.png'
            edit.run([ffmpeg,'-v','error','-f','lavfi','-i','color=red:s=80x80','-frames:v',1,'-threads',1,image])
            config={'ffmpeg':ffmpeg,'paths':[str(image)],'size':[720,720],'seconds':.5,
                    'audio_mode':2,'script':'','voice_index':0}
            with self.assertRaisesRegex(ValueError,'không có âm thanh'):
                edit.render(config,root)
            self.assertFalse((root/'film.mp4').exists())


if __name__=='__main__':
    unittest.main()
