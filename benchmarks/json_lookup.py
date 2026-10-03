#!/usr/bin/env python3
"""Local trusted-runtime JSON lookup benchmark; no model or network calls.

Compare committed source with working-tree source. Count cumulative requested
k_alloc bytes, not live heap/RSS. Timings include independent result assertions,
exclude process startup, and are not an application or agent-efficiency claim.
"""
import argparse
import hashlib
import json
import os
import platform
import random
import statistics
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def harness(runtime, stdlib, members, iterations):
    marker = "static KText k_alloc(size_t len) {"
    assert marker in runtime
    runtime = runtime.replace(marker, "static size_t requested_bytes, allocations;\n" + marker
                              + "\n    requested_bytes += len + 1; allocations++;", 1)
    document = "{" + ",".join(f'\"key{i}\":{i}' for i in range(members)) + "}"
    return runtime + "\n" + stdlib + "\n" + r'''
int main(void) {
  KText document = K_TEXT(DOCUMENT);
  KText pointer = K_TEXT(POINTER);
  requested_bytes = allocations = 0;
  struct timespec start, finish;
  clock_gettime(CLOCK_MONOTONIC, &start);
  for (int i = 0; i < ITERATIONS; i++) {
    KTextResult value = k_json_get(document, pointer);
    if (!value.ok || !k_equal(value.value, K_TEXT(EXPECTED))) return 1;
    k_drop(&value);
  }
  clock_gettime(CLOCK_MONOTONIC, &finish);
  double seconds = (finish.tv_sec - start.tv_sec) + (finish.tv_nsec - start.tv_nsec) / 1e9;
  printf("{\"seconds\":%.9f,\"requested_bytes\":%zu,\"allocations\":%zu,\"accepted\":true}\n", seconds, requested_bytes, allocations);
  return 0;
}
'''.replace("DOCUMENT", json.dumps(document)).replace("POINTER", json.dumps(f"/key{members-1}")) \
        .replace("EXPECTED", json.dumps(str(members-1))).replace("ITERATIONS", str(iterations))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True, help="committed Git revision")
    parser.add_argument("--members", type=int, default=2000)
    parser.add_argument("--iterations", type=int, default=100)
    parser.add_argument("--repeats", type=int, default=7)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not (1 <= args.members <= 10000 and 1 <= args.iterations <= 10000 and 1 <= args.repeats <= 100):
        parser.error("members/iterations must be 1..10000; repeats 1..100")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    # Refuse to overwrite evidence; retain failed commands as well as samples.
    with args.output.open("x") as destination:
        report = {"schema": 1, "scope": "wide-object final-key JSON lookup in trusted C runtime",
                  "agent_efficiency": "UNKNOWN", "platform": platform.platform(),
                  "machine": platform.machine(), "processor": platform.processor(),
                  "members": args.members, "iterations": args.iterations, "repeats": args.repeats,
                  "allocation_metric": "cumulative requested k_alloc bytes including terminators, not RSS",
                  "timing": "monotonic in-process seconds; parsing/lookup/allocation/assertion included; uncontrolled warm OS caches",
                  "flags": ["-std=c11", "-O2", "-pthread"], "commands": [], "samples": [], "sources": {}, "status": "FAILED"}
        def run(command, **kwargs):
            try:
                outcome = subprocess.run(command, cwd=ROOT, text=True, capture_output=True, timeout=60, **kwargs)
            except subprocess.TimeoutExpired as error:
                def decoded(value):
                    return value.decode(errors="replace") if isinstance(value, bytes) else (value or "")
                report["commands"].append({"argv": command, "returncode": None, "timed_out": True,
                                           "stdout": decoded(error.stdout), "stderr": decoded(error.stderr)})
                raise
            report["commands"].append({"argv": command, "returncode": outcome.returncode,
                                       "stdout": outcome.stdout, "stderr": outcome.stderr})
            outcome.check_returncode()
            return outcome.stdout
        try:
            cc = os.environ.get("CC", "cc")
            report["compiler"] = run([cc, "--version"])
            report["baseline"] = run(["git", "rev-parse", "--verify", args.baseline + "^{commit}"]).strip()
            report["head"] = run(["git", "rev-parse", "HEAD"]).strip()
            report["git_status"] = run(["git", "status", "--short"])
            with tempfile.TemporaryDirectory(prefix="keel-json-bench-") as directory:
                binaries = {}
                for label in ["baseline", "working_tree"]:
                    sources = {}
                    for name in ["runtime", "stdlib"]:
                        sources[name] = (run(["git", "show", f"{report['baseline']}:src/{name}.c"])
                                         if label == "baseline" else (ROOT / f"src/{name}.c").read_text())
                    report["sources"][label] = {name: hashlib.sha256(value.encode()).hexdigest() for name, value in sources.items()}
                    code = harness(sources["runtime"], sources["stdlib"], args.members, args.iterations)
                    path = Path(directory) / f"{label}.c"
                    path.write_text(code)
                    binary = Path(directory) / label
                    run([cc, *report["flags"], str(path), "-o", str(binary)])
                    binaries[label] = binary
                order = [label for label in binaries for _ in range(args.repeats)]
                random.Random(42).shuffle(order)
                report["order_seed"] = 42
                for label in order:
                    sample = json.loads(run([str(binaries[label])]))
                    report["samples"].append({"variant": label, **sample})
                report["median_seconds"] = {label: statistics.median(s["seconds"] for s in report["samples"] if s["variant"] == label) for label in binaries}
                report["status"] = "COMPLETE"
        finally:
            # Source contents are already identified by hashes; avoid bloating the log.
            for command in report["commands"]:
                if command["argv"][:2] == ["git", "show"] and command["returncode"] == 0:
                    command["stdout"] = "[source identified by SHA-256]"
            json.dump(report, destination, indent=2)
            destination.write("\n")
    print(args.output)


if __name__ == "__main__":
    main()
