# Recorded local measurements

Measured 2026-10-03 02:30 UTC (2026-10-02 evening in New York), on an Apple M1
with 8 logical CPUs and 16 GiB RAM, macOS 27.2 ARM64. Rust 1.98.1 and Apple
Clang 21.0.0 were installed. Compiler SHA-256:
`d6f8767c9b1ad02b70a4acff75d08cc8de8e892240e3a5a5f41c098be1cdc218`.
There was no Git repository, so a commit revision could not be recorded.

The complete raw observations and command lines are in [local.json](local.json).
Each repeated measurement used 7 samples and a generated corpus of 2,500 tiny
private functions (10,003–10,006 lines, depending on language). These functions
are mostly unused and can be eliminated; this is a frontend/iteration workload,
not a representative application or generic-instantiation benchmark.

| Measurement | Keel | C | Rust |
| --- | ---: | ---: | ---: |
| CLI check, P50 | 12.03 ms | 48.92 ms | 182.85 ms |
| CLI check, P95 | 12.90 ms | 50.97 ms | 199.32 ms |
| Edited build, P50 | 176.66 ms | 93.47 ms | 298.79 ms |
| Edited build, P95 | 276.04 ms | 99.98 ms | 349.65 ms |
| First build, fresh outputs, uncontrolled OS cache | 343.84 ms | 102.69 ms | 2,619.76 ms |
| Bounded recurrence executable, P50 including startup | 10.69 ms | 11.29 ms | 10.95 ms |
| Bounded recurrence executable, P95 including startup | 62.23 ms | 61.85 ms | 71.68 ms |
| Minimal stripped executable | 33,592 B | 16,832 B | 341,560 B |

The Keel resident service used a 64 MiB estimated cache budget. Exact-source
cache-hit round trips had P50 1.016 ms / P95 1.179 ms. Unique function-body edits
missed that cache and had P50 4.937 ms / P95 5.305 ms. The service peak RSS was
18,710,528 bytes (17.84 MiB). The recorded cache reported 7 hits and 8 misses,
confirming the measurement labels. This cache stores whole-source snapshots;
these timings do not establish declaration-level incremental compilation.

The full web-example native test command, including compilation and execution,
had P95 259.53 ms. All measured commands succeeded. This is not first-affected-test
latency or a claim that unrelated tests were skipped. Peak memory is the platform's
`/usr/bin/time` metric, not aggregate concurrent process-tree residency.

On this fixture the warm body-edit feedback, minimal binary size, and measured
resident-process memory met their numerical targets. The cold-cache build and
affected-test targets remain UNKNOWN because those specific behaviors were not
measured. Seven samples are a small local observation; timings and percentile
estimates are sensitive to load, OS caching, and process startup. C also supplies
fewer static guarantees than Keel, and Rust supports a much broader language.

## Real agent pilot

The registered [pilot-plan.json](pilot-plan.json) was executed without changing
its thresholds: `gpt-6-astra`, Codex CLI 0.159.3, one repetition of three tasks in
each of four conditions, 180 seconds and 32,000 total reported input-plus-output
tokens per trial. The compiler was frozen to the same SHA-256 used above. These
were real model calls using the existing ChatGPT login; no API key or paid provider
was provisioned. Full event/usage/artifact evidence is in
[pilot-trials.json](pilot-trials.json), and the evaluated report is in
[pilot-trials.gate.json](pilot-trials.gate.json).

| Condition | Native assertions passed | Accepted within registered budget | Total reported input + output tokens | Total agent wall time |
| --- | ---: | ---: | ---: | ---: |
| C, conventional tools | 3/3 | 0/3 | 205,539 | 67.28 s |
| C, improved JSON tools | 3/3 | 0/3 | 153,550 | 77.78 s |
| Keel, text editing | 3/3 | 0/3 | 182,244 | 79.13 s |
| Keel, compiler protocol | 3/3 | 0/3 | 216,011 | 78.96 s |

Every repair passed the independent native assertions and preserved protected
fixtures, but every trial exceeded the preregistered 32k aggregate-token budget.
CLI/system context repeated across tool calls contributed substantial input usage.
Those attempts remain rejected, and all their reported usage remains in the
ledger. No thresholds were relaxed after seeing the results. A higher-budget
experiment would need a new registered plan, not rescoring these as accepted.

Token totals include cached input; cached and uncached breakdowns are preserved in
the JSON. They are not dollar costs. In this tiny pilot Keel protocol used more
reported tokens than the improved C baseline; the pilot does not show the proposed
efficiency advantage. All acceptance denominators are zero under the registered
budget, actual inference/tool dollar costs are unavailable, tool-condition
isolation is instructed rather than enforced, and only three public microtasks
were tested once. The economic/adoption gate is therefore **UNKNOWN**, with exit
status 2. This is an observed limitation, not a passing benchmark or evidence of
production readiness.

Local timings and this pilot do not establish a 25% reduction in actual
inference-plus-tool dollar cost per independently accepted change.

## Keel workflow optimization experiments

The [workflow study](efficiency.md) records 48 further live trials with varied code
and test-generation tasks. Compact context, focused syntax guidance and combined
validation reduced reported tokens in these Keel-to-Keel comparisons: 50.8% in the
six-task development comparison and 61.8% on two reserved tasks. These figures do
not establish an advantage over C or Rust; the original pilot and economic gate
above remain unchanged. All raw prompts, attempts, budgets and oracle results are
retained, including over-budget outcomes.
