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

## Iterate on Keel agent efficiency

`efficiency.py` is a separate **Keel-only workflow experiment**, not a replacement
for the original four-language/tool-condition pilot or its economic gate. It tests:

| Variant | Initial context | Editing and validation |
| --- | --- | --- |
| `protocol` | Agent retrieves focused inspection | Structural edit/check, then public tests |
| `full_context` | Full `keel agent context` plus public files supplied upfront | Same separate validation |
| `compact_context` | Focused source/revision/dependencies, public files, exact relevant APIs | Same separate validation |
| `compact_combined` | Same compact packet | Structural edit with public-test validation in one invocation |

An opt-in fifth variant, `compact_guided`, adds a short collection or parsing syntax
example selected from the public input types/APIs to `compact_combined`. It was
introduced after development traces showed agents retrieving the entire collections
reference despite having API signatures. Compare it directly before promoting it:

```sh
python3 benchmarks/efficiency.py plan --model MODEL_ID --repeats 2 \
  --tasks unique copy_append --variants compact_combined compact_guided \
  --output build/efficiency-guidance-plan.json
```

All arms receive the same short syntax primer, requirements, public fixtures,
compiler and available tools. These arms isolate workflow *bundles*: compact versus
full also changes API presentation, and combined changes both call count and
successful output verbosity. They do not isolate every individual mechanism.
Test-generation tasks use text edits in every arm because body replacement cannot
add test blocks. For those tasks, the combined arm has the same workflow as compact
context and serves as a repeat/noise check. Conditions remain instructed rather than
enforced; inspect raw commands before interpreting results.

The compact packet is currently a **benchmark adapter**, not a new compiler CLI
mode. Its complete serialized contents must fit 12 KB; it fails rather than silently
omitting source. Prompt bytes are measured as bytes, never mislabeled as tokens.
Full context comes from the compiler being tested. Both upfront arms include the
same public project files. The preparation time is recorded separately from agent
wall time; its tool dollar cost remains unknown.

Development tasks cover scalar repair, implementation from a requirement, collections,
ownership/copy independence, an integration across source files, and regression-test
generation. Validation tasks cover a different collection operation and a different
test target. Keep validation tasks out of prompt tuning. These are small public
fixtures, not contamination-resistant or representative production repositories.

Code acceptance uses independent assertions and **both native and reference engines**.
Test generation is scored using agent-written tests alone: they must pass two correct
implementations and fail every registered mutant through executed assertions in
both engines. Compiler errors, timeouts, empty tests, and tests that reject correct
implementations cannot earn acceptance. Public tests do not earn mutation credit.
Evaluator fixtures, mutants and reference solutions are never copied into the agent
workspace or prompts. The temporary directory is not an adversarial read boundary.

### Offline verification (no inference)

```sh
cargo build --locked
python3 -m unittest discover -s benchmarks -p 'test_*.py' -v
python3 benchmarks/efficiency.py preflight \
  --output build/efficiency-preflight.json
```

Preflight validates every reference solution and verifies that the starter fails
independent acceptance. It also checks mutation scoring. This is plumbing evidence,
not an agent-efficiency measurement. CI runs the methodology tests without model calls.

### Register a bounded experiment, then run it explicitly

```sh
python3 benchmarks/efficiency.py plan --model MODEL_ID --repeats 3 \
  --seconds 180 --tokens 32000 --split development \
  --output build/efficiency-dev-plan.json
# LIVE: consumes the configured Codex account's quota.
python3 benchmarks/efficiency.py run --plan build/efficiency-dev-plan.json \
  --output-dir build/efficiency-dev
python3 benchmarks/efficiency.py report --run-dir build/efficiency-dev \
  --output build/efficiency-dev-report.json
```

The default plan contains 72 trials (six tasks × four variants × three repetitions).
For a small pilot, register fewer tasks and variants **before** running:

```sh
python3 benchmarks/efficiency.py plan --model MODEL_ID --repeats 1 \
  --tasks boundary unique --variants protocol compact_combined \
  --output build/efficiency-smoke-plan.json
python3 benchmarks/efficiency.py run --plan build/efficiency-smoke-plan.json \
  --output-dir build/efficiency-smoke --max-trials 2
python3 benchmarks/efficiency.py run --plan build/efficiency-smoke-plan.json \
  --output-dir build/efficiency-smoke --resume
```

A plan freezes task selection, split, model argument, compiler hash, harness hash,
paired repetitions, budgets and randomized ordering. Changed tasks, compiler or
harness require a new plan and output directory. Existing completed failures are
never retried on resume. Raw events and stderr are saved during execution, with one
immutable record per attempt. If a process crashes before a record is written,
resume refuses to replace that attempt: retain its raw evidence and mark the run
incomplete. Missing token usage stops further live trials instead of spending quota
on repeated authentication/environment failures. Plans, records, and reports are
created exclusively, never overwritten. `run`/`report` exit 2 for incomplete studies
and 3 for invalid reports; COMPLETE means complete accounting, **not an advantage**.

### Read the evidence and iterate

1. Inspect failures and command traces on development tasks. Change one context or
   workflow mechanism at a time; preserve all assertions and generator domains.
2. Register a new plan after changes. Keep previous plans, prompts, compiler hashes,
   failed attempts, raw event ledgers and reports. Do not retrofit the original pilot.
3. Compare tokens **per independently accepted change**, counting all failed and
   over-budget attempts in the numerator. Report correctness before budget policy,
   acceptance rate, and paired regressions separately. Missing usage and a zero
   acceptance denominator yield null, not zero or a win.
4. Reports show total/cached/output tokens, tool calls, failed commands, prompt bytes,
   time, and per-task outcomes. Completed CLI turns are not model-request counts:
   request-level token attribution remains unavailable. Cached input is already part
   of input usage; do not add it again or remove it from the registered budget.
5. A development candidate must reduce reported tokens per accepted change without
   losing any paired baseline acceptance. The exploratory task-cluster bootstrap
   interval concerns total token reduction, not acceptance-adjusted dollar costs;
   small samples require more repetitions and tasks.
6. Freeze the candidate and evaluate the reserved split, without tuning on its results:

   ```sh
   python3 benchmarks/efficiency.py plan --model MODEL_ID --repeats 3 \
     --split validation --variants protocol compact_combined \
     --output build/efficiency-validation-plan.json
   python3 benchmarks/efficiency.py run --plan build/efficiency-validation-plan.json \
     --output-dir build/efficiency-validation
   ```

Token caps are checked after a turn because the CLI exposes aggregate completion
usage, not a reliable live aggregate cap. The runner retains usage from failures.
Do not raise a completed experiment's budget or discard failed tasks to create a
win. Any new diagnostic budget must be preregistered and reported separately.
Dollar costs and enforced condition isolation remain UNKNOWN. Even a successful
Keel workflow comparison does not establish superiority over improved C; that still
requires the separate four-condition study, billing evidence and broader tasks.

The runner uses the documented [Codex non-interactive JSON event interface](https://learn.chatgpt.com/docs/non-interactive-mode)
with workspace-write sandboxing, ephemeral sessions, ignored user configuration and
no approval escalation. Model selection is pinned by argument; backend identity and
provider defaults are not independently attested.
