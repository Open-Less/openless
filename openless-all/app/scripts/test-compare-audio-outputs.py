"""Negative controls for the before/after PCM comparison gate."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class ComparisonTests(unittest.TestCase):
    def test_exact_difference_and_truncation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            before, after = root / "before", root / "after"
            for folder in (before, after):
                folder.mkdir()
                (folder / "results.json").write_text(json.dumps([
                    {"file": "signal.pcm", "seconds": 1, "samples": 3}
                ]))
                (folder / "signal.pcm").write_bytes(b"\x00\x00\x01\x00\xff\xff")
            report = root / "report.json"
            command = [sys.executable, str(Path(__file__).with_name("compare-audio-outputs.py")),
                       str(before), str(after), "--report", str(report)]
            def run():
                return subprocess.run(command, capture_output=True, text=True).returncode
            self.assertEqual(run(), 0)
            (after / "signal.pcm").write_bytes(b"\x01\x00\x01\x00\xff\xff")
            self.assertEqual(run(), 1)
            self.assertEqual(json.loads(report.read_text())["max_error_lsb"], 1)
            (after / "signal.pcm").write_bytes(b"\x00\x00")
            self.assertNotEqual(run(), 0)


if __name__ == "__main__":
    unittest.main()
