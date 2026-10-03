# Server/data validation record

Review baseline: `73f7ba0` (Keel 0.1.0). Local environment on 2026-10-03:
macOS ARM64, Darwin 27.2.0; rustc/cargo 1.98.1; Apple clang 21.0.0.
These results cover this patch in the working tree, which also contained ongoing
user-owned game/site edits. Those unrelated edits are excluded from this patch.
Remote Linux/Intel results for this patch are not established by local execution.

| Command | Local result |
| --- | --- |
| `cargo fmt --check` | Passed |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed |
| `cargo test --locked --all-targets` | Passed: 77 compiler/runtime tests, 11 agent workflows, 3 CLI presentation tests, 20 public CLI tests (111 total) |
| `cargo test --locked --test e2e all_application_examples -- --nocapture` | Passed after extending worker test coverage |
| `python3 -m unittest discover -s benchmarks -p 'test_*.py'` | Passed: 57 tests |
| `python3 -m unittest discover -s scripts/tests -p 'test_*.py'` | Passed: 21 installer/site tests |
| `python3 -m unittest discover -s examples/evolving_arena -p 'test_*.py' -v` | Passed: 6 offline tests |
| `python3 -m unittest discover -s examples/jev_pong -p 'test_*.py' -v` | Passed: 8 offline tests |
| `cargo run --locked -- test examples/catalog_api --engine both` | TESTED, no native/reference mismatches |
| `python3 benchmarks/json_lookup.py --baseline 73f7ba0 --output NEW.json` | Completed 7 samples per variant; independent lookup assertions passed |

The public CLI example test checks and builds four standalone programs, six
application projects and four worker manifests. It runs both-engine tests for
all of these that define tests, including both arena workers. Pong workers with
empty suites are built and exercised by offline process fixtures; an empty suite
is not reported as TESTED. Deliberate `holes.keel` and `counterexample.keel`
fixtures retain their separate BLOCKED/FAILED expectations.

New coverage includes explicit expected JSON output/decimal spelling, escaped
pointer keys, root/array replacement, duplicate-member semantics, missing paths,
malformed and oversized input, output size/depth bounds, returned-value lifetime,
header case/duplicates/folding, query decoding/duplicates/invalid UTF-8, static
checker rejection, effect propagation, and source-guide examples. Loopback tests
compile the actual catalog routes with address/undefined-behavior sanitizers and
exercise fragmented JSON requests, HTTP failures, host permissions, slow clients,
and recovery. A socketpair fixture verifies total response deadlines against a
peer that does not consume output.

During development, the new fixture initially used an unsupported source-string
escape and the example manifest omitted its entry field; both were corrected.
Adding a sixth runnable tour example required increasing the tour's expected count.
Initial socket tests also had intermittent startup/early-close failures: readiness
now waits for the launched server's own startup message, and rejected requests can
stop accepting writes while their response is still checked. Successful-response
assertions and the total-deadline thresholds remain unchanged. Five subsequent
repeated runs of all seven HTTP tests passed; the exact cause of the earlier
intermittent failures was not independently established. An early broad
CLI run failed without diagnostics while other builds were active; subsequent
validation was sequenced to avoid replacing executables during those tests.
These intermediate failures are not counted as successful runs.

Sanitizer fixtures use bounded execution time with `memory_mib = 0`; ASan maps a
large shadow address space, incompatible with the ordinary Linux RLIMIT_AS cap.
This follows the existing collection-test setup and corrects the stdlib fixture's
4 GiB cap. Normal native worker memory limits and their tests are unchanged.
See [LLVM's ASan limitations](https://clang.llvm.org/docs/AddressSanitizer.html#limitations)
and [issue #5](https://github.com/jakecyr/keel/issues/5). This Linux startup concern
was found by source review; a local Linux reproduction was not performed.

Address/UB sanitizer instrumentation is enabled for the added fixtures. Live servers
are killed after assertions, with leak detection disabled for those server processes;
this does not establish leak-free graceful shutdown. macOS worker address-space
limits remain unenforced. WebSocket wire coverage depends on the installed curl
build and is not upgraded from unavailable to successful evidence. No live inference,
paid integration, release publication, global installation or independent audit was
performed.

The [raw JSON lookup report](../benchmarks/results/json-lookup-2026-10-03.json)
contains all timing samples, commands, source hashes and allocation-request counts.
It measures a narrow runtime workload, not production throughput, peak RSS,
agent tokens, or dollar cost. Production readiness remains unestablished for the
reasons in [the review](server-readiness.md) and [release gates](release-gaps.md).
