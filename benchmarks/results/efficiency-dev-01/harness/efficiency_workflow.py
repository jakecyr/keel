#!/usr/bin/env python3
"""Public trial helper. No evaluator fixtures, model calls, or acceptance answers."""
import json
from pathlib import Path
import subprocess
import sys


def main():
    action = sys.argv[1] if len(sys.argv) > 1 else "validate"
    if action == "validate" and len(sys.argv) == 2:
        command = ["./keel", "test", ".", "--json"]
    elif action == "edit" and len(sys.argv) == 3:
        command = ["./keel", "edit", ".", "--request", sys.argv[2], "--json"]
    else:
        raise SystemExit("usage: python3 workflow.py validate | edit REQUEST.json")
    process = subprocess.run(command, capture_output=True, text=True, timeout=45)
    try:
        result = json.loads(process.stdout)
    except ValueError:
        print(json.dumps({"status": "FAILED", "stdout": process.stdout, "stderr": process.stderr}))
        return 1
    # Return compact successful evidence; preserve full diagnostics on failure.
    if process.returncode == 0 and result.get("status") in ("APPLIED", "TESTED"):
        evidence = result.get("evidence") or result
        result = {"status": result["status"], "revision": result.get("revision"),
                  "validation": evidence.get("status"),
                  "tests": [{"name": t["name"], "status": t["status"], "cases": t.get("cases")}
                            for t in evidence.get("tests", [])],
                  "assurance": "Public cases only; independent acceptance runs after you finish."}
    print(json.dumps(result, separators=(",", ":")))
    return process.returncode


if __name__ == "__main__":
    sys.exit(main())
