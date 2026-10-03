# Architecture and next language stages

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

Canonical repository state is source. The parsed AST, call graph, and hole context are derived on each invocation. There is no semantic database to synchronize. The compiler is written in Rust; generated applications use a small C host runtime and platform libraries, with no Rust or compiler runtime dependency.

| File | Responsibility |
| --- | --- |
| `src/syntax.rs` | Lexer, parser, syntax tree, source offsets and diagnostics |
| `src/check.rs` | Signatures, primitive types, ownership flow, effects, contract checking, call graph, hole context |
| `src/native.rs` | Native lowering through C with explicit temporaries and destruction |
| `src/runtime.c` | Checked arithmetic, Text storage, diagnostics, permission gates, POSIX HTTP host |
| `src/main.rs` | CLI protocol, compiler invocation, bounded test workers, replay/shrinking, revision-bound edits, inspection/review |
| `src/tests.rs` | Compiler, native runtime, protocol, and HTTP regression tests |

The C backend is an intentional bootstrap tradeoff. It produces real native executables now and gives access to mature address/undefined-behavior sanitizers, but inherits external compiler latency and platform requirements. C integer undefined behavior is avoided through checked arithmetic helpers. Source expression order is materialized with temporaries rather than inherited from C argument evaluation. There is one backend, and both ordinary builds and tests use the same lowering and runtime semantics.

## Agent protocol

Commands use ordinary files and optional JSON reports. Each source snapshot gets a deterministic FNV-1a content revision. This is a stale-edit identity, **not a cryptographic digest** or authenticity check. Function handles are `fn:<name>` scoped to that revision. Expression handles and multi-edit transactions are deferred.

`inspect` retrieves a selected implementation and direct dependency interfaces, plus callers, effects, contracts, test names, and holes. Its source-character budget applies to implementation snippets; other metadata is not size-bounded. It marks truncation. The graph is syntactic, not a computed minimal task-relevant slice.

`edit` only replaces a braced body. It rejects attempts to inject additional declarations or modify signature clauses. The candidate is parsed and checked in memory and optionally tested before a sibling staging file is atomically renamed into place. It re-reads the original file before writing to detect concurrent changes during validation. There is no cross-editor locking or filesystem compare-and-swap, so a noncooperating editor racing in the narrow final read/rename interval is outside v0's concurrency guarantee. Single-writer use is required. Existing file permissions are preserved.

`check` distinguishes static success from incomplete holes and behavioral evidence. `test` provides deterministic seeds and replay inputs, first-failure source positions, shrinking, and explicit uncertainty. `explain` expands source context only. `review` compares function interfaces/bodies and effects, reports added/removed functions and test counts, and explicitly carries no fresh testing evidence. It does not analyze changed test bodies, transitive resource costs, or behavior equivalence.

The interface makes protected changes difficult to make accidentally. It cannot secure acceptance criteria against an agent with arbitrary filesystem access. Deployment needs a separate trusted acceptance-test store, launcher policy, and external evaluator.

## Memory and runtime boundary

Text is the first noncopyable type. It exercises movement, call-scoped borrowing, joins, cleanup on multiple returns, rebinding, loop ownership restoration, and explicit allocation. Composite ownership is intentionally not claimed on the strength of a Text-only prototype.

Host primitives have compiler-known types and effects. The HTTP adapter and libc are trusted native code. Permission checks constrain the provided primitives, not arbitrary native extensions; v0 exposes no FFI to Keel programs. HTTP request memory is borrowed only during the handler call. Returning that borrow is statically forbidden; response construction allocates owned output.

Processes isolate test failures. Time limits are enforced by the Rust parent; memory limits, syscall sandboxing, compiler timeouts, and bounded parser recursion are not implemented. Stack overflow and native crashes are reported as worker failures. The toolchain is for trusted development inputs, not a hostile multi-tenant compilation service.

There is no tracing recorder. Diagnostics record kind, source offset, and integer property input. Runtime source offsets require the matching revision; compiler-generated test reports supply it. Raw executable diagnostics alone do not contain an embedded build revision.

## Next stages and acceptance gates

1. **Semantic foundation:** introduce a typed intermediate representation and a reference evaluator; differential-test it against native execution. Write explicit preservation/ownership invariants and adversarial tests before adding composite values. Existing integer-reference and sanitizer tests are an initial check, not a soundness proof.
2. **Useful data model:** add nominal records, tagged unions, exhaustive matching, Option/Result, explicit propagation, and owned contiguous lists. Add field-sensitive moves and destructor lowering together. Expand property generation and shrinking around these types. Recoverable host failures should become Result values here.
3. **Restricted exclusive borrowing:** add `edit` for the dynamic call extent with non-aliasing checks. Keep borrowed storage out of returned values, containers, escaping closures, and suspension. Do not add lifetime parameters until evidence requires them.
4. **Native feedback speed:** separate frontend queries at declaration boundaries, cache interface/body hashes, bound cache memory, and add a Cranelift backend. First prove equivalent checked arithmetic, cleanup, and error behavior against the bootstrap backend. Retire the C backend or clearly select it for differential testing; avoid two user-visible semantics.
5. **Agent editing and policy:** add canonical formatting that preserves comments, stable revision-scoped expression handles, transactional multi-edits, explicit rebases, cross-process edit coordination, modules, source-linked acceptance stores, and measured affected-test selection.
6. **Capability values and simulation:** pass typed host capabilities as parameters, bind them to separately controlled deployment policy, and substitute fake clock/storage/randomness/network implementations. Add bounded state-machine generators and replayable traces before concurrency.
7. **Structured concurrency and resource reporting:** define cancellation and channel ownership with bounded queues, instrument allocation/resource summaries with proven/estimated/unknown categories, and make instrumentation optional and bounded. Shared immutable ownership must stay explicit.
8. **Packages and adoption:** explicit manifests/lockfiles and typed adapters, no implicit downloads or arbitrary build scripts in Keel package resolution. Rust's Cargo bootstrap dependencies are separate from this future application package system.

General theorem proving, specialized model training, unrestricted metaprogramming, and an optimization-focused second production backend remain deferred.

## Experiment needed before claiming advantage

Use the four original conditions: existing language with conventional tooling; that language with equally strong semantic retrieval/tests; Keel with text edits; Keel with the full protocol. Pin the model, prompts, task corpus, tools, acceptance evaluator, and budgets. Include failures in total inference plus execution cost, divided by accepted changes. Report success rate separately.

Start with concrete HTTP-router changes, exact-deadline cache bugs after a fake-clock host exists, unknown APIs, ownership transfers, and integer boundary fixes. Protect acceptance tests outside the agent's writable tree. Repeat trials and retain attempts, diagnostics, revisions, timing, and acceptance evidence.

The original 50 ms/500 ms/2 s compiler goals require a specified 10,000-line project and machine; no measurements on the tiny examples establish those targets. The proposed 25% economic improvement is a future go/no-go criterion, not a result of this prototype.
