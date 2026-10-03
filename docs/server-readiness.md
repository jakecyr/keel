# Server use cases, design review, and release evidence

This review covers Keel 0.1.0 at `73f7ba0` plus the server/data changes accompanying
this document. Keel remains experimental. This is a repository review and local
validation, not an independent security audit or production certification.

## Language and design overview

The Rust compiler parses readable functions and control flow, checks explicit
signatures, ownership, declared effects, and exhaustive built-in matches, then
emits C with checked integer arithmetic and defined left-to-right evaluation.
Owned Text/List/Result values have normal-path destruction. Runtime traps exit
without unwinding. The independent Rust evaluator supports pure tests; host I/O
needs native fixtures. Contracts establish ENFORCED when executed; tests establish
TESTED for recorded cases. Neither establishes PROVEN.

Current data types are Int, Bool, Text, Unit, List<Int>, Option<Int>,
Result<Int, Text>, and Result<Text, Text>. Functions provide explicit callable
interfaces; `pub` communicates intended visibility in inspection, not a module
boundary. JSON is validated UTF-8 text with bounded parsing, preserving decimal
spelling. The runtime also offers bounded files, processes, clients, configuration,
and a sequential localhost application server. Native adapters and system
libraries remain trusted code. Exact launcher grants are not an OS sandbox.

Read [LANGUAGE.md](../LANGUAGE.md) for runnable syntax, [architecture](architecture.md)
for implementation boundaries, and [release gaps](release-gaps.md) for release
criteria. `docs/design.md` describes visual identity, not language semantics.
The machine-readable [design status](design-status.json) remains authoritative
about unimplemented original-design features.

## Common server workloads

| Workload | Data and transformations | Available approach | Remaining production needs |
| --- | --- | --- | --- |
| REST/catalog API | Records, optional fields, arrays, exact money, validation, projection, updates, pagination | JSON pointers, checked integer cents, bounded array counting/replacement, query/header lookup; `examples/catalog_api` | Nominal records/unions, generic collections, schema decoding, richer errors, database transactions, pagination/storage indexes |
| Authentication/webhooks | Header credentials, raw signed bodies, timestamps, replay keys, secret redaction | Request header/body access and explicit host effects | Reviewed cryptography, secret-bearing types, constant-time verification, randomness, durable replay storage, rate limiting; string equality is not an authentication design |
| Background workers | Typed jobs, process status, retries, deadlines, idempotency keys | Bounded process calls and owned child handles; offline game fixtures | Durable queues, cancellation across nested groups, controlled clocks, backpressure, structured supervision |
| Realtime services | Binary/text frames, sessions, incremental events, fanout | Buffered SSE and one-shot client adapters only | Persistent connections, streaming server APIs, binary buffers, general concurrency and bounded channels |
| ETL/integration | JSON/CSV/XML fields, arrays, exact decimal values, validation, aggregation | Bounded parsing, integer arithmetic, explicit JSON replacement | Decimal arithmetic, generic text/record collections, retained parse trees, streaming parsers |
| Static/browser tools | Public binary assets, routing, JSON requests, same-origin controls | `http.serve_app` and `http.serve_api`, root-relative static serving | TLS/service termination architecture, graceful shutdown, handler deadlines, production concurrency, structured telemetry |

The table describes capabilities needed for these workloads, not promises that
all production gaps have been closed by this change.

## Types, interfaces, inheritance, and agent ergonomics

Server domains benefit from nominal records (Product, User, Job), tagged unions
for state transitions, and generic containers/results. Those are the next language
features to design, before an inheritance system. They need specified C layouts,
field-sensitive moves, branch-join and cleanup rules, independent evaluator values,
negative tests, and sanitizer coverage. A schema must distinguish absent fields,
JSON null, and empty values; implicit coercion would hide validation errors.

No class inheritance or user-defined interface syntax is implemented here. The
existing design favors explicit ownership and small callable interfaces. Composition
of records and functions is the proposed direction for data models; nominal records
are still unimplemented. If generic interface constraints are introduced later,
start with explicit statically resolved signatures and an unambiguous implementation
selection rule. Dynamic dispatch, subtyping, implicit conversions, and reflection
would each need their own use case and ownership/ABI design. They are not prerequisites
for every server and should not be added merely to resemble another language.

The additions here use existing Text and Result types and preserve old handlers:
`http.serve_api`, `http.path`, `http.query`, `http.header`, `json.array_len`, and
`json.set`. They remove hand-written protocol parsing from small applications while
remaining discoverable through `keel api NAME --json`. JSON text remains a workaround,
not a replacement for domain typing. Repeated array updates still parse/copy the
document; keep batches bounded until retained values and typed records exist.
No token-cost improvement is claimed without agent evaluation evidence.

## Security review and fixes

| Boundary reviewed | Finding/action | Regression evidence and remaining limit |
| --- | --- | --- |
| Legacy HTTP reception/transmission | Per-read/write timeouts could be extended indefinitely by slow peers. Use nonblocking sockets and total monotonic deadlines for both directions. | Slow-trickle real-socket tests require closure within the total deadline and a subsequent healthy response. Handlers themselves remain unbounded. |
| Request Text construction | Legacy paths accepted invalid UTF-8/control bytes. Metadata-capable hosts must also validate query/header bytes before borrowing them as Text. | Native sanitizer tests reject invalid paths, header bytes, query bytes, controls and fragments before routing. Only a deliberately restricted HTTP subset is supported. |
| Header/query ambiguity | Applications previously lacked a safe extraction API. | Pure native/reference tests validate entire inputs, reject malformed trailing fields and duplicate matches, and distinguish empty from missing values. No implicit first/last selection for credential-like headers. |
| Request framing and browser origin | Existing app host rejects duplicate Content-Length, Transfer-Encoding, invalid Host, cross-origin Origin and Fetch Metadata. | Existing and extended socket fixtures cover these paths, body limits, fragmentation and HEAD. Host policy is not authentication against local processes; legacy host remains a pure GET example adapter. |
| Static files | Root-relative openat with no-follow and nonregular/dot/encoded-separator rejection. | Existing binary asset/traversal/symlink sanitizer tests retained. Root parent directories and local filesystem writers remain trusted. |
| JSON transformation | Inserting unchecked raw strings can corrupt a document; insertion can exceed size/depth even when individual inputs fit. | Validate original/replacement/result, cap output at 1 MiB and 64 levels, return owned output, and preserve original bytes outside the selected value. Missing paths are errors. Use json.quote for text. |
| Process/file/capability boundary | Exact grants authorize host calls, not transitive behavior or filesystem/DNS isolation. | Existing denial, child cleanup and file tests retained. Detached descendants, parent symlinks, inherited environment, DNS/IP binding and hostile local writers remain limits. |

Framing review used [RFC 9112](https://www.rfc-editor.org/rfc/rfc9112.html), and
pointer selection uses [RFC 6901](https://www.rfc-editor.org/rfc/rfc6901.html).
Keel deliberately chooses last-member selection for duplicate JSON object keys;
that extension is documented and is not an interoperable duplicate-key guarantee.
JSON replacement is an existing-location operation, not RFC 6902 JSON Patch.

The fixed host findings are tracked in [issue #4](https://github.com/jakecyr/keel/issues/4).
Unimplemented data-model and application capabilities continue in
[issue #1](https://github.com/jakecyr/keel/issues/1). Unknown security properties remain
unknown; this finite review does not establish absence of vulnerabilities.

## Performance evidence

Native JSON key decoding previously allocated the remaining document length for
every key. It now allocates only the encoded string extent. Internal selection
borrows a validated source span so integer/string decoding and replacement avoid
an unnecessary selected-fragment copy. Public returned values still own their data.

Run `python3 benchmarks/json_lookup.py --baseline 73f7ba0 --output NEW.json`.
The checked-in [raw results](../benchmarks/results/json-lookup-2026-10-03.json)
record OS/compiler, source hashes, flags, command outcomes, seeded order, and all
seven samples per version. For a 2,000-field object, 100 lookups per sample:

| Metric | Baseline | Updated |
| --- | --- | --- |
| Requested k_alloc bytes per lookup, including terminators | 29,005,803 | 18,903 |
| Allocation calls per lookup | 2,002 | 2,002 |
| Median elapsed seconds per 100 lookups | 0.016613 | 0.014311 |

Requested allocation bytes are cumulative requests, not live memory or peak RSS.
Timing includes parsing, lookup, allocation and result assertions, but excludes
process startup; OS caches are uncontrolled. This small macOS ARM64 fixture is
not a production throughput benchmark or an agent-cost study. Repeated document
parsing, generic collection design, and service concurrency remain larger work.

## Validation and release decision

The catalog example contains independent expected outputs, invalid-input tests,
and a percentage property. Rust tests execute pure helpers in both engines, run
the real API on loopback under address/undefined-behavior sanitizers, and check
checker/effect/borrow rejection. Public CLI tests check, test and build all application
examples offline, and build worker manifests; intentionally failing `holes` and
`counterexample` fixtures retain BLOCKED/FAILED outcomes. Existing Python game
fixtures exercise offline process orchestration without paid inference.

Keep the experimental label in README, package metadata, and the GitHub site.
The release criteria still require independent ownership/capability review,
sustained fuzzing, supported-platform evidence, operational bounds, compatibility
policy and provenance. Passing local tests and adding useful server APIs does not
satisfy those gates. See [validation record](server-validation.md) for commands,
results, and platform limits of this review.
