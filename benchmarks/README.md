# Measurements and independent agent evaluation

This directory separates three kinds of evidence: local compiler/runtime timings,
optional real-model pilot trials, and an economic acceptance gate. Faster scripted
commands do not prove agents use fewer tokens or cost less.

## Run local measurements

```sh
cargo build --release --locked
python3 benchmarks/measure.py --repeats 7 --output benchmarks/results/local.json
python3 -m unittest discover -s benchmarks -p 'test_*.py' -v
```

The Python tools use only the standard library, `cc`, `rustc`, and a built release
Keel compiler. Raw JSON includes every command, outcome, observation, source/compiler
hash, seed, OS/architecture, tool versions, and Git state. Outputs are isolated in a
temporary directory. Measurement ordering is seeded and shuffled between languages.

The default corpus has 2,500 independent declarations and over 10,000 source lines.
Repeated CLI measurements change a function body and invoke a **new process**, with
warm OS caches. Separate `keel serve` measurements use a resident process and
report exact-source cache hits separately from unique body edits that miss the
whole-source cache. This does not claim declaration-level incrementality.
The first build has fresh
outputs but uncontrolled OS caches: it is not labeled a cold-cache measurement.
All languages use optimization level 2 and debug information; Rust overflow checks
are enabled. C does not supply Keel's ownership/effect guarantees. Runtime timings
use a bounded integer recurrence with an independently calculated final assertion;
process startup is included. This narrow workload is not a general performance claim.

P50 is the median; P95 is the nearest-rank percentile. Seven repetitions are a
small local sample, not a stable population estimate. `/usr/bin/time` reports peak
RSS when supported; it is not aggregate process-tree residency. The service's
peak RSS covers all cache-hit and cache-miss requests in that resident process.
The harness also measures stripped minimal binaries and the full web-example
native test command (compilation plus all selected tests, not first affected-test
latency). Hardware/load differences make cross-machine comparisons inappropriate.

`--strict` exits 1 if a measured original target fails, 2 if any target remains
unknown, and 0 only if every target is established. A compiler process failure
always exits 1. The original cold-cache, declaration-incrementality, affected-test and
agent-economic goals remain UNKNOWN unless separately established.

## Register and run agent trials

```sh
python3 benchmarks/agent_eval.py plan --model MODEL_ID --repeats 3 \
  --seconds 180 --tokens 32000 --output benchmarks/results/plan.json
# Optional: uses your configured Codex authentication and consumes its quota.
python3 benchmarks/codex_pilot.py --plan benchmarks/results/plan.json \
  --output benchmarks/results/trials.json
python3 benchmarks/agent_eval.py gate --plan benchmarks/results/plan.json \
  --results benchmarks/results/trials.json --output benchmarks/results/gate.json
```

The plan registers every task/repetition in all four conditions before execution:
existing language (C) with conventional tools, C with improved JSON tools, Keel
with ordinary text editing, and Keel with its development protocol. Trial order
is randomized with a fixed seed. Each trial uses the same pinned model argument,
time cap and total-token cap. The latter is checked after completion because the
CLI does not expose a reliable aggregate token hard cap; over-budget results are
rejected and their usage remains in the ledger.

The pilot copies only the task starter, public examples, and relevant tools into
a fresh temporary workspace. It uses `codex exec --json --ignore-user-config
--ephemeral --sandbox workspace-write --skip-git-repo-check`, with approval policy
`never`. It never disables the sandbox. Independent assertions are added by the
runner **after** the agent exits, in a separate workspace. Protected public fixture
or compiler changes invalidate acceptance. Output contains source, command,
prompt hash, raw events, total input/cached-input/output usage, elapsed time,
independent native acceptance evidence and a checksum. No trial ledgers are
overwritten. Incomplete runs retain completed attempts and cannot pass the gate.

The C improved condition has a small JSON inspect/check/test/revision-bound edit
tool with transactional public-test validation. It is useful for plumbing trials,
but it does not yet establish parity with an excellent existing-language language
server and agent environment. Tool restrictions in the pilot are instructions,
not enforced API boundaries, so `condition_isolation_verified` remains false.
Keel text versus protocol likewise remains an instructed distinction. The pilot
therefore cannot pass the economic gate even if every repair succeeds.

The current tasks are three small scalar repairs. Their assertions are held out
from the trial workspace, but are public in this repository, not secret or
contamination-resistant. A production study must add independent repository
changes, unfamiliar APIs, ownership, stateful and performance-sensitive tasks;
use stronger baseline tools; isolate allowed tools; collect actual billing and
tool cost evidence; repeat sufficiently; and estimate uncertainty. The runner's
temporary workspace is not an adversarial security boundary for untrusted native
code; run third-party agents and unknown generated programs inside an OS/container
sandbox. It is suitable for these controlled project fixtures.

Codex JSON token fields are read from `turn.completed` events according to
[the official non-interactive documentation](https://learn.chatgpt.com/docs/non-interactive-mode).
Subscription usage is not a known dollar cost. The pilot records both inference
and tool dollar costs as null, never as zero. Do not substitute token ratios for
the promised reduction in cost per accepted change.

## Gate and external adapter contract

`agent_eval.py gate` exits 0 (PASS), 1 (FAIL), 2 (UNKNOWN), or 3 (INVALID).

An external adapter may replace `codex_pilot.py`, retaining its record schema.
It must consume the registered plan; copy only public task data to the agent;
meter every inference and tool call, including failed attempts; execute
independent acceptance; and retain all planned outcomes. Dollar costs require a
`metering_evidence` reference to a trusted billing/compute ledger. Declare
`condition_isolation_verified` only when allowed tool surfaces are actually
enforced and audited. Freeze evidence in evaluator-owned storage inaccessible to
the agent. Checksums detect changed records and accidental corruption; they are
**not signatures**, and cannot authenticate a ledger rewritten by an adversary.
The gate trusts the evaluator and metering authority, never an agent's own claim
that it passed. For external evidence, independently replay acceptance first.

The gate rejects missing/duplicate trials, mismatched model/budgets, changed
plans/tasks, changed tested sources, changed event ledgers, negative/nonfinite
costs, and over-budget successes. Unknown usage, costs, or isolation yields
UNKNOWN. A zero acceptance denominator or zero-cost baseline also yields UNKNOWN.
All inference plus tool costs are summed, including failures, then divided by
independently accepted changes. Acceptance rate is reported separately. A cheaper
result cannot pass by abandoning tasks the baseline solved: every paired baseline
acceptance must also be accepted under Keel protocol. The 25% reduction is compared
against the less costly of the two existing-language baselines, not a deliberately
weak baseline. A gate PASS applies only to the registered sample; it is not a
statistical population claim or a claim that the language is production ready.

Tests use explicitly synthetic ledgers to verify this arithmetic and rejection
behavior. They never count as evidence that real agents are more efficient.
