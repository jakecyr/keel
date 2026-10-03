"""No model/network calls. Synthetic ledgers test arithmetic, never prove performance."""
import copy
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from contextlib import redirect_stdout
import io

from agent_eval import CONDITIONS, acceptance, digest, evaluate, load_suite, make_plan, seal
from codex_pilot import extract_usage, instructions
from measure import corpus, main as measurement_main, measure, percentile, runtime_source, summarize


class GateTests(unittest.TestCase):
    def setUp(self):
        self.suite = load_suite()
        self.plan = make_plan(self.suite, "synthetic-test-model", 2)
        tasks = {task["id"]: task for task in self.suite["tasks"]}
        self.records = []
        for trial in self.plan["trials"]:
            task = tasks[trial["task_id"]]
            source = "synthetic: this ledger tests evaluation arithmetic, not a real agent"
            events = [{"type": "turn.completed", "usage": {"input_tokens": 100, "cached_input_tokens": 20, "output_tokens": 10}}]
            self.records.append(seal({**trial, "plan_sha256": self.plan["integrity_sha256"],
                                      "model": self.plan["model"], "budget": self.plan["budget"],
                                      "source": source, "acceptance": {"accepted": True,
                                      "evaluator": "independent_native_assertions_v1",
                                      "oracle_sha256": digest(task["held_out_cases"]),
                                      "source_sha256": hashlib.sha256(source.encode()).hexdigest()},
                                      "elapsed_seconds": 10, "input_tokens": 100, "cached_input_tokens": 20,
                                      "output_tokens": 10, "events": events, "events_sha256": digest(events),
                                      "condition_isolation_verified": True,
                                      "costs": {"inference_usd": 0.5 if trial["condition"] == "keel_protocol" else 1,
                                                "tool_usd": 0.1, "metering_evidence": "SYNTHETIC unit-test fixture"}}))

    def change(self, record, **values):
        record.update(values)
        record.pop("integrity_sha256", None)
        record.update(seal(record))

    def test_valid_complete_paired_experiment(self):
        report = evaluate(self.plan, self.suite, self.records)
        self.assertEqual(report["status"], "PASS")
        self.assertAlmostEqual(report["measured_cost_reduction"], 1 - 0.6 / 1.1)

    def test_failed_attempt_costs_remain_in_numerator(self):
        for record in self.records:
            if record["repeat"] == 0:
                self.change(record, acceptance={**record["acceptance"], "accepted": False})
        report = evaluate(self.plan, self.suite, self.records)
        keel = report["conditions"]["keel_protocol"]
        self.assertEqual(keel["attempts"], 6)
        self.assertEqual(keel["accepted"], 3)
        self.assertAlmostEqual(keel["cost_per_accepted_change_usd"], 1.2)

    def test_cheaper_by_abandonment_fails(self):
        record = next(r for r in self.records if r["condition"] == "keel_protocol")
        self.change(record, acceptance={**record["acceptance"], "accepted": False})
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "FAIL")

    def test_equal_acceptance_rate_does_not_hide_paired_regression(self):
        for record in self.records:
            failed = record["repeat"] == 0 and ((record["condition"] == "keel_protocol" and record["task_id"] == "deadline") or
                                                (record["condition"].startswith("existing") and record["task_id"] == "clamp"))
            if failed:
                self.change(record, acceptance={**record["acceptance"], "accepted": False})
        report = evaluate(self.plan, self.suite, self.records)
        self.assertEqual(report["status"], "FAIL")
        self.assertEqual(report["paired_acceptance_regressions"], ["deadline:0"])

    def test_strongest_existing_baseline_used(self):
        for record in self.records:
            if record["condition"] == "existing_improved":
                self.change(record, costs={"inference_usd": 0.1, "tool_usd": 0.1, "metering_evidence": "synthetic"})
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "FAIL")

    def test_missing_trial_is_invalid(self):
        self.assertEqual(evaluate(self.plan, self.suite, self.records[:-1])["status"], "INVALID")

    def test_duplicate_replacing_trial_is_invalid(self):
        self.records[-1] = self.records[0]
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "INVALID")

    def test_model_mismatch_is_invalid(self):
        self.change(self.records[0], model="different-model")
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "INVALID")

    def test_budget_mismatch_is_invalid(self):
        self.change(self.records[0], budget={"wall_seconds": 999, "total_tokens": 32000})
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "INVALID")

    def test_changed_suite_is_invalid(self):
        self.suite["tasks"][0]["held_out_cases"][0]["expected"] = True
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "INVALID")

    def test_record_tampering_is_invalid(self):
        self.records[0]["costs"]["inference_usd"] = 0
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "INVALID")

    def test_source_tampering_with_resealed_record_is_invalid(self):
        self.change(self.records[0], source="different source")
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "INVALID")

    def test_event_tampering_with_resealed_record_is_invalid(self):
        self.change(self.records[0], events=[])
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "INVALID")

    def test_usage_totals_must_match_raw_events(self):
        self.change(self.records[0], input_tokens=50)
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "INVALID")

    def test_missing_usage_evidence_is_unknown(self):
        self.change(self.records[0], events=[], events_sha256=digest([]))
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "UNKNOWN")

    def test_fractional_token_counts_are_invalid(self):
        self.change(self.records[0], input_tokens=100.0)
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "INVALID")

    def test_missing_cost_is_unknown_not_zero(self):
        self.change(self.records[0], costs={"inference_usd": None, "tool_usd": 0})
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "UNKNOWN")

    def test_missing_metering_evidence_is_unknown(self):
        self.change(self.records[0], costs={"inference_usd": 1, "tool_usd": 1})
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "UNKNOWN")

    def test_no_condition_enforcement_is_unknown(self):
        self.change(self.records[0], condition_isolation_verified=False)
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "UNKNOWN")

    def test_zero_denominator_is_unknown(self):
        for record in self.records:
            self.change(record, acceptance={**record["acceptance"], "accepted": False})
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "UNKNOWN")

    def test_zero_cost_baseline_is_unknown(self):
        for record in self.records:
            self.change(record, costs={"inference_usd": 0, "tool_usd": 0, "metering_evidence": "synthetic"})
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "UNKNOWN")

    def test_negative_cost_is_invalid(self):
        self.change(self.records[0], costs={"inference_usd": -1, "tool_usd": 0, "metering_evidence": "synthetic"})
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "INVALID")

    def test_nonfinite_cost_is_invalid(self):
        self.records[0]["costs"]["inference_usd"] = float("nan")
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "INVALID")

    def test_nonboolean_acceptance_is_invalid(self):
        self.change(self.records[0], acceptance={**self.records[0]["acceptance"], "accepted": "true"})
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "INVALID")

    def test_over_budget_accepted_trial_is_invalid(self):
        self.change(self.records[0], input_tokens=100000)
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "INVALID")

    def test_changed_plan_is_invalid_even_when_resealed(self):
        self.plan["trials"].pop()
        self.plan.pop("integrity_sha256")
        self.plan = seal(self.plan)
        self.assertEqual(evaluate(self.plan, self.suite, self.records)["status"], "INVALID")

    def test_malformed_input_is_invalid(self):
        self.assertEqual(evaluate(None, self.suite, self.records)["status"], "INVALID")
        self.assertEqual(evaluate(self.plan, self.suite, [None])["status"], "INVALID")
        with self.assertRaises(ValueError):
            make_plan(self.suite, 42, 1)
        with self.assertRaises(ValueError):
            make_plan(self.suite, "model", True)

    def test_gate_cli_nonzero_on_unknown(self):
        for record in self.records:
            self.change(record, costs=None)
        with tempfile.TemporaryDirectory() as directory:
            plan, records = Path(directory) / "plan.json", Path(directory) / "records.json"
            plan.write_text(json.dumps(self.plan))
            records.write_text(json.dumps(self.records))
            result = subprocess.run([sys.executable, str(Path(__file__).with_name("agent_eval.py")), "gate", "--plan", str(plan), "--results", str(records)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertEqual(json.loads(result.stdout)["status"], "UNKNOWN")


class MeasurementTests(unittest.TestCase):
    def test_strict_measurement_gate_requires_every_target(self):
        with tempfile.TemporaryDirectory() as directory:
            for status, expected in (("PASS", 0), ("FAIL", 1), ("UNKNOWN: unavailable evidence", 2)):
                report = {"compiler_binary_unchanged": True, "languages": {},
                          "web_example_native_test": {"failures": 0},
                          "engineering_targets": {"target": status},
                          "service": {"returncode": 0, "cache_behavior_verified": True,
                                      "first_check": {"returncode": 0},
                                      "exact_source_cache_hits": {"failures": 0},
                                      "unique_body_edit_cache_misses": {"failures": 0}}}
                with patch("measure.local_benchmarks", return_value=report), patch.object(sys, "argv", ["measure.py", "--output", str(Path(directory) / "result.json"), "--strict"]), redirect_stdout(io.StringIO()):
                    self.assertEqual(measurement_main(), expected)

    def test_nearest_rank_percentile(self):
        self.assertEqual(percentile(list(range(1, 101)), 95), 95)
        with self.assertRaises(ValueError):
            percentile([], 95)

    def test_failed_samples_are_retained(self):
        report = summarize([{"returncode": 1, "elapsed_ms": 100}, {"returncode": 0, "elapsed_ms": 5}])
        self.assertEqual(report["failures"], 1)
        self.assertEqual(len(report["samples"]), 2)

    def test_ten_thousand_line_corpus(self):
        for language in ("keel", "rust", "c"):
            self.assertGreaterEqual(len(corpus(language).splitlines()), 10000)

    def test_process_timeout_is_failure(self):
        report = measure([sys.executable, "-c", "import time; time.sleep(30)"], Path.cwd(), timeout=0.1)
        self.assertTrue(report["timed_out"])
        self.assertNotEqual(report["returncode"], 0)

    def test_usage_counts_all_turns(self):
        report = extract_usage([{"type": "turn.completed", "usage": {"input_tokens": 10, "cached_input_tokens": 5, "output_tokens": 2}},
                                {"type": "turn.completed", "usage": {"input_tokens": 20, "cached_input_tokens": 8, "output_tokens": 3}}])
        self.assertEqual(report, {"input_tokens": 30, "cached_input_tokens": 13, "output_tokens": 5})

    def test_missing_usage_is_unknown(self):
        self.assertIsNone(extract_usage([])["input_tokens"])
        self.assertIsNone(extract_usage([{"type": "turn.completed", "usage": {"input_tokens": 1}}])["input_tokens"])

    def test_held_out_oracle_not_in_prompt(self):
        for task in load_suite()["tasks"]:
            for condition in CONDITIONS:
                prompt = instructions(task, condition)
                self.assertNotIn("held_out_cases", prompt)
                self.assertNotIn("9223372036854775807", prompt)

    @unittest.skipUnless(shutil.which("cc"), "C toolchain required")
    def test_independent_acceptance_rejects_bug_and_accepts_fix(self):
        task = load_suite()["tasks"][0]
        buggy = acceptance(task, task["c"], "c", None)
        repaired = acceptance(task, task["c"].replace("<=", "<"), "c", None)
        self.assertFalse(buggy["accepted"])
        self.assertTrue(repaired["accepted"])

    @unittest.skipUnless(shutil.which("cc"), "C toolchain required")
    def test_baseline_json_tool_structural_edit_and_stale_rejection(self):
        task = load_suite()["tasks"][0]
        helper = Path(__file__).with_name("fixture_tools.py").resolve()
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory)
            source = workspace / "solution.c"
            source.write_text(task["c"])
            (workspace / "public_task.json").write_text(json.dumps({k: v for k, v in task.items() if k not in ("held_out_cases", "c", "keel")}))
            inspect = subprocess.run([sys.executable, str(helper), "inspect"], cwd=workspace, capture_output=True, text=True, check=True)
            request = {"base_revision": json.loads(inspect.stdout)["revision"], "target": "fn:is_fresh", "operation": "replace_body", "source": "{ return now < deadline; }"}
            (workspace / "request.json").write_text(json.dumps(request))
            command = [sys.executable, str(helper), "edit", "--request", "request.json"]
            edited = subprocess.run(command, cwd=workspace, capture_output=True, text=True)
            self.assertEqual(edited.returncode, 0, edited.stderr)
            accepted_source = source.read_text()
            self.assertIn("now < deadline", accepted_source)
            rejected = subprocess.run(command, cwd=workspace, capture_output=True, text=True)
            self.assertNotEqual(rejected.returncode, 0)
            self.assertEqual(source.read_text(), accepted_source)


if __name__ == "__main__":
    unittest.main()
