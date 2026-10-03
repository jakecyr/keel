# Architecture and remaining language stages

Keel is experimental. This document describes the implemented toolchain, not a
completed implementation of the original design. See [release gaps](release-gaps.md)
for omissions and the evidence required for a supported production release.

## Implemented pipeline

```text
UTF-8 Keel source
  → lexer / recursive-descent parser
  → declaration signatures and typed expression checks
  → flow-sensitive ownership and declared-effect checks
  → explicit evaluation order / cleanup in generated C
  → system native compiler
  → executable or isolated test worker
```

Canonical repository state is source. A CLI invocation derives the parsed AST,
call graph, and hole context from its source snapshot. The persistent service can
reuse checked whole-source snapshots; there is no second editable semantic
database. The compiler is written in Rust. Generated applications use a C host
runtime and platform libraries, with no Rust or compiler runtime dependency.

An independent Rust reference evaluator executes checked tests without native
compilation. `keel test --engine both` compares native and reference outcomes on
the selected cases; it does not prove semantic equivalence for all programs.

| File | Responsibility |
| --- | --- |
| `src/syntax.rs` | Lexer, parser, syntax tree, source offsets and diagnostics |
| `src/check.rs` | Signatures, built-in types, ownership/borrow flow, exhaustive matches, effects, contracts, call graph, hole context |
| `src/native.rs` | Native lowering through C with explicit temporaries and destruction |
| `src/runtime.c` | Checked arithmetic, owned Text/List/Result storage, bounds checks, parsing, diagnostics, permission gates, POSIX HTTP host |
| `src/eval.rs` | Bounded reference evaluation, deterministic properties, shrinking, replay, differential regression tests |
| `src/main.rs` | CLI protocol, compiler invocation, bounded test workers, replay/shrinking, revision-bound edits, inspection/review |
| `src/project.rs`, `src/files.rs` | Explicit project composition, initialization, acceptance-file policy, bounded reads, path protection, atomic writes and edit locks |
| `src/process.rs` | Subprocess process groups, execution/address-space limits, bounded diagnostic capture |
| `src/agent.rs`, `src/service.rs` | Embedded versioned references, context/catalog commands, persistent JSON-lines service and snapshot cache |
| `src/format.rs`, `src/lint.rs` | Conservative formatting and unused-binding/effect warnings |
| `src/*tests.rs`, `tests/` | Compiler, sanitizer, audit, native HTTP, public CLI, and developer/agent workflow regressions |

The C backend is an intentional bootstrap tradeoff. It produces real native executables now and gives access to mature address/undefined-behavior sanitizers, but inherits external compiler latency and platform requirements. C integer undefined behavior is avoided through checked arithmetic helpers. Source expression order is materialized with temporaries rather than inherited from C argument evaluation. There is one backend, and both ordinary builds and tests use the same lowering and runtime semantics.

## Agent protocol

Commands use ordinary files and JSON reports. Each source snapshot gets a
deterministic FNV-1a content revision. This is a stale-edit identity, not a
cryptographic digest or authenticity check. Function handles are `fn:<name>`
scoped to that revision. Transactions support up to 128 distinct function-body
replacements in one physical source file. Expression handles, rebasing, and
multi-file write transactions remain deferred.

`keel init` creates or refreshes a project without replacing existing source or
tests. It preserves human guidance around managed blocks in both AGENTS.md and
CLAUDE.md. The installed binary supplies `agent commands`, `agent spec`, and
`agent context`, so agent guidance need not depend on a repository checkout or
online documentation. See the [agent protocol](agent-protocol.md) and
[supported-language guide](agent-language.md).

`inspect` retrieves a selected implementation and direct dependency interfaces, plus callers, effects, contracts, test names, and holes. Its source-character budget applies to implementation snippets; other metadata is not size-bounded. It marks truncation. The graph is syntactic, not a computed minimal task-relevant slice.

`edit` only replaces braced bodies. It rejects attempts to inject additional
declarations or modify signatures/contracts, and protects declarations in
manifest acceptance files, including helper functions. Project files must parse
independently so a declaration cannot straddle file boundaries. The candidate is
checked in memory and optionally tested before a sibling staging file is
atomically renamed into place. Optional edit validation conservatively runs all
native tests, not a computed affected-test subset. Cooperating edits use an
exclusive lock file and re-read the original source before writing. A
noncooperating editor racing in the final read/rename interval is outside this
guarantee; there is no filesystem compare-and-swap. Permissions are preserved.

`check` distinguishes static success from incomplete holes and behavioral
evidence. `test` provides deterministic seeds, replay inputs, source positions,
shrinking, and explicit uncertainty. Engines are `native` (default), `reference`,
and `both`; uncertain engine outcomes remain uncertain. `explain` expands source
context only. `review` compares function interfaces/bodies, effects, and textual
test changes, reporting added/removed tests and required acceptance review. It
does not infer behavioral equivalence, resource costs, or fresh test evidence.

`fmt` normalizes indentation and whitespace while preserving string bytes and
comments. It is not a full canonical syntax printer. `lint` reports unused
bindings and declared effects; it is advisory unless `--deny-warnings` is used.
`serve` provides check/inspect/test/lint/format requests and cache statistics over
JSON lines. Its least-recently-used cache is limited by estimated snapshot bytes,
not an enforced total process-memory budget. Text edits cause whole-source cache
misses; there is no declaration-level incremental invalidation yet.

The interface makes protected changes difficult to make accidentally. It cannot secure acceptance criteria against an agent with arbitrary filesystem access. Deployment needs a separate trusted acceptance-test store, launcher policy, and external evaluator.

## Memory and runtime boundary

Text, contiguous List<Int>, and Result<Int, Text> are noncopyable owned types.
Int, Bool, and Option<Int> copy by value. Read/edit loans span an entire call,
including later argument evaluation; `edit` requires exclusive mutable access.
For loops hold read loans on their input, and matches borrow owned scrutinees
while payloads are in scope. Cleanup covers normal block exit, replacement, and
function return. Traps terminate the process without unwinding. These concrete
container types do not establish support for arbitrary generic ownership,
records, user-defined unions, shared ownership, closures, or suspension.

Host primitives have compiler-known types and effects. The HTTP adapter and libc are trusted native code. Permission checks constrain the provided primitives, not arbitrary native extensions; v0 exposes no FFI to Keel programs. HTTP request memory is borrowed only during the handler call. Returning that borrow is statically forbidden; response construction allocates owned output.

Native test declarations run in separate subprocesses. Per-worker and suite
execution budgets, process groups, and bounded stderr capture prevent common
runaway-worker failures. Native compilation has a separate timeout. Linux workers
also enforce address-space limits; the same memory limit is not enforced on
macOS. The frontend caps source size and parser nesting but does not have an OS
CPU/memory sandbox. Stack overflow and native crashes are worker failures.

The reference evaluator has per-case step, recursion, and cumulative owned
payload-allocation limits in addition to execution deadlines. Its allocation
counter is not a complete heap limit. It does not execute host effects. Reference
and native budgets apply separately in `both` mode, and native compilation is
outside the suite's execution deadline. Exact defaults are in the
[language reference](language.md).

Bounded file reads reject nonregular inputs. Output protection checks source
aliases and symlink traversal; atomic staging cleanup does not remove files
created by another writer. These guardrails are not a hostile multi-tenant
compilation sandbox. The HTTP adapter still has per-socket timeouts rather than
a full production server lifecycle, total request deadline, or concurrency model.

There is no tracing recorder. Diagnostics record kind, source offset, and integer property input. Runtime source offsets require the matching revision; compiler-generated test reports supply it. Raw executable diagnostics alone do not contain an embedded build revision.

## Next stages and acceptance gates

1. Extend the independent evaluator/native differential corpus, fuzz the frontend
   and lowering, and write reviewed ownership/preservation invariants. Existing
   sanitizer and differential tests are evidence, not a soundness proof.
2. Add nominal records, user-defined tagged unions, general simple generics,
   propagation, field-sensitive moves, and generic collection generators/shrinkers.
   Keep borrowed storage nonescaping; retain explicit clones and mutable loans.
3. Add declaration-level dependency queries and native caches, measured memory
   accounting, and a Cranelift backend with checked arithmetic/cleanup parity.
4. Extend formatting, revision-scoped expression edits, rebasing, module support,
   external acceptance ownership, atomic multi-file edits, and affected-test
   selection. Existing manifest composition is a shared namespace, not modules.
5. Introduce typed capability values, deployment binding, controlled-world clock,
   storage/random/network substitution, state-machine generators, and fault replay.
6. Define structured concurrency, bounded channels, cancellation, explicit shared
   immutable ownership, and resource/telemetry reports that distinguish proven,
   estimated, observed, and unknown behavior.
7. Add Keel dependency lockfiles, artifact hashes, typed FFI, supported-platform
   distribution and version compatibility. Rust Cargo dependencies are separate
   from a future Keel application package system.

General theorem proving, specialized model training, unrestricted metaprogramming, and an optimization-focused second production backend remain deferred.

## Experiment needed before claiming advantage

Use the four original conditions: existing language with conventional tooling; that language with equally strong semantic retrieval/tests; Keel with text edits; Keel with the full protocol. Pin the model, prompts, task corpus, tools, acceptance evaluator, and budgets. Include failures in total inference plus execution cost, divided by accepted changes. Report success rate separately.

Start with concrete HTTP-router changes, exact-deadline cache bugs after a fake-clock host exists, unknown APIs, ownership transfers, and integer boundary fixes. Protect acceptance tests outside the agent's writable tree. Repeat trials and retain attempts, diagnostics, revisions, timing, and acceptance evidence.

The [recorded measurements](../benchmarks/results/README.md) cover a specified
synthetic 10,000-line fixture, with warm feedback, resident-process memory, and
minimal binary-size results. They do not establish representative application
performance, cold-cache build, or first-affected-test targets. See
[validation evidence](validation.md) for conditions and limits.

The real-agent pilot retained its preregistered budgets and failed attempts.
Although its repairs passed independent assertions, none met the experiment's
aggregate-token budget; monetary costs and enforced condition isolation were
unavailable. The proposed 25% cost-per-accepted-change improvement remains
UNKNOWN and is not achieved by these results.
