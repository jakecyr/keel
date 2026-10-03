"""Offline methodology tests. Never starts Codex or calls a model/network."""
import copy
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

from agent_eval import digest, seal
from efficiency import (VARIANTS, assertion_failure, context_packet, event_metrics, file_hash, make_plan, policy_reasons,
                        preflight, prompt_for, report, save_new, score_artifact, tests_only,
                        run_trial, validate_plan, write_workspace)
from efficiency_tasks import SUITE

KEEL = Path(__file__).resolve().parent.parent / "target/debug/keel"


class ExperimentTests(unittest.TestCase):
    def setUp(self):
        self.plan = make_plan("synthetic", repeats=2, task_ids=["boundary", "unique"], compiler_sha256="synthetic")
        tasks = {t["id"]: t for t in SUITE["tasks"]}
        self.records = []
        for trial in self.plan["trials"]:
            task = tasks[trial["task_id"]]
            tokens = 100 if trial["variant"] == "protocol" else 50
            events = [{"type": "turn.completed", "usage": {"input_tokens": tokens, "cached_input_tokens": 20, "output_tokens": 5}}]
            artifacts = {"solution.keel": "synthetic arithmetic fixture, not agent output"}
            record = {**trial, "plan_sha256": self.plan["integrity_sha256"], "model": "synthetic",
                      "compiler_sha256": "synthetic", "events": events, "events_sha256": digest(events),
                      "prompt": "synthetic", "prompt_sha256": digest("synthetic"), "prompt_bytes": 9,
                      "input_tokens": tokens, "cached_input_tokens": 20, "output_tokens": 5,
                      "elapsed_seconds": 1, "timed_out": False, "returncode": 0,
                      "protected_files_changed": [], "metrics": event_metrics(events), "artifacts": artifacts,
                      "acceptance": {"evaluator": "keel_workflow_oracle_v1", "task_sha256": digest(task),
                                     "artifact_sha256": digest(artifacts), "passed": True},
                      "policy_rejections": [], "accepted": True}
            self.records.append(seal(record))

    def change(self, record, **values):
        record.update(values)
        record.pop("integrity_sha256", None)
        record.update(seal(record))

    def test_registration_is_paired_deterministic_and_split_isolated(self):
        validate_plan(self.plan)
        self.assertEqual(len(self.plan["trials"]), 16)
        self.assertEqual(self.plan, make_plan("synthetic", repeats=2, task_ids=["boundary", "unique"], compiler_sha256="synthetic"))
        with self.assertRaises(ValueError):
            make_plan("model", task_ids=["positive_sum"])
        with self.assertRaises(ValueError):
            make_plan("model", repeats=True)
        with self.assertRaises(ValueError):
            make_plan("model", variants=["protocol"])
        broken = {**self.plan, "trials": self.plan["trials"][:-1]}
        broken.pop("integrity_sha256")
        with self.assertRaises(ValueError):
            validate_plan(seal(broken))

    def test_changed_harness_requires_new_plan_but_old_results_report(self):
        with patch("efficiency.harness_hash", return_value="changed"):
            with self.assertRaises(ValueError):
                validate_plan(self.plan)
            self.assertEqual(report(self.plan, self.records)["status"], "COMPLETE")

    def test_counts_cached_input_once_and_keeps_failed_attempts(self):
        record = next(r for r in self.records if r["variant"] == "compact_context")
        self.change(record, acceptance={**record["acceptance"], "passed": False}, accepted=False)
        result = report(self.plan, self.records)
        group = result["variants"]["compact_context"]
        self.assertEqual(group["total_tokens_including_failures"], 220)
        self.assertEqual(group["accepted"], 3)
        self.assertAlmostEqual(group["tokens_per_accepted_change"], 220 / 3)
        self.assertFalse(result["comparisons"]["compact_context"]["candidate_for_validation"])
        self.assertTrue(result["comparisons"]["compact_context"]["paired_acceptance_regressions"])
        self.assertEqual(result["economic_advantage"], "UNKNOWN")

    def test_missing_duplicate_and_tampered_records(self):
        incomplete = report(self.plan, self.records[:-1])
        self.assertEqual(incomplete["status"], "INCOMPLETE")
        self.assertTrue(all(c["token_reduction_per_accepted_change"] is None for c in incomplete["comparisons"].values()))
        self.assertEqual(report(self.plan, self.records + [self.records[0]])["status"], "INVALID")
        self.change(self.records[0], input_tokens=2)
        self.assertEqual(report(self.plan, self.records)["status"], "INVALID")

    def test_changed_artifact_and_false_policy_acceptance_rejected(self):
        original = copy.deepcopy(self.records)
        self.change(self.records[0], artifacts={"solution.keel": "changed"})
        self.assertEqual(report(self.plan, self.records)["status"], "INVALID")
        self.records = original
        self.change(self.records[0], protected_files_changed=["public_tests.keel"])
        self.assertEqual(report(self.plan, self.records)["status"], "INVALID")

    def test_unknown_usage_cannot_be_counted_as_free_or_accepted(self):
        record = self.records[0]
        self.change(record, events=[], events_sha256=digest([]), metrics=event_metrics([]),
                    input_tokens=None, cached_input_tokens=None, output_tokens=None,
                    accepted=False, policy_rejections=["missing token usage"])
        group = report(self.plan, self.records)["variants"][record["variant"]]
        self.assertIsNone(group["total_tokens_including_failures"])
        self.assertIsNone(group["tokens_per_accepted_change"])

    def test_over_budget_and_timeout_rejected_without_losing_correctness(self):
        record = self.records[0]
        record["elapsed_seconds"] = 181
        record["timed_out"] = True
        self.assertIn("time budget exceeded", policy_reasons(self.plan, record))
        record["events"][0]["usage"]["input_tokens"] = 40000
        self.assertIn("token budget exceeded", policy_reasons(self.plan, record))
        self.assertTrue(record["acceptance"]["passed"])

    def test_completed_items_counted_once_and_requests_unknown(self):
        item = {"type": "command_execution", "exit_code": 1, "aggregated_output": "bad"}
        metrics = event_metrics([{"type": "item.started", "item": item}, {"type": "item.completed", "item": item}])
        self.assertEqual(metrics["command_calls"], 1)
        self.assertEqual(metrics["failed_commands"], 1)
        self.assertEqual(metrics["tool_output_bytes"], 3)
        self.assertIsNone(metrics["model_requests"])

    def test_evidence_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "evidence.json"
            save_new(path, {"first": True})
            with self.assertRaises(FileExistsError):
                save_new(path, {"second": True})
            self.assertEqual(json.loads(path.read_text()), {"first": True})

    def test_tests_only_rejects_code_properties_empty_and_broken_blocks(self):
        self.assertTrue(tests_only('// comment\ntest "braces {" { assert true // }\n }'))
        for source in ('', '// tests', 'fn x() {}', 'property "x" (x in gen.int(min: 0, max: 1)) { assert true }',
                       'test "x" { assert true', 'test "x" {} fn x() {}'):
            self.assertFalse(tests_only(source), source)

    def test_mutant_traps_do_not_count_as_assertion_failures(self):
        failure = {"name": "assert something", "status": "FAILED", "failure": {"kind": "division_by_zero"}}
        outcome = {"result": {"status": "FAILED", "tests": [failure],
                              "differential": {"reference": {"tests": [failure]}}}}
        self.assertFalse(assertion_failure(outcome))
        failure["failure"]["kind"] = "assertion_failure"
        self.assertTrue(assertion_failure(outcome))


@unittest.skipUnless(KEEL.is_file(), "cargo build --locked supplies local compiler; no model calls")
class FixtureTests(unittest.TestCase):
    def test_runner_records_fake_agent_output_and_rejects_protected_edits(self):
        task = SUITE["tasks"][0]
        plan = make_plan("synthetic", repeats=1, task_ids=[task["id"]], compiler_sha256=file_hash(KEEL))
        trial = next(t for t in plan["trials"] if t["variant"] == "compact_combined")
        for tamper in (False, True):
            with self.subTest(tamper=tamper), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                fake = root / "codex"
                fake.write_text(f"#!{sys.executable}\n" +
                                "import json, pathlib, sys\nsys.stdin.read()\n" +
                                f"pathlib.Path('solution.keel').write_text({task['correct']!r})\n" +
                                ("pathlib.Path('public_tests.keel').write_text('')\n" if tamper else "") +
                                "print(json.dumps({'type':'turn.completed','usage':{'input_tokens':100,'cached_input_tokens':50,'output_tokens':10}}))\n")
                fake.chmod(0o755)
                output = root / "artifacts"
                output.mkdir()
                with patch.dict(os.environ, {"PATH": str(root) + os.pathsep + os.environ["PATH"]}):
                    record = run_trial(plan, trial, task, KEEL, output)
                self.assertTrue(record["acceptance"]["passed"])
                self.assertEqual(record["accepted"], not tamper)
                self.assertEqual(record["input_tokens"], 100)
                self.assertTrue((output / "events.jsonl").is_file())
                self.assertEqual(report(plan, [record])["status"], "INCOMPLETE")

    def test_changed_interface_is_rejected_even_if_behavior_passes(self):
        task = SUITE["tasks"][0]
        source = task["correct"].replace("pub fn", "fn")
        evidence = score_artifact(task, {"solution.keel": source}, KEEL)
        self.assertFalse(evidence["passed"])
        self.assertIn("interface", evidence["reason"])

    def test_all_correct_solutions_pass_and_starters_fail_independently(self):
        evidence = preflight(KEEL, SUITE["tasks"])
        self.assertTrue(evidence["passed"], json.dumps(evidence))

    def test_public_packets_do_not_leak_oracles_and_combined_is_smaller(self):
        for task in SUITE["tasks"]:
            with self.subTest(task=task["id"]), tempfile.TemporaryDirectory() as directory:
                workspace = Path(directory)
                write_workspace(workspace, task, KEEL)
                full = context_packet(task, "full_context", KEEL, workspace)
                compact = context_packet(task, "compact_context", KEEL, workspace)
                guided = context_packet(task, "compact_guided", KEEL, workspace)
                self.assertLess(len(json.dumps(compact)), len(json.dumps(full)))
                self.assertLess(len(json.dumps(guided)), len(json.dumps(full)))
                for variant in VARIANTS:
                    packet = (None if variant == "protocol" else full if variant == "full_context"
                              else guided if variant == "compact_guided" else compact)
                    prompt = prompt_for(task, variant, packet)
                    self.assertNotIn('"mutants"', prompt)
                    self.assertNotIn('"correct_implementations"', prompt)
                    if task["oracle"]:
                        self.assertNotIn(task["oracle"], prompt)
                    self.assertNotIn("efficiency_tasks.py", prompt)

    def test_vacuous_generated_tests_and_compile_errors_earn_no_mutation_credit(self):
        task = next(t for t in SUITE["tasks"] if t["id"] == "clamp_tests")
        for source in ('test "vacuous" { assert true }', 'test "broken" { assert unknown(1) }',
                       'test "always failing" { assert false }'):
            evidence = score_artifact(task, {"generated_tests.keel": source}, KEEL)
            self.assertFalse(evidence["passed"])
            if "unknown" in source:
                self.assertEqual(evidence["mutants_killed"], [])
