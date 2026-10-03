#!/usr/bin/env python3
"""Optional real Codex trials. Running consumes the user's configured Codex quota."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import subprocess
import tempfile
import time

from agent_eval import HERE, acceptance, digest, evaluate, load_suite, seal, validate_plan


def extract_usage(events):
    completed = [event.get("usage", {}) for event in events if event.get("type") == "turn.completed"]
    keys = ("input_tokens", "cached_input_tokens", "output_tokens")
    if not completed or any(any(type(item.get(key)) is not int or item[key] < 0 for key in keys) for item in completed):
        return {key: None for key in keys}
    return {key: sum(item[key] for item in completed) for key in keys}


def public_source(task, language):
    if language == "c":
        return "#include <stdint.h>\n#include <stdbool.h>\n" + task[language]
    return task[language]


def instructions(task, condition):
    language = "keel" if condition.startswith("keel") else "c"
    file = "solution.keel" if language == "keel" else "solution.c"
    base = f"Repair {file} to meet this requirement: {task['requirement']}\nEdit only {file}; retain the exact function interface. Do not add main or test blocks to that file. Public examples: {json.dumps(task['public_cases'])}. Hidden acceptance assertions are applied by an independent runner after you finish. Do not search outside this workspace or use network access. Do not alter public_task.json, public_tests.*, tools.py, or keel. Complete the repair and validate it.\n"
    if language == "keel":
        base += "Keel basics: fn name(arg: Int) -> Int/Bool, let immutable, var mutable, if/while with braces, return, &&/||, checked signed 64-bit arithmetic. No semicolons required. Public tests are in public_tests.keel. Run `cat solution.keel public_tests.keel > trial.keel && ./keel test trial.keel --json` for public tests. `./keel check solution.keel --json` checks types.\n"
        if condition == "keel_protocol":
            base += f"Use the semantic protocol: `./keel inspect solution.keel --symbol {task['function']} --json`. Structural edit: `./keel edit solution.keel --request request.json --json` with {{\"base_revision\":\"revision from inspect\",\"target\":\"fn:{task['function']}\",\"operation\":\"replace_body\",\"source\":\"{{ replacement body }}\",\"run\":\"check\"}}. Follow with public test validation.\n"
        else:
            base += "Use ordinary source-file reads and text edits; do not use inspect/edit/review semantic protocol commands.\n"
    elif condition == "existing_improved":
        base += "A matching JSON protocol is available: `python3 tools.py inspect`, `python3 tools.py check`, `python3 tools.py test`, `python3 tools.py edit --request request.json`. Edit JSON uses base_revision from inspect, target fn:<function>, operation replace_body, source a braced C body; candidate is checked and public-tested transactionally.\n"
    else:
        base += "Use ordinary source-file reads and text edits. Validate public examples with `cc -std=c11 -O2 public_tests.c -o public_tests && ./public_tests`.\n"
    return base


def run_trial(plan, trial, task, keel):
    language = "keel" if trial["condition"].startswith("keel") else "c"
    with tempfile.TemporaryDirectory(prefix="keel-agent-pilot-") as directory:
        workspace = Path(directory)
        file = workspace / ("solution.keel" if language == "keel" else "solution.c")
        file.write_text(public_source(task, language))
        public_task = {k: v for k, v in task.items() if k not in ("held_out_cases", "c", "keel")}
        (workspace / "public_task.json").write_text(json.dumps(public_task, indent=2))
        if language == "keel":
            shutil.copy2(keel, workspace / "keel")
            checks = "\n".join(f"    assert {task['function']}({', '.join(map(str, c['args']))}) == {str(c['expected']).lower()}" for c in task["public_cases"])
            (workspace / "public_tests.keel").write_text('test "public examples" {\n' + checks + "\n}\n")
        else:
            checks = "\n".join(f"if ({task['function']}({', '.join(map(str, c['args']))}) != {int(c['expected'])}LL) return 1;" for c in task["public_cases"])
            (workspace / "public_tests.c").write_text('#include "solution.c"\nint main(void) {\n' + checks + "\nreturn 0;\n}\n")
            if trial["condition"] == "existing_improved":
                shutil.copy2(HERE / "fixture_tools.py", workspace / "tools.py")
        protected = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in workspace.iterdir() if p != file}
        prompt = instructions(task, trial["condition"])
        command = ["codex", "exec", "--json", "--ignore-user-config", "--ephemeral",
                   "--sandbox", "workspace-write", "--skip-git-repo-check", "-C", directory,
                   "--model", plan["model"], "-c", "approval_policy=\"never\"", "-"]
        started = time.monotonic()
        process = subprocess.Popen(command, cwd=workspace, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True, start_new_session=True)
        timed_out = False
        try:
            stdout, stderr = process.communicate(prompt, timeout=plan["budget"]["wall_seconds"])
        except subprocess.TimeoutExpired:
            timed_out = True
            os.killpg(process.pid, signal.SIGKILL)
            stdout, stderr = process.communicate()
        elapsed = time.monotonic() - started
        events = []
        for line in stdout.splitlines():
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError:
                events.append({"type": "unparsed_output", "text": line})
        usage = extract_usage(events)
        source = file.read_text() if file.exists() and not file.is_symlink() else ""
        measured = acceptance(task, source, language, keel)
        metering_complete = usage["input_tokens"] is not None and usage["output_tokens"] is not None
        budget_exceeded = metering_complete and usage["input_tokens"] + usage["output_tokens"] > plan["budget"]["total_tokens"]
        protected_changed = [name for name, checksum in protected.items() if not (workspace / name).is_file() or (workspace / name).is_symlink() or hashlib.sha256((workspace / name).read_bytes()).hexdigest() != checksum]
        valid_finish = process.returncode == 0 and not timed_out and not budget_exceeded and not protected_changed
        if not valid_finish:
            measured["accepted_before_budget_and_integrity_checks"] = measured["accepted"]
            measured["accepted"] = False
        return seal({**trial, "plan_sha256": plan["integrity_sha256"], "model": plan["model"],
                     "budget": plan["budget"], "source": source, "acceptance": measured,
                     "command": command, "prompt_sha256": hashlib.sha256(prompt.encode()).hexdigest(),
                     "elapsed_seconds": elapsed, **usage, "timed_out": timed_out,
                     "budget_exceeded": budget_exceeded, "returncode": process.returncode,
                     "protected_files_changed": protected_changed, "stderr": stderr,
                     "events": events, "events_sha256": digest(events),
                     "condition_isolation_verified": False,
                     "compiler_sha256": hashlib.sha256(Path(keel).read_bytes()).hexdigest(),
                     "environment": {"platform": platform.platform(), "python": platform.python_version()},
                     "costs": {"inference_usd": None, "tool_usd": None, "metering_evidence": None},
                     "limitations": ["Subscription token usage is not a dollar cost.",
                                     "Tool conditions are instructed, not sandbox-enforced; this is an exploratory pilot.",
                                     "Total-token budget is evaluated after the turn; over-budget repairs are rejected.",
                                     "Model selection is pinned by CLI argument; backend identity is not independently attested."]})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--keel", type=Path, default=HERE.parent / "target/release/keel")
    parser.add_argument("--max-trials", type=int, help="Debugging only; incomplete result sets are INVALID")
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Output already exists; preserve trial ledgers and choose a fresh output path")
    if not args.keel.is_file():
        parser.error("Build release Keel compiler before running")
    suite = load_suite()
    plan = json.loads(args.plan.read_text())
    validate_plan(plan, suite)
    tasks = {task["id"]: task for task in suite["tasks"]}
    records = []
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="keel-pilot-frozen-toolchain-") as directory:
        frozen_keel = Path(directory) / "keel"
        shutil.copy2(args.keel, frozen_keel)
        for trial in plan["trials"][:args.max_trials]:
            print(json.dumps({"running": trial["trial_id"]}), flush=True)
            record = run_trial(plan, trial, tasks[trial["task_id"]], frozen_keel)
            records.append(record)
            args.output.write_text(json.dumps(records, indent=2) + "\n")
            print(json.dumps({"trial": trial["trial_id"], "accepted": record["acceptance"]["accepted"],
                              "input_tokens": record["input_tokens"], "output_tokens": record["output_tokens"],
                              "elapsed_seconds": record["elapsed_seconds"]}), flush=True)
    report = evaluate(plan, suite, records)
    args.output.with_suffix(".gate.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return {"PASS": 0, "FAIL": 1, "UNKNOWN": 2, "INVALID": 3}[report["status"]]


if __name__ == "__main__":
    raise SystemExit(main())
