#!/usr/bin/env python3
"""Pre-register agent comparisons, independently score artifacts, fail closed on claims."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import random
import subprocess
import sys
import tempfile

CONDITIONS = ("existing_conventional", "existing_improved", "keel_text", "keel_protocol")
HERE = Path(__file__).resolve().parent
SUITE = HERE / "tasks.json"


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def digest(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def seal(value):
    return {**value, "integrity_sha256": digest(value)}


def verify_seal(value):
    content = {k: v for k, v in value.items() if k != "integrity_sha256"}
    if value.get("integrity_sha256") != digest(content):
        raise ValueError("integrity checksum mismatch")


def load_suite(path=SUITE):
    suite = json.loads(Path(path).read_text())
    if len({t["id"] for t in suite["tasks"]}) != len(suite["tasks"]):
        raise ValueError("duplicate task identifiers")
    return suite


def make_plan(suite, model, repeats, seed=42, seconds=180, tokens=32000):
    if not isinstance(model, str) or not model.strip() or any(type(value) is not int or value < 1 for value in (repeats, seconds, tokens)) or type(seed) is not int:
        raise ValueError("model, repeats, time and token budget must be positive")
    trials = [{"trial_id": f"{t['id']}:{r}:{condition}", "task_id": t["id"],
               "repeat": r, "condition": condition}
              for t in suite["tasks"] for r in range(repeats) for condition in CONDITIONS]
    random.Random(seed).shuffle(trials)
    return seal({"schema_version": 1, "kind": "preregistered_agent_plan", "suite_sha256": digest(suite),
                 "model": model, "repeats": repeats, "seed": seed,
                 "budget": {"wall_seconds": seconds, "total_tokens": tokens},
                 "required_cost_reduction": 0.25, "trials": trials})


def validate_plan(plan, suite):
    verify_seal(plan)
    if plan.get("kind") != "preregistered_agent_plan" or plan.get("schema_version") != 1:
        raise ValueError("unsupported plan schema")
    if plan["suite_sha256"] != digest(suite):
        raise ValueError("task suite changed after registration")
    expected = make_plan(suite, plan["model"], plan["repeats"], plan["seed"],
                         plan["budget"]["wall_seconds"], plan["budget"]["total_tokens"])
    if expected != plan:
        raise ValueError("plan differs from complete paired four-condition design")


def acceptance(task, source, language, keel, timeout=15):
    """The evaluator adds assertions AFTER the agent finishes, outside its workspace."""
    with tempfile.TemporaryDirectory(prefix="keel-independent-oracle-") as directory:
        workspace = Path(directory)
        file = workspace / ("accept.keel" if language == "keel" else "accept.c")
        if language == "keel":
            checks = "\n".join(f"    assert {task['function']}({', '.join(map(str, case['args']))}) == {str(case['expected']).lower()}" for case in task["held_out_cases"])
            file.write_text(source + '\ntest "independent acceptance" {\n' + checks + "\n}\n")
            command = [str(keel), "test", str(file), "--json", "--timeout-ms", "2000"]
        else:
            checks = "\n".join(f"    if ({task['function']}({', '.join(map(str, case['args']))}) != {int(case['expected'])}LL) return 1;" for case in task["held_out_cases"])
            file.write_text("#include <stdint.h>\n#include <stdbool.h>\n" + source + "\nint main(void) {\n" + checks + "\nreturn 0;\n}\n")
            binary = workspace / "accept"
            command = ["cc", "-std=c11", "-O2", "-fsanitize=undefined", "-fno-sanitize-recover=all", str(file), "-o", str(binary)]
        try:
            process = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
            if process.returncode == 0 and language == "c":
                process = subprocess.run([str(binary)], capture_output=True, text=True, timeout=timeout)
            return {"accepted": process.returncode == 0, "returncode": process.returncode,
                    "evaluator": "independent_native_assertions_v1",
                    "source_sha256": hashlib.sha256(source.encode()).hexdigest(),
                    "oracle_sha256": digest(task["held_out_cases"]),
                    "stdout": process.stdout[-4096:], "stderr": process.stderr[-4096:]}
        except subprocess.TimeoutExpired:
            return {"accepted": False, "returncode": None, "evaluator": "independent_native_assertions_v1",
                    "source_sha256": hashlib.sha256(source.encode()).hexdigest(),
                    "oracle_sha256": digest(task["held_out_cases"]), "error": "acceptance timeout"}


def finite_nonnegative(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value) and value >= 0


def evaluate(plan, suite, records):
    """PASS means this registered experiment met its gate, never universal correctness."""
    try:
        validate_plan(plan, suite)
        expected = {t["trial_id"]: t for t in plan["trials"]}
        if len(records) != len(expected) or {r["trial_id"] for r in records} != set(expected):
            raise ValueError("missing, duplicate, or unexpected trials (failed attempts must be included)")
        task_map = {t["id"]: t for t in suite["tasks"]}
        unknown = []
        for record in records:
            verify_seal(record)
            trial = expected[record["trial_id"]]
            if any(record[key] != trial[key] for key in ("condition", "task_id", "repeat")):
                raise ValueError("trial metadata mismatch")
            if record["plan_sha256"] != plan["integrity_sha256"] or record["model"] != plan["model"] or record["budget"] != plan["budget"]:
                raise ValueError("mismatched plan, model, or instruction budget")
            task = task_map[record["task_id"]]
            oracle = record["acceptance"]
            if type(oracle["accepted"]) is not bool or oracle["evaluator"] != "independent_native_assertions_v1" or oracle["oracle_sha256"] != digest(task["held_out_cases"]):
                raise ValueError("acceptance was not scored against the registered oracle")
            if "accepted_before_budget_and_integrity_checks" in oracle and type(oracle["accepted_before_budget_and_integrity_checks"]) is not bool:
                raise ValueError("pre-policy assertion result must be Boolean")
            if hashlib.sha256(record["source"].encode()).hexdigest() != oracle["source_sha256"]:
                raise ValueError("artifact differs from independently tested source")
            if record.get("events_sha256") != digest(record["events"]):
                raise ValueError("event ledger changed")
            completed_usage = [event.get("usage", {}) for event in record["events"] if event.get("type") == "turn.completed"]
            if not completed_usage:
                unknown.append(f"{record['trial_id']}: no completed-turn usage evidence")
            for key in ("input_tokens", "cached_input_tokens", "output_tokens"):
                if completed_usage and all(type(usage.get(key)) is int and usage[key] >= 0 for usage in completed_usage):
                    if record.get(key) is not None and record[key] != sum(usage[key] for usage in completed_usage):
                        raise ValueError("usage totals do not match raw completed-turn events")
            for key in ("elapsed_seconds", "input_tokens", "cached_input_tokens", "output_tokens"):
                value = record.get(key)
                if value is None:
                    unknown.append(f"{record['trial_id']}: missing {key}")
                elif not finite_nonnegative(value):
                    raise ValueError(f"invalid {key}")
                elif key.endswith("tokens") and type(value) is not int:
                    raise ValueError("token counts must be integers")
            if record.get("cached_input_tokens", 0) is not None and record.get("input_tokens", 0) is not None and record["cached_input_tokens"] > record["input_tokens"]:
                raise ValueError("cached input tokens exceed total input tokens")
            if record.get("input_tokens") is not None and record.get("output_tokens") is not None and record["input_tokens"] + record["output_tokens"] > plan["budget"]["total_tokens"] and oracle["accepted"]:
                raise ValueError("over-budget attempt cannot count as accepted")
            if record.get("elapsed_seconds") is not None and record["elapsed_seconds"] > plan["budget"]["wall_seconds"] + 5 and oracle["accepted"]:
                raise ValueError("over-time attempt cannot count as accepted")
            costs = record.get("costs")
            if costs is None or costs.get("inference_usd") is None or costs.get("tool_usd") is None:
                unknown.append(f"{record['trial_id']}: missing inference/tool dollar costs")
            elif not all(finite_nonnegative(costs[key]) for key in ("inference_usd", "tool_usd")):
                raise ValueError("invalid inference/tool dollar cost")
            elif not costs.get("metering_evidence"):
                unknown.append(f"{record['trial_id']}: no cost metering evidence")
            if not record.get("condition_isolation_verified"):
                unknown.append(f"{record['trial_id']}: tool-condition isolation unverified")
        if len(records) != len({r["trial_id"] for r in records}):
            raise ValueError("duplicate trial")
        by_condition = {}
        for condition in CONDITIONS:
            group = [r for r in records if r["condition"] == condition]
            accepted = sum(r["acceptance"]["accepted"] for r in group)
            known_cost = all(r.get("costs") and r["costs"].get("inference_usd") is not None and r["costs"].get("tool_usd") is not None for r in group)
            cost = sum(r["costs"]["inference_usd"] + r["costs"]["tool_usd"] for r in group) if known_cost else None
            by_condition[condition] = {"attempts": len(group), "accepted": accepted, "success_rate": accepted / len(group),
                                       "native_assertions_passed_before_policy": sum(r["acceptance"].get("accepted_before_budget_and_integrity_checks", r["acceptance"]["accepted"]) for r in group),
                                       "total_cost_usd_including_failures": cost,
                                       "cost_per_accepted_change_usd": cost / accepted if cost is not None and accepted else None}
            for key in ("input_tokens", "cached_input_tokens", "output_tokens", "elapsed_seconds"):
                values = [r.get(key) for r in group]
                by_condition[condition]["total_" + key] = sum(values) if all(value is not None for value in values) else None
        summary = {"conditions": by_condition, "required_cost_reduction": plan["required_cost_reduction"],
                   "adoption_readiness": "UNKNOWN: this public microtask suite does not establish representative repository, API, ownership, stateful, or performance-task results"}
        if unknown:
            return {"status": "UNKNOWN", "reasons": unknown, **summary}
        keel = by_condition["keel_protocol"]
        baselines = [by_condition[c] for c in ("existing_conventional", "existing_improved")]
        if not keel["accepted"] or any(not base["accepted"] for base in baselines):
            return {"status": "UNKNOWN", "reasons": ["zero accepted changes prevents a cost comparison"], **summary}
        baseline = min(base["cost_per_accepted_change_usd"] for base in baselines)
        if baseline == 0:
            return {"status": "UNKNOWN", "reasons": ["zero-cost baseline has no defined percentage reduction"], **summary}
        reduction = 1 - keel["cost_per_accepted_change_usd"] / baseline
        regressions = []
        paired = {(r["task_id"], r["repeat"], r["condition"]): r for r in records}
        for task in task_map:
            for repeat in range(plan["repeats"]):
                if not paired[(task, repeat, "keel_protocol")]["acceptance"]["accepted"] and any(paired[(task, repeat, c)]["acceptance"]["accepted"] for c in ("existing_conventional", "existing_improved")):
                    regressions.append(f"{task}:{repeat}")
        passed = reduction >= plan["required_cost_reduction"] and not regressions
        return {"status": "PASS" if passed else "FAIL", "measured_cost_reduction": reduction,
                "paired_acceptance_regressions": regressions,
                "reasons": [] if passed else ["cost improvement or paired independent acceptance requirement not met"],
                "scope": "Registered sample only; no statistical population or universal correctness claim", **summary}
    except (KeyError, TypeError, ValueError, ZeroDivisionError, AttributeError, OverflowError) as error:
        return {"status": "INVALID", "reasons": [str(error)]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    plan_parser = commands.add_parser("plan")
    plan_parser.add_argument("--model", required=True)
    plan_parser.add_argument("--repeats", type=int, default=3)
    plan_parser.add_argument("--seconds", type=int, default=180)
    plan_parser.add_argument("--tokens", type=int, default=32000)
    plan_parser.add_argument("--seed", type=int, default=42)
    plan_parser.add_argument("--output", type=Path, required=True)
    gate_parser = commands.add_parser("gate")
    gate_parser.add_argument("--plan", type=Path, required=True)
    gate_parser.add_argument("--results", type=Path, required=True)
    gate_parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    suite = load_suite()
    if args.command == "plan":
        result = make_plan(suite, args.model, args.repeats, args.seed, args.seconds, args.tokens)
    else:
        result = evaluate(json.loads(args.plan.read_text()), suite, json.loads(args.results.read_text()))
    content = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(content)
    print(content, end="")
    return {"PASS": 0, "FAIL": 1, "UNKNOWN": 2, "INVALID": 3}.get(result.get("status"), 0)


if __name__ == "__main__":
    raise SystemExit(main())
