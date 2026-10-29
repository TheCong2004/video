import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("avatar_prepare", Path(__file__).with_name("avatar_prepare.py"))
adapter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adapter)


class AvatarPreparationTests(unittest.TestCase):
    def test_probe_enforces_duration_and_dimensions_before_decoding(self):
        def probe(duration=10, width=1080, height=1920):
            return {"format": {"duration": duration}, "streams": [{"codec_type": "video", "width": width, "height": height}]}
        self.assertEqual(adapter.validate_probe(probe())["duration_seconds"], 10)
        for data in [probe(0), probe(31), probe("NaN"), probe(width=9000), {"streams": []}]:
            with self.subTest(data=data), self.assertRaises(ValueError):
                adapter.validate_probe(data)

    def test_readiness_is_explicit_and_does_not_claim_lipsync_or_voice(self):
        with patch.object(adapter.importlib.util, "find_spec", return_value=None), patch.object(adapter.shutil, "which", return_value=None):
            result = adapter.readiness()
        self.assertFalse(result["ready"])
        self.assertIn("torch", result["missing"])
        self.assertFalse(result["capabilities"]["realtime_lipsync"])
        self.assertFalse(result["capabilities"]["voice_clone"])


if __name__ == "__main__":
    unittest.main()
