# Release gaps and evidence required

Keel is an experimental implementation of a subset of the original design.
Passing the repository's regression suite is necessary evidence for that subset;
it does not establish language soundness, production security, general runtime
performance, or lower agent cost. This document records release criteria, not
claims that those criteria have already been met.

## Original-design coverage

| Area | Implemented scope | Remaining scope and release criterion |
| --- | --- | --- |
| Readable source | Functions, explicit signatures, local inference, comments, familiar control flow; conservative formatter | Publish a versioned grammar and semantic specification. Establish formatter token preservation and idempotence across a generated corpus. Formatting is currently conservative indentation/whitespace normalization, not a full canonical printer. |
| Data model | Int, Bool, Text, Unit, built-in List<Int>, Option<Int>, Result<Int, Text> | Nominal records, user-defined tagged unions, arbitrary simple generics, generic collections, and typed domain wrappers are absent. Each addition needs layout/ABI rules, exhaustive checking, ownership rules, and native differential tests. |
| Ownership | Explicit read/edit/take, owned text/list/result values, deterministic normal-path destruction, nonescaping borrows | No Shared<T>, escaping closures, owner-managed stores, checked graph handles, or async suspension. Document and review a formal ownership model, then validate it with adversarial aliases, branch joins, nested calls, returns, loops, and sanitizer/fuzz campaigns. |
| Recoverable errors | Exhaustive Option/Result match; checked optional indexing and integer parsing | General error types and explicit propagation syntax are absent. Define propagation cleanup and contract interaction before adding them. Allocation failure currently terminates rather than returning a typed error. |
| Integer semantics | Signed 64-bit arithmetic traps on overflow; evaluation order is explicit | Explicit checked/wrapping operations and other integer widths remain absent. Publish semantics for each added operation and maintain a reference evaluator independent of native lowering. |
| Effects and capabilities | Declared stdout/listen effects; separately supplied runtime allowlist policy; trusted native adapters | Capabilities are not first-class typed values. Filesystem, storage, clock, randomness, network clients, secret types, approved destination binding, and test substitution are absent. Build a host capability boundary with authority attenuation and enforceable deployment policy independent of editable source. |
| Concurrency | None | Structured scopes, bounded channels, ownership transfer, cancellation, deterministic scheduling simulations, and deadlock investigation are absent. Require race/lifetime review and bounded resource tests before advertising concurrency. |
| Agent context | Inspect returns selected source, dependencies, callers, signatures, holes and effects | No task/requirement graph, approved requirements retrieval, expression-level node handles, binding query API for arbitrary holes, or per-token generation constraints. Apply output budgets to metadata and dependency expansion as well as source snippets. Explicitly report omissions. |
| Structural edits | Revision-bound function body replacement; same-physical-file transactions; checks and optional tests; manifest acceptance-file protection | Expression/statement edits, rebasing, structural merges, and atomic multi-file transactions are absent. Protection is a local tool policy, not an OS security boundary against an actor with arbitrary filesystem writes. External acceptance ownership and approval enforcement require a separate trusted controller. |
| Contracts | Runtime pre/postcondition enforcement with pure expressions | No formal prover, specification library, bounded model checker, contract result database, or optimizer proofs. Retain clear UNKNOWN/TESTED/ENFORCED distinctions. Optional proofs must name their model and assumptions, and timeouts must stay UNKNOWN. |
| Property testing | Examples and one bounded integer generator per property; shrinking and replay | Generic record/variant/list generators, generated stateful operation sequences, rejection/vacuity detection, precondition-aware shrinking, fault injection, mutation testing, and controlled-world simulation are absent. Add independent oracles and verify shrinkers preserve their domain. |
| Diagnostics and evidence | Structured static/runtime failures with source locations, shrunk integer cases, source context and textual review | No recorded expression/branch trace, contract provenance graph, generated resource-cost proof, or full transitive assurance report. Test-case counts on failing runs are budgets, not measured completion counts. Distinguish traps, runtime faults, permissions, and resource limits. |
| Compilation | Rust frontend, C11 lowering and system native compiler; bounded independent reference evaluator for the supported test subset | Cranelift is absent. The daemon caches whole-source checked snapshots; it has no declaration-level dependency invalidation, function native-code cache, or incremental optimized-call invalidation. Extend reference/native differential coverage with every language feature. The reference evaluator does not execute host effects. |
| Resource bounds | Source/parser limits; bounded subprocess stderr and execution; Linux worker address-space limits; bounded cache estimate | Cache accounting is an estimate, not measured total daemon memory. Frontend CPU/memory, macOS worker memory, runtime allocation reporting, and total HTTP request deadlines need explicit enforcement/measurement. Test adversarial inputs at published limits. |
| Observability | Structured failures and basic service startup message | No bounded structured production events, build/source-linked telemetry, dropped-event counters, sampling semantics, secret-bearing types, or field allowlists. Add explicit value-capture approval and redaction tests before telemetry captures application data. |
| Projects and dependencies | Explicit manifest combines named source files in one namespace; Rust implementation dependencies are pinned | This is not a module system or Keel package manager. Namespaces/import resolution, Keel lockfiles/artifact hashes, signature packages, typed FFI, ABI stability, dependency supply-chain policy, and cross-compilation remain absent. Never download dependencies during ordinary compilation. |
| Adoption and compatibility | Example CLI/HTTP/collection programs; agent instructions and CLI/service interfaces | Versioned protocol compatibility, migration guidance, release packaging/signing, long-running service operations, supported platform/ABI matrix, and sustained external user testing remain release work. The HTTP adapter is a small example host, not a production HTTP stack. |
| Agent advantage | Reproducible local measurements can be collected by the benchmark harness | No synthetic or scripted fixture establishes a model's cost per accepted change. The four-condition, repeated-trial, independent-acceptance experiment must be run with real agents before claiming superiority. |

## A release gate for the supported subset

A limited supported release can explicitly defer large features, but it must
state its supported domain. It cannot be described as the fully implemented
original language while the table above contains these omissions.

1. Freeze and version the supported grammar, types, ownership/effect rules,
   integer traps, evaluation order, contract behavior, runtime ABI, and agent
   protocol. Publish deliberate restrictions and compatibility policy.
2. Pass unit and CLI integration tests on every claimed operating-system and
   architecture combination, including native builds, permission denials,
   independent acceptance tests, malformed input, stale edits, atomicity,
   source protection, symlink/hardlink aliases, and project file boundaries.
3. Run memory/undefined-behavior sanitizers and continuous parser/checker/native
   lowering fuzzing. Differentially compare valid generated programs with an
   independent reference evaluator. Publish corpus, duration, seeds, findings,
   fixes, and remaining defects; a finite passing corpus is not a soundness proof.
4. Obtain an independent ownership and capability-boundary security review.
   Define trusted components: compiler, system C compiler/linker, native runtime,
   host adapters, deployment policy owner, and acceptance-test owner. State the
   scope of filesystem/process isolation and concurrent-edit guarantees.
5. Test all resource limits adversarially. Demonstrate that malformed syntax,
   large contexts, compiler subprocesses, descendant processes, blocked input,
   allocation pressure, and test loops have documented bounds. Report platform
   limitations instead of presenting unenforced limits as active protection.
6. Validate release builds with deterministic artifact provenance and dependency
   inventory. Supply reproducible installation, rollback, build, test, and
   incident-reproduction instructions. Remove optional test/compiler facilities
   from application artifacts where claimed, and measure what is actually linked.
7. Publish measured benchmarks with hardware, OS/toolchain versions, optimization
   flags, project size, installed dependencies, warm/cold definitions, repetition
   count, raw samples, median/tail latency, peak RSS, and executable size. Do not
   treat a tiny example or a cache hit as a 10,000-line incremental-edit benchmark.

## Evidence for the efficiency objective

Use the same model versions, comparable instruction/tool budgets, repeated
trials, and independent acceptance criteria in four conditions:

- Existing language with conventional tools.
- Existing language with equally strong context, structured diagnostics, and testing.
- Keel with ordinary text editing.
- Keel with the complete implemented protocol.

Include repository-level changes, unfamiliar APIs, ownership-sensitive work,
stateful bugs, and performance-sensitive tasks. Keep failed attempts and tool
execution in the numerator of cost per accepted change; report acceptance rate
separately. Record inference cost, tool cost, time to useful diagnostic, repair
iterations, regressions, human intervention, runtime speed, peak memory, and
binary size. Use confidence intervals and disclose task/model selection.

The original 25% lower cost-per-accepted-change objective is a gate to measure,
not a test assertion to force true. A release may honestly report that it misses
the gate. Claims also require no decline in independently assessed acceptance.
Compiler microbenchmarks and deterministic agent-workflow fixtures are useful
regressions; neither substitutes for this experiment.

Original numerical engineering targets remain unestablished until measured on
the specified 10,000-line reference project: p95 warm edit feedback under 50 ms,
p95 small edit to affected native test under 500 ms, cold build under 2 seconds,
daemon under 256 MiB, and stripped minimal CLI under 1 MiB. Include generic
instantiations, actual linked libraries, invalidation work, and failed runs.
