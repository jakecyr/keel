import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import json_lookup


class JsonLookupBenchmarkTests(unittest.TestCase):
    def test_harness_keeps_an_independent_last_key_assertion(self):
        code = json_lookup.harness("static KText k_alloc(size_t len) { return value; }", "", 3, 7)
        self.assertIn('K_TEXT("/key2")', code)
        self.assertIn('K_TEXT("2")', code)
        self.assertIn('i < 7', code)
        self.assertIn('requested_bytes += len + 1', code)
        self.assertIn('if (!value.ok || !k_equal', code)

    def test_evidence_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.json"
            path.write_text("original evidence")
            result = subprocess.run([sys.executable, str(Path(json_lookup.__file__)), "--baseline", "HEAD",
                                     "--output", str(path)], capture_output=True, text=True, timeout=5)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(path.read_text(), "original evidence")

    def test_invalid_workload_does_not_create_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.json"
            result = subprocess.run([sys.executable, str(Path(json_lookup.__file__)), "--baseline", "HEAD",
                                     "--members", "0", "--output", str(path)], capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 2)
            self.assertFalse(path.exists())
