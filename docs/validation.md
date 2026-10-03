# Prototype validation

Validated on macOS ARM64 with Rust 1.98.1, Cargo 1.98.1, and Apple Clang 21.0.0. These results concern this small prototype and its examples, not the performance or soundness of the full proposed language.

## Automated checks

`cargo test --offline` passes 17 regression tests. Several are table-driven and execute multiple independent rejection/runtime cases:

- Static mismatches, invalid parameters, immutable assignments, unknown names, missing returns, untyped holes, invalid entrypoints, forbidden early test returns, and out-of-range integer literals.
- Implicit Text copies, use after move, escaping borrows, loop/branch ownership, overlapping read/take arguments, and equality operand borrow conflicts.
- Direct/transitive effect violations, invalid host handlers, impure contracts, and holes in contracts.
- Native overflow traps for addition, subtraction, multiplication, negation, division, and remainder, plus division/remainder by zero.
- 500 generated integer arithmetic comparisons against Rust-computed expected values across signed inputs.
- Left-to-right argument failure order, Boolean short-circuiting, conditional ownership returns, and restoring moved values in loops.
- Runtime precondition/postcondition enforcement.
- Typed-hole context and a blocked test alongside an unaffected passing test.
- Full-width and singleton integer generators; a counterexample shrunk from 1,000 to 10 and replayed with the same failure.
- A hung worker timing out as UNKNOWN; an empty test selection never succeeding.
- Successful structural body replacement, unchanged interface/contracts/test source, stale-revision rejection, failed-candidate rollback, and declaration/effect injection rejection.
- HTTP listener permission denial, exact permission binding, actual TCP requests for all routes, query stripping, correct Content-Length, unsupported method handling, and malformed/incomplete request handling.
- 1,000 iterations of Text movement/rebinding, nested transfers, early ownership returns in other native tests, and UTF-8 values under AddressSanitizer and UndefinedBehaviorSanitizer.

## Example projects

```sh
./target/debug/keel test examples/web_server.keel --cases 1000 --seed 42 --json
```

Passes four example tests and two properties with 1,000 inputs each: **2,004 executed cases**. The HTTP regression test separately launches a real native server on an available loopback port and verifies its responses over TCP.

`examples/ownership.keel` passes both examples. `examples/holes.keel` intentionally reports one TESTED and one BLOCKED test. `examples/counterexample.keel` intentionally fails at `now == deadline == 0` and supplies a replay value. These nonzero exits are expected behavior.

## What this evidence does not establish

No formal type/ownership soundness proof, full-language reference evaluator, fuzzed frontend corpus, distributed simulation, production HTTP compliance, cross-platform certification, memory-budget isolation, or economic agent benchmark is supplied. Native sanitizers cover the exercised executions only; they are not a proof of all possible programs. Tests and acceptance criteria in this repository remain writable by anyone with repository write access.

The tiny example's successful native compilation does not establish the original cold/warm build targets on a 10,000-line project. No latency, memory, binary-size, or cost improvement is claimed without a matching benchmark protocol.
