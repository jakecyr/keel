# Validation evidence

Local validation uses macOS ARM64, Rust/Cargo 1.98.1, and Apple Clang 21.0.0.
Keel is experimental: these results concern the implemented subset, not the
soundness or completeness of the original design.

## Reproduce the checks

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
python3 -m unittest discover -s benchmarks -p 'test_*.py'
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
```

The Rust suite has 50 unit/audit/tooling tests, 10 developer/agent-workflow
integration tests, and 19 CLI/native/HTTP end-to-end tests. Many are table-driven
and exercise multiple programs. The Python suites have 38 benchmark-methodology
tests and 16 offline installer tests.

Coverage includes:

- Type/effect errors, missing returns, exhaustive matches, holes, and bounded
  malformed-source parsing.
- Owned Text/List/Result lifetimes, explicit moves, exclusive edit borrows,
  argument evaluation order, branch/loop ownership, and destructor paths.
- Checked arithmetic, safe indexing, recoverable integer parsing, independent
  deduplication oracles, and native/reference differential execution.
- AddressSanitizer/UndefinedBehaviorSanitizer executions of text and collection
  lifetimes. These cover exercised executions, not every possible program.
- Contract failures, generated integer boundaries, shrinking, replay, blocked
  holes, and time/resource exhaustion retaining UNKNOWN rather than passing.
- Actual native HTTP servers, fragmented/malformed requests, routes, response
  lengths, and runtime listener-permission checks.
- Revision-bound edit transactions, rollback, protected acceptance helpers,
  source/output aliases, symlinks, policy preservation, and staging-file safety.
- Compiler/worker deadlines, bounded diagnostic pipes, detached descendants,
  nonregular input rejection, service cache invalidation/eviction, and JSON errors.
- Init/build/run/test workflows, preserved human agent instructions, idempotent
  reinitialization, offline agent references, focused context, lint, and formatting.
- Installer checksums, archive-member validation, supported platform mappings,
  pipe-style execution, and preservation of existing binaries on failure.
- Benchmark ledger integrity, budget handling, paired comparison requirements,
  missing evidence, and refusal to turn UNKNOWN economics into a passing result.

Linux/macOS GitHub Actions configuration is in
[ci.yml](../.github/workflows/ci.yml). Local checks do not establish that remote CI
has run; no GitHub run or release was performed during this implementation.
Linux memory enforcement is configured through RLIMIT_AS; it has not been
validated on Linux by this local macOS run. macOS worker memory is not enforced.

## Example projects and installation

```sh
cargo build --release --locked
./target/release/keel test examples/web --engine both --cases 1000 --seed 42
./target/release/keel test examples/collections.keel --engine both --cases 1000 --seed 42
./target/release/keel build examples/web -o build/server
```

The manifest web project has four examples and one integer property: 1,004 cases
per engine at the above budget. The original single-file `web_server.keel`
fixture has an additional property, for 2,004 cases. Actual TCP behavior is tested separately by the host
integration suite. `examples/holes.keel` deliberately reports BLOCKED and
`examples/counterexample.keel` deliberately fails; their nonzero exits are
expected and regression-tested.

The installer tests are offline fixture tests, not downloads from a published
release. CI packages archives and checksums but does not publish them. The source
installation smoke uses a temporary Cargo prefix, leaving the user's normal
installation and shell configuration untouched.

## Measured performance and agent trials

[Recorded results](../benchmarks/results/README.md) include raw timing and real
agent-pilot evidence. On the specified approximately 10,000-line fixture,
resident-service body-edit feedback had p95 5.305 ms; this was a whole-source
cache miss, not declaration-level incrementality. Cold-cache and affected-test
targets remain unestablished.

All 12 real-agent pilot repairs passed independent native assertions, but all
exceeded the preregistered aggregate-token budget. Consequently none was accepted
under that experiment's rules. Keel's protocol condition used more reported
tokens than the improved C baseline. Actual monetary costs and enforced
condition isolation were unavailable. The 25% cost-reduction goal is **UNKNOWN,
not achieved**; thresholds and failed attempts have not been rewritten.

## Remaining limits

There is no soundness proof, sustained frontend/native fuzz campaign, external
security review, distributed simulation, production HTTP certification, signed
release, or comprehensive platform validation. The reference evaluator supports
the current test subset, not arbitrary external host effects. Acceptance files
are protected by structural-edit policy, not against arbitrary filesystem access.
See [release gaps](release-gaps.md) and [machine-readable status](design-status.json).
