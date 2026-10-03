"""Metrics publication must reconcile with complete recorded trial evidence."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("build_site", Path(__file__).resolve().parents[1] / "build_site.py")
site = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(site)


def seal(value):
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()
    return {**value, "integrity_sha256": hashlib.sha256(encoded).hexdigest()}


class SiteEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.directory = self.root / "benchmarks/results/example"
        self.directory.mkdir(parents=True)
        plan = seal({"trials": [{"trial_id": "one"}, {"trial_id": "two"}]})
        (self.directory / "plan.json").write_text(json.dumps(plan))
        self.summary = {"status": "COMPLETE", "plan_sha256": plan["integrity_sha256"], "variants": {}}
        for index, variant in enumerate(("protocol", "compact_combined")):
            tokens = 100 if index == 0 else 60
            record = seal({"trial_id": ("one", "two")[index], "variant": variant,
                           "plan_sha256": plan["integrity_sha256"], "input_tokens": tokens,
                           "output_tokens": 10, "accepted": True, "acceptance": {"passed": True}})
            folder = self.directory / f"trial-{index:04d}"
            folder.mkdir()
            (folder / "record.json").write_text(json.dumps(record))
            self.summary["variants"][variant] = {"attempts": 1, "accepted": 1,
                                                "correct_before_policy": 1,
                                                "total_tokens_including_failures": tokens + 10}
        self.save_summary()

    def save_summary(self):
        (self.directory.parent / "example-summary.json").write_text(json.dumps(self.summary))

    def test_complete_evidence_produces_ratio_without_dropping_output_tokens(self):
        with patch.object(site, "ROOT", self.root):
            summary = site.workflow_report("example")
        self.assertAlmostEqual(site.workflow_reduction(summary, "compact_combined"), 100 * (1 - 70 / 110))

    def test_stale_summary_cannot_advertise_better_token_usage(self):
        self.summary["variants"]["compact_combined"]["total_tokens_including_failures"] = 1
        self.save_summary()
        with patch.object(site, "ROOT", self.root), self.assertRaises(AssertionError):
            site.workflow_report("example")

    def test_missing_attempt_and_modified_record_reject_publication(self):
        record = self.directory / "trial-0000/record.json"
        original = record.read_text()
        record.write_text(original.replace('"input_tokens": 100', '"input_tokens": 1'))
        with patch.object(site, "ROOT", self.root), self.assertRaises(AssertionError):
            site.workflow_report("example")
        record.unlink()
        with patch.object(site, "ROOT", self.root), self.assertRaises(AssertionError):
            site.workflow_report("example")

    def test_incomplete_study_cannot_be_promoted(self):
        self.summary["status"] = "INCOMPLETE"
        self.save_summary()
        with patch.object(site, "ROOT", self.root), self.assertRaises(AssertionError):
            site.workflow_report("example")
