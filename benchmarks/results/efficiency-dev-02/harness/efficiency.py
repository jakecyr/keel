#!/usr/bin/env python3
"""Preregister, preflight, run and report Keel workflow ablations. Live runs consume quota."""
import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import random
import shutil
import signal
import statistics
import subprocess
import tempfile
import time

from agent_eval import digest, seal, verify_seal
from codex_pilot import extract_usage
from efficiency_tasks import SUITE

HERE = Path(__file__).resolve().parent
VARIANTS = ("protocol", "full_context", "compact_context", "compact_combined")
BASICS = """Keel: fn f(x: Int) -> Int { return x }; let immutable, var mutable; if/while/for use braces. Int is checked signed 64-bit. Lists: [1, 2], []; no implicit conversions. Owned Text/List parameters use read/edit/take; mutation calls use edit, and owned copies require explicit clone. Compute reads before an exclusive edit borrow. Match both Ok(value)/Err(message) or Some(value)/None arms. Tests: test \"name\" { assert expression }. No shadowing. `result` is reserved for postconditions, even without contracts; name locals/parameters `output` or another identifier. For exact syntax use ./keel agent spec language or collections; for signatures ./keel api NAME --json. TESTED means sampled cases, never proof."""


def file_hash(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def harness_hash():
    return digest({name: file_hash(HERE / name) for name in
                   ("efficiency.py", "efficiency_tasks.py", "efficiency_workflow.py", "agent_eval.py", "codex_pilot.py")})


def save_new(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x") as output:
        json.dump(value, output, indent=2, allow_nan=False)
        output.write("\n")


def make_plan(model, repeats=3, seed=42, seconds=180, tokens=32000,
              split="development", task_ids=None, variants=VARIANTS, compiler_sha256=None):
    if not isinstance(model, str) or not model.strip():
        raise ValueError("model must be explicit")
    if any(type(n) is not int or n <= 0 for n in (repeats, seconds, tokens)) or type(seed) is not int:
        raise ValueError("positive integer repeats and budgets required")
    available = [t["id"] for t in SUITE["tasks"] if t["split"] == split]
    selected = available if task_ids is None else list(task_ids)
    if not selected or len(set(selected)) != len(selected) or not set(selected) <= set(available):
        raise ValueError("select unique tasks within one registered split")
    variants = list(variants)
    if len(variants) < 2 or len(set(variants)) != len(variants) or not set(variants) <= set(VARIANTS):
        raise ValueError("choose at least two unique known variants; first is the baseline")
    trials = [{"trial_id": f"{task}:{r}:{variant}", "task_id": task, "repeat": r, "variant": variant}
              for task in selected for r in range(repeats) for variant in variants]
    random.Random(seed).shuffle(trials)
    return seal({"schema": 1, "kind": "keel_workflow_experiment", "suite_sha256": digest(SUITE),
                 "harness_sha256": harness_hash(), "compiler_sha256": compiler_sha256,
                 "model": model, "repeats": repeats, "seed": seed, "split": split,
                 "task_ids": selected, "variants": variants, "baseline": variants[0],
                 "budget": {"wall_seconds": seconds, "total_tokens": tokens}, "trials": trials})


def validate_plan(plan, current_harness=True):
    verify_seal(plan)
    expected = make_plan(plan["model"], plan["repeats"], plan["seed"],
                         plan["budget"]["wall_seconds"], plan["budget"]["total_tokens"],
                         plan["split"], plan["task_ids"], plan["variants"], plan["compiler_sha256"])
    if not current_harness:
        expected.pop("integrity_sha256")
        expected["harness_sha256"] = plan["harness_sha256"]
        expected = seal(expected)
    if expected != plan:
        raise ValueError("registered suite, harness, or paired trial design changed")


def write_workspace(workspace, task, keel, helper=True):
    for name, content in task["files"].items():
        (workspace / name).write_text(content)
    if helper:
        shutil.copy2(keel, workspace / "keel")
        shutil.copy2(HERE / "efficiency_workflow.py", workspace / "workflow.py")
        (workspace / "AGENTS.md").write_text("This is an isolated Keel experiment. Follow the supplied task and workflow. Edit only the designated files. Do not read outside this workspace or use the network.\n")


def keel_json(keel, workspace, *args):
    started = time.monotonic()
    try:
        process = subprocess.run([str(keel), *args], cwd=workspace, capture_output=True, text=True, timeout=45)
        try:
            result = json.loads(process.stdout)
        except ValueError:
            result = {"status": "INVALID_OUTPUT", "stdout": process.stdout}
        return {"returncode": process.returncode, "result": result, "stderr": process.stderr,
                "elapsed_seconds": time.monotonic() - started}
    except subprocess.TimeoutExpired:
        return {"returncode": None, "result": {"status": "TIMEOUT"}, "elapsed_seconds": time.monotonic() - started}


def context_packet(task, variant, keel, workspace):
    if variant == "protocol":
        return None
    response = keel_json(keel, workspace, "agent", "context", ".", "--symbol", task["symbol"], "--json")
    if response["returncode"] != 0:
        raise ValueError(f"cannot prepare context for {task['id']}: {response}")
    context = response["result"]
    # Both upfront conditions contain identical public files; only reference/metadata differ.
    if variant == "full_context":
        return {"context": context, "public_files": task["files"]}
    bundle = context["context"]
    packet = {"schema": 1, "revision": bundle["revision"], "incomplete": bundle["incomplete"],
              "functions": [{k: f[k] for k in ("target", "source", "dependencies", "contracts", "effects")}
                            for f in bundle["functions"]],
              "public_files": task["files"], "apis": []}
    for api in task["apis"]:
        response = keel_json(keel, workspace, "api", api, "--json")
        if response["returncode"] != 0:
            raise ValueError(f"unknown fixture API: {api}")
        packet["apis"].append(response["result"])
    # Fail explicitly rather than silently hiding essential context.
    if len(json.dumps(packet, ensure_ascii=False).encode()) > 12000 or packet["incomplete"]:
        raise ValueError("compact task packet exceeds its complete 12KB budget")
    return packet


def prompt_for(task, variant, packet):
    prompt = (f"{task['requirement']}\nEdit only: {', '.join(task['editable'])}. Preserve other files, interfaces, contracts, and existing tests. "
              "An independent evaluator checks your result after you exit. Do not read outside the workspace, use network access, or delegate. "
              "The workspace contains all public task files and ./keel. Complete the change and validate it.\n" + BASICS + "\n")
    if task["kind"] == "tests":
        prompt += "Write test blocks using ordinary text edits. Validate with `python3 workflow.py validate`. Do not add functions, properties, or implementation code.\n"
    else:
        run = "affected_checks_and_tests" if variant == "compact_combined" else "check"
        request = {"base_revision": "COPY_CURRENT_REVISION", "target": "fn:" + task["symbol"],
                   "operation": "replace_body", "source": "{ replacement body }", "run": run}
        prompt += "Use revision-bound structural edits: write request.json with " + json.dumps(request) + ".\n"
        if variant == "compact_combined":
            prompt += "Run `python3 workflow.py edit request.json`. APPLIED with TESTED evidence already validates all public tests; no separate check/test is needed unless source changes.\n"
        else:
            prompt += "Run `./keel edit . --request request.json --json`, then `python3 workflow.py validate`. The edit already checks types; no extra check is needed.\n"
    if packet is None:
        prompt += f"First inspect `./keel inspect . --symbol {task['symbol']} --json`; read relevant public files as needed.\n"
    else:
        prompt += "Initial context below contains current source, revision and public tests. Use it directly; retrieve fresh context only if needed after a change or error.\n"
        prompt += json.dumps(packet, ensure_ascii=False, separators=(",", ":")) + "\n"
    return prompt


def tests_only(source):
    """Conservatively admit only top-level test blocks, ignoring strings/comments.

    Compiler still parses/types/checks the complete file. Nested helper declarations
    and directives are rejected by the compiler; this scanner prevents replacing API code.
    """
    import re
    tokens = re.findall(r'//[^\n]*|"(?:\\.|[^"\\])*"|[A-Za-z_][A-Za-z_0-9]*|[^\s]', source)
    tokens = [t for t in tokens if not t.startswith("//")]
    i = 0
    count = 0
    while i < len(tokens):
        if tokens[i] != "test" or i + 2 >= len(tokens) or not tokens[i + 1].startswith('"') or tokens[i + 2] != "{":
            return False
        i += 3
        depth = 1
        while i < len(tokens) and depth:
            depth += (tokens[i] == "{") - (tokens[i] == "}")
            i += 1
        if depth:
            return False
        count += 1
    return count > 0


def assertion_failure(outcome):
    result = outcome["result"]
    if result.get("status") != "FAILED":
        return False
    # Require executed assertion failures in both engines, not syntax errors or timeouts.
    groups = [result.get("tests", []), result.get("differential", {}).get("reference", {}).get("tests", [])]
    return all(any(t.get("status") == "FAILED" and "assert" in json.dumps(t).lower() for t in group) for group in groups)


def interfaces(inspection):
    return [{k: f[k] for k in ("name", "public", "parameters", "result", "effects", "contracts")}
            for f in inspection["result"].get("functions", [])]


def score_artifact(task, artifacts, keel):
    evidence = {"evaluator": "keel_workflow_oracle_v1", "task_sha256": digest(task),
                "artifact_sha256": digest(artifacts), "passed": False, "runs": []}
    with tempfile.TemporaryDirectory(prefix="keel-efficiency-oracle-") as directory:
        workspace = Path(directory)
        write_workspace(workspace, task, keel, helper=False)
        original = keel_json(keel, workspace, "inspect", ".", "--json") if task["kind"] == "code" else None
        if set(artifacts) != set(task["editable"]) or any(not isinstance(x, str) for x in artifacts.values()):
            evidence["reason"] = "missing or invalid editable artifact"
            return evidence
        for name, source in artifacts.items():
            (workspace / name).write_text(source)
        if task["kind"] == "code":
            candidate = keel_json(keel, workspace, "inspect", ".", "--json")
            if original["returncode"] != 0 or candidate["returncode"] != 0 or interfaces(original) != interfaces(candidate):
                evidence["reason"] = "interface changed or invalid implementation"
                evidence["runs"].append(candidate)
                return evidence
            manifest = json.loads((workspace / "keel.json").read_text())
            manifest["tests"].append("oracle.keel")
            (workspace / "keel.json").write_text(json.dumps(manifest))
            (workspace / "oracle.keel").write_text(task["oracle"])
            result = keel_json(keel, workspace, "test", ".", "--engine", "both", "--json")
            evidence["runs"].append(result)
            evidence["passed"] = result["returncode"] == 0 and result["result"].get("status") == "TESTED"
        else:
            if not tests_only(artifacts["generated_tests.keel"]):
                evidence["reason"] = "generated file must contain only nonempty test blocks"
                return evidence
            # Public tests cannot earn mutation credit: run only agent-generated tests.
            manifest = json.loads((workspace / "keel.json").read_text())
            manifest["tests"] = ["generated_tests.keel"]
            (workspace / "keel.json").write_text(json.dumps(manifest))
            correct_passed = True
            for source in task["correct_implementations"]:
                (workspace / "solution.keel").write_text(source)
                result = keel_json(keel, workspace, "test", ".", "--engine", "both", "--json")
                evidence["runs"].append({"kind": "correct", **result})
                correct_passed &= result["returncode"] == 0 and result["result"].get("status") == "TESTED"
            killed = []
            for name, source in task["mutants"].items():
                (workspace / "solution.keel").write_text(source)
                result = keel_json(keel, workspace, "test", ".", "--engine", "both", "--json")
                evidence["runs"].append({"kind": "mutant", "name": name, **result})
                if assertion_failure(result):
                    killed.append(name)
            evidence.update(correct_implementations_passed=bool(correct_passed), mutants_killed=killed,
                            mutants_total=len(task["mutants"]),
                            passed=bool(correct_passed and len(killed) == len(task["mutants"])))
    return evidence


def preflight(keel, tasks):
    results = []
    for task in tasks:
        good = score_artifact(task, {task["editable"][0]: task["correct"]}, keel)
        bad = score_artifact(task, {name: task["files"][name] for name in task["editable"]}, keel)
        results.append({"task_id": task["id"], "passed": good["passed"] and not bad["passed"],
                        "correct": good, "starter": bad})
    return {"kind": "offline_fixture_preflight", "agent_efficiency": "UNKNOWN",
            "passed": all(r["passed"] for r in results), "tasks": results}


def event_metrics(events):
    items = [e["item"] for e in events if e.get("type") == "item.completed" and isinstance(e.get("item"), dict)]
    types = Counter(item.get("type") for item in items)
    commands = [item for item in items if item.get("type") == "command_execution"]
    return {"completed_turns": sum(e.get("type") == "turn.completed" for e in events),
            "model_requests": None, "model_requests_note": "CLI events do not expose every inference request",
            "tool_calls": sum(n for k, n in types.items() if k not in ("agent_message", "reasoning")),
            "command_calls": len(commands), "file_change_calls": types["file_change"],
            "failed_commands": sum(item.get("exit_code") not in (0, None) for item in commands),
            "tool_output_bytes": sum(len(item.get("aggregated_output", "").encode()) for item in commands)}


def policy_reasons(plan, record):
    reasons = []
    usage = extract_usage(record["events"])
    if any(v is None for v in usage.values()):
        reasons.append("missing token usage")
    elif usage["cached_input_tokens"] > usage["input_tokens"]:
        reasons.append("invalid cached usage")
    elif usage["input_tokens"] + usage["output_tokens"] > plan["budget"]["total_tokens"]:
        reasons.append("token budget exceeded")
    if record["timed_out"] or record["elapsed_seconds"] > plan["budget"]["wall_seconds"]:
        reasons.append("time budget exceeded")
    if record["returncode"] != 0:
        reasons.append("agent did not finish successfully")
    if record["protected_files_changed"]:
        reasons.append("protected files changed")
    return reasons


def run_trial(plan, trial, task, keel, artifact_dir):
    with tempfile.TemporaryDirectory(prefix="keel-efficiency-agent-") as directory:
        workspace = Path(directory)
        write_workspace(workspace, task, keel)
        protected = {p.name: file_hash(p) for p in workspace.iterdir() if p.name not in task["editable"]}
        started = time.monotonic()
        packet = context_packet(task, trial["variant"], keel, workspace)
        prompt = prompt_for(task, trial["variant"], packet)
        preparation_seconds = time.monotonic() - started
        command = ["codex", "exec", "--json", "--ignore-user-config", "--ephemeral", "--sandbox", "workspace-write",
                   "--skip-git-repo-check", "-C", str(workspace), "--model", plan["model"], "-c", 'approval_policy="never"', "-"]
        started = time.monotonic()
        timed_out = False
        error = None
        # Raw events survive interruption. A partial run is never silently retried.
        with (artifact_dir / "events.jsonl").open("x") as stdout, (artifact_dir / "stderr.txt").open("x") as stderr:
            try:
                process = subprocess.Popen(command, cwd=workspace, stdin=subprocess.PIPE, stdout=stdout,
                                           stderr=stderr, text=True, start_new_session=True)
                try:
                    process.communicate(prompt, timeout=plan["budget"]["wall_seconds"])
                except (subprocess.TimeoutExpired, KeyboardInterrupt) as failure:
                    timed_out = True
                    error = type(failure).__name__
                    os.killpg(process.pid, signal.SIGKILL)
                    process.communicate()
                returncode = process.returncode
            except OSError as failure:
                returncode = None
                error = str(failure)
        elapsed = time.monotonic() - started
        events = []
        for line in (artifact_dir / "events.jsonl").read_text().splitlines():
            try:
                events.append(json.loads(line))
            except ValueError:
                events.append({"type": "unparsed_output", "text": line})
        artifacts = {name: (workspace / name).read_text() for name in task["editable"]
                     if (workspace / name).is_file() and not (workspace / name).is_symlink()}
        changed = [name for name, checksum in protected.items()
                   if not (workspace / name).is_file() or (workspace / name).is_symlink() or file_hash(workspace / name) != checksum]
        record = {**trial, "plan_sha256": plan["integrity_sha256"], "model": plan["model"],
                  "compiler_sha256": file_hash(keel), "prompt": prompt, "prompt_sha256": digest(prompt),
                  "prompt_bytes": len(prompt.encode()), "preparation_seconds": preparation_seconds,
                  "artifacts": artifacts, "command": command, "events": events, "events_sha256": digest(events),
                  "stderr": (artifact_dir / "stderr.txt").read_text(), "error": error,
                  "elapsed_seconds": elapsed, "timed_out": timed_out, "returncode": returncode,
                  "protected_files_changed": changed, **extract_usage(events), "metrics": event_metrics(events),
                  "acceptance": score_artifact(task, artifacts, keel),
                  "condition_isolation_verified": False,
                  "costs": {"inference_usd": None, "tool_usd": None}}
        record["policy_rejections"] = policy_reasons(plan, record)
        record["accepted"] = record["acceptance"]["passed"] and not record["policy_rejections"]
        return seal(record)


def summarize_group(records):
    accepted = sum(r["accepted"] for r in records)
    usage_known = bool(records) and all(r["input_tokens"] is not None and r["output_tokens"] is not None for r in records)
    total = sum(r["input_tokens"] + r["output_tokens"] for r in records) if usage_known else None
    return {"attempts": len(records), "accepted": accepted,
            "correct_before_policy": sum(r["acceptance"]["passed"] for r in records),
            "acceptance_rate": accepted / len(records) if records else None,
            "total_tokens_including_failures": total,
            "tokens_per_accepted_change": total / accepted if total is not None and accepted else None,
            "cached_input_tokens": sum(r["cached_input_tokens"] for r in records) if usage_known else None,
            "output_tokens": sum(r["output_tokens"] for r in records) if usage_known else None,
            "elapsed_seconds": sum(r["elapsed_seconds"] for r in records),
            "tool_calls": sum(r["metrics"]["tool_calls"] for r in records),
            "failed_commands": sum(r["metrics"]["failed_commands"] for r in records),
            "mean_prompt_bytes": statistics.mean(r["prompt_bytes"] for r in records) if records else None}


def bootstrap_reduction(pairs, seed):
    """Resample tasks as clusters, keeping paired variants/repetitions together."""
    if len(pairs) < 2:
        return None
    rng = random.Random(seed)
    values = []
    for _ in range(2000):
        sample = rng.choices(pairs, k=len(pairs))
        base = sum(x[0] for x in sample)
        if base:
            values.append(1 - sum(x[1] for x in sample) / base)
    if not values:
        return None
    values.sort()
    return [values[int(len(values) * .025)], values[min(len(values) - 1, int(len(values) * .975))]]


def report(plan, records):
    try:
        validate_plan(plan, current_harness=False)
        expected = {t["trial_id"]: t for t in plan["trials"]}
        if len({r["trial_id"] for r in records}) != len(records):
            raise ValueError("duplicate records")
        tasks = {t["id"]: t for t in SUITE["tasks"]}
        for r in records:
            verify_seal(r)
            if r["trial_id"] not in expected or any(r[k] != v for k, v in expected[r["trial_id"]].items()):
                raise ValueError("unexpected trial")
            if r["plan_sha256"] != plan["integrity_sha256"] or r["model"] != plan["model"] or r["compiler_sha256"] != plan["compiler_sha256"]:
                raise ValueError("plan/model/compiler mismatch")
            if r["events_sha256"] != digest(r["events"]) or r["prompt_sha256"] != digest(r["prompt"]):
                raise ValueError("changed events or prompt")
            usage = extract_usage(r["events"])
            if any(r[k] != v for k, v in usage.items()) or r["metrics"] != event_metrics(r["events"]):
                raise ValueError("usage/metrics differ from event ledger")
            if type(r["elapsed_seconds"]) not in (int, float) or not math.isfinite(r["elapsed_seconds"]) or r["elapsed_seconds"] < 0:
                raise ValueError("invalid elapsed time")
            if r["prompt_bytes"] != len(r["prompt"].encode()):
                raise ValueError("prompt byte count mismatch")
            a = r["acceptance"]
            if a["artifact_sha256"] != digest(r["artifacts"]) or a["task_sha256"] != digest(tasks[r["task_id"]]) or a["evaluator"] != "keel_workflow_oracle_v1" or type(a["passed"]) is not bool:
                raise ValueError("changed artifact or oracle")
            if r["policy_rejections"] != policy_reasons(plan, r) or type(r["accepted"]) is not bool or r["accepted"] != (a["passed"] and not r["policy_rejections"]):
                raise ValueError("acceptance violates budget/integrity policy")
        missing = sorted(set(expected) - {r["trial_id"] for r in records})
        groups = {v: summarize_group([r for r in records if r["variant"] == v]) for v in plan["variants"]}
        per_task = {t: {v: summarize_group([r for r in records if r["variant"] == v and r["task_id"] == t])
                        for v in plan["variants"]} for t in plan["task_ids"]}
        comparisons = {}
        indexed = {(r["task_id"], r["repeat"], r["variant"]): r for r in records}
        baseline = plan["baseline"]
        for variant in plan["variants"][1:]:
            regressions = []
            for task in plan["task_ids"]:
                for repeat in range(plan["repeats"]):
                    base = indexed.get((task, repeat, baseline))
                    candidate = indexed.get((task, repeat, variant))
                    if base and candidate and base["accepted"] and not candidate["accepted"]:
                        regressions.append(f"{task}:{repeat}")
            base = groups[baseline]["tokens_per_accepted_change"]
            candidate = groups[variant]["tokens_per_accepted_change"]
            reduction = 1 - candidate / base if not missing and base and candidate is not None else None
            pairs = [(per_task[t][baseline]["total_tokens_including_failures"], per_task[t][variant]["total_tokens_including_failures"])
                     for t in plan["task_ids"]]
            interval = bootstrap_reduction(pairs, plan["seed"]) if not missing and all(x is not None for pair in pairs for x in pair) else None
            comparisons[variant] = {"baseline": baseline, "token_reduction_per_accepted_change": reduction,
                                    "paired_acceptance_regressions": regressions,
                                    "exploratory_total_token_reduction_95pct_task_bootstrap": interval,
                                    "candidate_for_validation": bool(plan["split"] == "development" and reduction is not None and reduction > 0 and not regressions)}
        return {"status": "INCOMPLETE" if missing else "COMPLETE", "plan_sha256": plan["integrity_sha256"],
                "split": plan["split"], "missing_trials": missing, "variants": groups,
                "per_task": per_task, "comparisons": comparisons,
                "economic_advantage": "UNKNOWN", "condition_isolation_verified": False,
                "limitations": ["Keel-only workflow comparison, not a C comparison or the economic release gate.",
                                "Tool conditions are instructed, not enforced. CLI exposes aggregate turn usage, not per-request attribution.",
                                "Cached tokens remain in total input; missing usage and dollar costs remain unknown.",
                                "Budgets are checked after completion. Failed attempts remain in totals.",
                                "Public small task suite; validation split is held out from tuning, not secret or contamination-resistant.",
                                "Task bootstrap is exploratory, especially with few tasks/repetitions; it is not an acceptance-adjusted cost interval.",
                                "Checksums detect corruption, not malicious ledger rewriting. Replay independent acceptance for external evidence."]}
    except (KeyError, ValueError, TypeError, AttributeError, ZeroDivisionError) as error:
        return {"status": "INVALID", "reason": str(error)}


def read_records(directory):
    return [json.loads(path.read_text()) for path in sorted(Path(directory).glob("trial-*/record.json"))]


def run(plan, keel, directory, max_trials=None, resume=False):
    validate_plan(plan)
    if file_hash(keel) != plan["compiler_sha256"]:
        raise ValueError("compiler changed after registration")
    directory = Path(directory)
    if resume:
        if json.loads((directory / "plan.json").read_text()) != plan:
            raise ValueError("resume plan mismatch")
    else:
        directory.mkdir(parents=True, exist_ok=False)
        save_new(directory / "plan.json", plan)
        (directory / "harness").mkdir()
        for name in ("efficiency.py", "efficiency_tasks.py", "efficiency_workflow.py", "agent_eval.py", "codex_pilot.py"):
            shutil.copy2(HERE / name, directory / "harness" / name)
        git = subprocess.run(["git", "status", "--porcelain"], cwd=HERE.parent, capture_output=True, text=True)
        diff = subprocess.run(["git", "diff", "HEAD"], cwd=HERE.parent, capture_output=True)
        version = subprocess.run(["codex", "--version"], capture_output=True, text=True)
        save_new(directory / "environment.json", {"created_at": datetime.now(timezone.utc).isoformat(),
                 "platform": platform.platform(), "python": platform.python_version(),
                 "codex": version.stdout.strip(), "git_status": git.stdout,
                 "tracked_diff_sha256": hashlib.sha256(diff.stdout).hexdigest(), "harness_sha256": harness_hash()})
    records = read_records(directory)
    if report(plan, records)["status"] == "INVALID":
        raise ValueError("existing records invalid")
    done = {r["trial_id"] for r in records}
    tasks = {t["id"]: t for t in SUITE["tasks"]}
    count = 0
    with tempfile.TemporaryDirectory(prefix="keel-efficiency-frozen-") as temp:
        frozen = Path(temp) / "keel"
        shutil.copy2(keel, frozen)
        for index, trial in enumerate(plan["trials"]):
            if trial["trial_id"] in done:
                continue
            if max_trials is not None and count >= max_trials:
                break
            artifact_dir = directory / f"trial-{index:04d}"
            # An interrupted attempt may have consumed tokens. Never replace/retry it.
            if artifact_dir.exists():
                raise ValueError(f"unfinished attempt retained at {artifact_dir}; do not silently retry it")
            artifact_dir.mkdir()
            save_new(artifact_dir / "started.json", trial)
            print(json.dumps({"running": trial["trial_id"]}), flush=True)
            record = run_trial(plan, trial, tasks[trial["task_id"]], frozen, artifact_dir)
            save_new(artifact_dir / "record.json", record)
            records.append(record)
            count += 1
            print(json.dumps({"trial": trial["trial_id"], "accepted": record["accepted"],
                              "correct": record["acceptance"]["passed"], "input_tokens": record["input_tokens"],
                              "output_tokens": record["output_tokens"], "policy": record["policy_rejections"]}), flush=True)
            if record["input_tokens"] is None:
                print("Stopping: incomplete usage; inspect retained events before another live run.", flush=True)
                break
    result = report(plan, records)
    save_new(directory / f"report-{len(records):04d}-{time.time_ns()}.json", result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    pre = sub.add_parser("preflight", help="offline: validate correct solutions, starter failures, and mutation scorers")
    pre.add_argument("--keel", type=Path, default=HERE.parent / "target/debug/keel")
    pre.add_argument("--output", type=Path, required=True)
    plan = sub.add_parser("plan", help="offline: freeze a paired experiment before running agents")
    plan.add_argument("--model", required=True)
    plan.add_argument("--keel", type=Path, default=HERE.parent / "target/debug/keel")
    plan.add_argument("--split", choices=("development", "validation"), default="development")
    plan.add_argument("--tasks", nargs="+")
    plan.add_argument("--variants", nargs="+", choices=VARIANTS, default=VARIANTS)
    plan.add_argument("--repeats", type=int, default=3)
    plan.add_argument("--seed", type=int, default=42)
    plan.add_argument("--seconds", type=int, default=180)
    plan.add_argument("--tokens", type=int, default=32000)
    plan.add_argument("--output", type=Path, required=True)
    live = sub.add_parser("run", help="LIVE: consumes configured Codex quota; never invoked by tests")
    live.add_argument("--plan", type=Path, required=True)
    live.add_argument("--keel", type=Path, default=HERE.parent / "target/debug/keel")
    live.add_argument("--output-dir", type=Path, required=True)
    live.add_argument("--max-trials", type=int)
    live.add_argument("--resume", action="store_true")
    summary = sub.add_parser("report", help="offline: validate and aggregate immutable records")
    summary.add_argument("--run-dir", type=Path, required=True)
    summary.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "preflight":
            result = preflight(args.keel.resolve(), SUITE["tasks"])
            save_new(args.output, result)
            print(json.dumps({"passed": result["passed"], "tasks": [{"id": t["task_id"], "passed": t["passed"]} for t in result["tasks"]]}))
            return 0 if result["passed"] else 1
        if args.command == "plan":
            result = make_plan(args.model, args.repeats, args.seed, args.seconds, args.tokens, args.split,
                               args.tasks, args.variants, file_hash(args.keel))
            save_new(args.output, result)
            print(json.dumps({"plan": str(args.output), "trials": len(result["trials"]), "split": result["split"]}))
            return 0
        if args.command == "run":
            if args.max_trials is not None and args.max_trials <= 0:
                raise ValueError("max-trials must be positive")
            result = run(json.loads(args.plan.read_text()), args.keel.resolve(), args.output_dir, args.max_trials, args.resume)
        else:
            result = report(json.loads((args.run_dir / "plan.json").read_text()), read_records(args.run_dir))
            if args.output:
                save_new(args.output, result)
        print(json.dumps(result, indent=2))
        return {"COMPLETE": 0, "INCOMPLETE": 2, "INVALID": 3}[result["status"]]
    except (OSError, ValueError, KeyError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    raise SystemExit(main())
