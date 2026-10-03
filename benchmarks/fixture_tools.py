#!/usr/bin/env python3
"""Small C baseline JSON protocol copied into trial workspaces (no held-out cases)."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("operation", choices=["inspect", "check", "test", "edit"])
    parser.add_argument("--request", type=Path)
    args = parser.parse_args()
    path = Path("solution.c")
    source = path.read_text()
    task = json.loads(Path("public_task.json").read_text())
    revision = hashlib.sha256(source.encode()).hexdigest()
    if args.operation == "inspect":
        print(json.dumps({"revision": revision, "target": "fn:" + task["function"], "source": source,
                          "requirements": task["requirement"], "public_cases": task["public_cases"],
                          "effects": [], "incomplete": False}))
        return 0
    candidate = source
    if args.operation == "edit":
        request = json.loads(args.request.read_text())
        if request["base_revision"] != revision or request["target"] != "fn:" + task["function"] or request["operation"] != "replace_body":
            print(json.dumps({"status": "FAILED", "reason": "stale revision, invalid target or operation"}))
            return 1
        body = request["source"].strip()
        if not body.startswith("{") or not body.endswith("}"):
            return 1
        candidate = source[:source.index("{")] + body + "\n"
    checks = "\n".join(f"if ({task['function']}({', '.join(map(str, c['args']))}) != {int(c['expected'])}LL) return 1;" for c in task["public_cases"])
    with tempfile.TemporaryDirectory(prefix="public-check-", dir=".") as directory:
        file = Path(directory) / "check.c"
        binary = Path(directory) / "check"
        file.write_text("#include <stdbool.h>\n#include <stdint.h>\n" + candidate + "\nint main(void) {\n" + checks + "\nreturn 0;\n}\n")
        command = ["cc", "-std=c11", "-O2", "-Wall", "-Wextra", str(file), "-o", str(binary)]
        result = subprocess.run(command, capture_output=True, text=True, timeout=10)
        if result.returncode == 0 and args.operation != "check":
            result = subprocess.run([str(binary.resolve())], capture_output=True, text=True, timeout=5)
        if result.returncode == 0 and args.operation == "edit":
            path.write_text(candidate)
        print(json.dumps({"status": "PASSED" if result.returncode == 0 else "FAILED",
                          "stdout": result.stdout, "stderr": result.stderr,
                          "assurance": "Public examples only; independent acceptance not run"}))
        return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
