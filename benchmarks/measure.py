#!/usr/bin/env python3
"""Reproducible local toolchain measurements. These are NOT agent trials."""
import argparse
import hashlib
import json
import math
import os
import platform
from pathlib import Path
import random
import re
import select
import shutil
import signal
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def percentile(samples, percent):
    """Nearest-rank percentile; retain raw observations for other estimators."""
    if not samples:
        raise ValueError("empty observations")
    return sorted(samples)[max(0, math.ceil(len(samples) * percent / 100) - 1)]


def summarize(samples):
    values = [s["elapsed_ms"] for s in samples if s["returncode"] == 0]
    return {
        "samples": samples,
        "successes": len(values),
        "failures": len(samples) - len(values),
        "p50_ms": statistics.median(values) if values else None,
        "p95_ms": percentile(values, 95) if values else None,
        "peak_rss_bytes": max((s.get("peak_rss_bytes") or 0 for s in samples), default=0) or None,
    }


def measure(command, cwd, timeout=120):
    """Process startup included. Time is a child, never a shell expression."""
    command = [str(s) for s in command]
    with tempfile.TemporaryDirectory(prefix="keel-timing-") as directory:
        timing = Path(directory) / "time.txt"
        wrapped = command
        if Path("/usr/bin/time").is_file() and sys.platform in ("darwin", "linux"):
            wrapped = ["/usr/bin/time", "-l" if sys.platform == "darwin" else "-v", "-o", str(timing), *command]
        started = time.perf_counter_ns()
        process = subprocess.Popen(wrapped, cwd=cwd, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, start_new_session=True)
        timed_out = False
        try:
            stdout, stderr = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            os.killpg(process.pid, signal.SIGKILL)
            stdout, stderr = process.communicate()
        elapsed = (time.perf_counter_ns() - started) / 1_000_000
        peak = None
        if timing.exists():
            report = timing.read_text()
            if sys.platform == "darwin":
                match = re.search(r"(\d+)\s+maximum resident set size", report)
                peak = int(match.group(1)) if match else None
            elif sys.platform == "linux":
                match = re.search(r"Maximum resident set size \(kbytes\):\s*(\d+)", report)
                peak = int(match.group(1)) * 1024 if match else None
        return {"command": command, "elapsed_ms": elapsed,
                "returncode": process.returncode, "timed_out": timed_out,
                "peak_rss_bytes": peak,
                "stdout_truncated": len(stdout.decode(errors="replace")) > 8192,
                "stderr_truncated": len(stderr.decode(errors="replace")) > 8192,
                "stdout": stdout.decode(errors="replace")[-8192:],
                "stderr": stderr.decode(errors="replace")[-8192:]}


def version(command):
    try:
        return subprocess.check_output(command, stderr=subprocess.STDOUT, text=True).strip()[:2000]
    except (OSError, subprocess.CalledProcessError) as error:
        return str(error)


def service_benchmarks(keel, declarations, repeats):
    """Separate exact-source cache hits from unique body edits (whole-source misses)."""
    with tempfile.TemporaryDirectory(prefix="keel-service-bench-") as directory:
        timing = Path(directory) / "time.txt"
        command = [str(keel), "serve", "--max-cache-mib", "64"]
        wrapped = command
        if Path("/usr/bin/time").is_file() and sys.platform in ("darwin", "linux"):
            wrapped = ["/usr/bin/time", "-l" if sys.platform == "darwin" else "-v", "-o", str(timing), *command]
        process = subprocess.Popen(wrapped, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True, start_new_session=True)
        sequence = 0

        def request(method, source=None):
            nonlocal sequence
            sequence += 1
            payload = {"id": sequence, "method": method}
            if source is not None:
                payload["source"] = source
            started = time.perf_counter_ns()
            process.stdin.write(json.dumps(payload) + "\n")
            process.stdin.flush()
            if not select.select([process.stdout], [], [], 30)[0]:
                raise TimeoutError("service response exceeded 30 seconds")
            response = json.loads(process.stdout.readline())
            elapsed = (time.perf_counter_ns() - started) / 1_000_000
            status = response.get("result", {}).get("status")
            return {"elapsed_ms": elapsed, "returncode": 1 if "error" in response or (method == "check" and status != "CHECKED") else 0,
                    "response": response, "source_sha256": hashlib.sha256(source.encode()).hexdigest() if source else None}

        try:
            source = corpus("keel", declarations)
            first = request("check", source)
            before_hits = request("stats")["response"].get("result", {})
            hits = [request("check", source) for _ in range(repeats)]
            after_hits = request("stats")["response"].get("result", {})
            misses = [request("check", source.replace("let delta = 0", f"let delta = {i + 1}", 1)) for i in range(repeats)]
            stats = request("stats")
            request("shutdown")
            _, stderr = process.communicate(timeout=10)
            peak = None
            if timing.exists():
                pattern = r"(\d+)\s+maximum resident set size" if sys.platform == "darwin" else r"Maximum resident set size \(kbytes\):\s*(\d+)"
                match = re.search(pattern, timing.read_text())
                if match:
                    peak = int(match.group(1)) * (1024 if sys.platform == "linux" else 1)
            return {"command": command, "first_check": first, "exact_source_cache_hits": summarize(hits),
                    "unique_body_edit_cache_misses": summarize(misses), "final_stats": stats["response"],
                    "cache_behavior_verified": after_hits.get("hits", 0) - before_hits.get("hits", 0) == repeats and stats["response"].get("result", {}).get("misses", 0) - after_hits.get("misses", 0) == repeats,
                    "peak_rss_bytes": peak, "returncode": process.returncode, "stderr": stderr,
                    "incrementality": "Whole-source cache only. A changed function reparses/rechecks the source.",
                    "timing_scope": "JSON serialization/write/read round trip; first request includes startup, later requests exclude it"}
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
                process.communicate()


def corpus(language, declarations=2500):
    """10,000 declaration lines by default; all functions are intentionally tiny."""
    if language == "keel":
        return "".join(f"fn item_{i}(value: Int) -> Int {{\n    let delta = {i}\n    return value + delta\n}}\n" for i in range(declarations)) + f"fn main() {{\n    assert item_{declarations - 1}(7) == {declarations + 6}\n}}\n"
    if language == "rust":
        return "#![allow(dead_code)]\n" + "".join(f"fn item_{i}(value: i64) -> i64 {{\n    let delta = {i};\n    return value + delta;\n}}\n" for i in range(declarations)) + f"fn main() {{\n    assert_eq!(item_{declarations - 1}(7), {declarations + 6});\n}}\n"
    return "#include <assert.h>\n#include <stdint.h>\n" + "".join(f"static int64_t item_{i}(int64_t value) {{\n    const int64_t delta = {i};\n    return value + delta;\n}}\n" for i in range(declarations)) + f"int main(void) {{\n    assert(item_{declarations - 1}(7) == {declarations + 6});\n    return 0;\n}}\n"


def runtime_source(language, iterations):
    expected = 17
    for _ in range(iterations):
        expected = (expected * 48271) % 2147483647
    if language == "keel":
        return f"fn main() {{\n    var value = 17\n    var i = 0\n    while i < {iterations} {{\n        value = (value * 48271) % 2147483647\n        i = i + 1\n    }}\n    assert value == {expected}\n}}\n"
    if language == "rust":
        return f"fn main() {{ let mut value: i64 = 17; for _ in 0..{iterations} {{ value = (value * 48271) % 2147483647; }} assert_eq!(value, {expected}); }}\n"
    return f"#include <stdint.h>\n#include <assert.h>\nint main(void) {{ int64_t value = 17; for (int i = 0; i < {iterations}; ++i) {{ value = (value * 48271) % 2147483647; }} assert(value == {expected}); return 0; }}\n"


def commands(language, source, binary, keel):
    if language == "keel":
        return [keel, "check", source, "--json"], [keel, "build", source, "-o", binary, "--json"]
    if language == "rust":
        common = ["rustc", "--edition=2024", "-C", "opt-level=2", "-C", "debuginfo=2", "-C", "overflow-checks=yes"]
        return [*common, "--emit=metadata", source, "-o", str(binary) + ".rmeta"], [*common, source, "-o", binary]
    common = [os.environ.get("CC", "cc"), "-std=c11", "-O2", "-g"]
    return [*common, "-fsyntax-only", source], [*common, source, "-o", binary]


def local_benchmarks(keel, repeats=7, declarations=2500, iterations=1_000_000, seed=42):
    keel = str(Path(keel).resolve())
    if not Path(keel).is_file():
        raise ValueError("Build a release compiler first: cargo build --release --locked")
    binary_hash = digest(keel)
    result = {
        "schema_version": 1, "kind": "local_toolchain_measurements", "agent_efficiency": "UNKNOWN",
        "started_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "environment": {"platform": platform.platform(), "machine": platform.machine(),
                        "python": sys.version, "cpu_count": os.cpu_count(),
                        "cpu_model": version(["sysctl", "-n", "machdep.cpu.brand_string"]) if sys.platform == "darwin" else platform.processor(),
                        "physical_memory_bytes": version(["sysctl", "-n", "hw.memsize"]) if sys.platform == "darwin" else os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES"),
                        "git_revision": version(["git", "rev-parse", "HEAD"]),
                        "git_status": version(["git", "status", "--porcelain"]),
                        "rustc": version(["rustc", "--version"]), "cc": version([os.environ.get("CC", "cc"), "--version"]),
                        "keel_binary_sha256": binary_hash},
        "configuration": {"repeats": repeats, "declarations": declarations,
                          "runtime_iterations": iterations, "ordering_seed": seed},
        "limitations": [
            "CLI figures use process-per-invocation; service figures separately label exact-source cache hits and unique body-edit misses.",
            "First build uses fresh outputs with uncontrolled OS caches; it is NOT a true cold-cache build.",
            "Repeated builds/checks use a warm OS cache but new compiler processes.",
            "10k-line corpus consists of tiny independent functions; it does not represent large generic or dependency graphs.",
            "C does not provide Keel's ownership/effect checks; timings do not measure equivalent static assurance.",
            "Runtime recurrence stays in signed 64-bit range; it is a narrow throughput/startup microbenchmark.",
            "Peak RSS is /usr/bin/time's platform-dependent maximum for a command, not aggregate concurrent process-tree RSS.",
            "No hosted model is run; these timings cannot establish a 25% agent-cost advantage.",
        ], "languages": {},
    }
    rng = random.Random(seed)
    with tempfile.TemporaryDirectory(prefix="keel-bench-") as directory:
        workspace = Path(directory)
        specs = {}
        for language, extension in [("keel", "keel"), ("rust", "rs"), ("c", "c")]:
            source = workspace / f"corpus.{extension}"
            source.write_text(corpus(language, declarations))
            binary = workspace / f"corpus-{language}"
            check, build = commands(language, source, binary, keel)
            specs[language] = (source, binary, check, build)
            result["languages"][language] = {"source_lines": len(source.read_text().splitlines()),
                                             "source_sha256": digest(source)}
        order = list(specs)
        rng.shuffle(order)
        for language in order:
            source, binary, check, build = specs[language]
            result["languages"][language]["first_build_fresh_outputs_os_cache_uncontrolled"] = measure(build, workspace)
        for operation in ("check", "build"):
            samples = {language: [] for language in specs}
            for trial in range(repeats):
                order = list(specs)
                rng.shuffle(order)
                for language in order:
                    source, binary, check, build = specs[language]
                    # A body-only edit prevents this being just repeated identical source.
                    source.write_text(corpus(language, declarations).replace("let delta = 0", f"let delta = {trial % 2}" )
                                      .replace("const int64_t delta = 0", f"const int64_t delta = {trial % 2}"))
                    sample = measure(check if operation == "check" else build, workspace)
                    sample["trial_index"] = trial
                    sample["source_sha256"] = digest(source)
                    samples[language].append(sample)
            for language in specs:
                result["languages"][language][f"edited_{operation}_warm_os_cache_fresh_process"] = summarize(samples[language])
        for language, (source, binary, _, _) in specs.items():
            if binary.exists():
                result["languages"][language]["corpus_binary_bytes_unstripped"] = binary.stat().st_size
            source.write_text(runtime_source(language, iterations))
            _, build = commands(language, source, binary, keel)
            build_result = measure(build, workspace)
            result["languages"][language]["runtime_build"] = build_result
            if build_result["returncode"] == 0:
                result["languages"][language]["runtime_native"] = summarize([measure([binary], workspace) for _ in range(repeats)])
            source.write_text("fn main() {}\n" if language in ("rust", "keel") else "int main(void) { return 0; }\n")
            minimum_build = measure(build, workspace)
            result["languages"][language]["minimal_build"] = minimum_build
            if minimum_build["returncode"] == 0:
                result["languages"][language]["minimal_binary_bytes_unstripped"] = binary.stat().st_size
                strip = shutil.which("strip")
                if strip:
                    stripped = workspace / f"minimal-stripped-{language}"
                    shutil.copyfile(binary, stripped)
                    observation = measure([strip, stripped], workspace)
                    if observation["returncode"] == 0:
                        result["languages"][language]["minimal_binary_bytes_stripped"] = stripped.stat().st_size
        # Native example tests exercise compilation, process isolation, and assertions.
        result["web_example_native_test"] = summarize([
            measure([keel, "test", ROOT / "examples/web_server.keel", "--cases", "1000", "--seed", "42", "--json"], workspace)
            for _ in range(repeats)
        ])
    result["compiler_binary_unchanged"] = digest(keel) == binary_hash
    result["service"] = service_benchmarks(keel, declarations, repeats)
    result["compiler_binary_unchanged"] = result["compiler_binary_unchanged"] and digest(keel) == binary_hash
    keel_results = result["languages"]["keel"]
    check = result["service"]["unique_body_edit_cache_misses"]
    result["engineering_targets"] = {
        "warm_feedback_under_50ms": ("UNKNOWN: requires >=10000-line corpus" if declarations < 2500 else
            ("PASS" if check["failures"] == 0 and check["p95_ms"] < 50 else "FAIL")),
        "minimal_binary_under_1MiB": "UNKNOWN" if "minimal_binary_bytes_stripped" not in keel_results else
            ("PASS" if keel_results["minimal_binary_bytes_stripped"] < 1048576 else "FAIL"),
        "cold_build_under_2s": "UNKNOWN: OS caches were not controlled",
        "affected_test_under_500ms": "UNKNOWN: affected-test selection is not measured",
        "daemon_under_256MiB": ("UNKNOWN: requires >=10000-line corpus and measured RSS" if declarations < 2500 or result["service"]["peak_rss_bytes"] is None else
            ("PASS" if result["service"]["peak_rss_bytes"] < 256 * 1024 * 1024 else "FAIL")),
        "agent_cost_reduction_25_percent": "UNKNOWN: requires independently accepted, metered model trials",
    }
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keel", type=Path, default=ROOT / "target/release/keel")
    parser.add_argument("--repeats", type=int, default=7)
    parser.add_argument("--declarations", type=int, default=2500)
    parser.add_argument("--iterations", type=int, default=1_000_000)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--strict", action="store_true", help="Require ALL original targets; UNKNOWN exits 2, FAIL exits 1")
    args = parser.parse_args()
    if args.repeats < 2 or args.declarations < 1 or not 1 <= args.iterations <= 100_000_000:
        parser.error("Require >=2 repeats, >=1 declaration, and 1..100000000 iterations")
    report = local_benchmarks(args.keel, args.repeats, args.declarations, args.iterations, args.seed)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"report": str(args.output), "targets": report["engineering_targets"]}, indent=2))
    failed = not report["compiler_binary_unchanged"] or any(
        stats.get("failures", 0) or stats.get("returncode", 0)
        for language in report["languages"].values() for stats in language.values() if isinstance(stats, dict)
    ) or report["web_example_native_test"]["failures"] or report["service"]["returncode"] or not report["service"]["cache_behavior_verified"] or report["service"]["first_check"]["returncode"] or report["service"]["exact_source_cache_hits"]["failures"] or report["service"]["unique_body_edit_cache_misses"]["failures"]
    if failed:
        return 1
    if args.strict:
        statuses = report["engineering_targets"].values()
        return 1 if "FAIL" in statuses else (2 if any(s.startswith("UNKNOWN") for s in statuses) else 0)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
