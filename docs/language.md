# Keel language, version 0

This document specifies the experimental implemented subset. It is not a claim
that the original language design is complete or production-certified. See the
[architecture](architecture.md), [release gaps](release-gaps.md), and compact
[agent language guide](agent-language.md). The installed compiler embeds that
guide: `keel agent spec language` works without repository paths or web access.

## Source and declarations

Source is UTF-8. Identifiers use ASCII letters, digits, and underscores, and cannot start with a digit. `//` comments run to the end of the line. String literals support UTF-8 and `\n`, `\r`, `\t`, `\"`, and `\\` escapes. There is no interpolation.

A file contains complete function, example-test, and property declarations.
`keel.json` can explicitly compose entry, source, and acceptance-test files into
one shared namespace; each file must parse independently. Functions may refer to
declarations later in the file or in another listed file. Duplicate function or
test names are errors. `pub` marks an intended public interface in inspection;
there is no module visibility enforcement or automatic import/package discovery.

Source is limited to 4 MiB per compilation snapshot. Parsing rejects nesting over
64 parser levels and more than 64 consecutive infix operators at one precedence
level with `resource_limit`. Split complex expressions into named locals.

```text
pub fn double(value: Int) -> Int
    requires value >= 0 && value <= 1000
    ensures result == value * 2
{
    return value * 2
}
```

Every parameter has an explicit type. A missing return annotation means `Unit`. Executables require `fn main()` with no parameters and a `Unit` result. A non-Unit function must return on every statically recognized path. Loops are not assumed to terminate or to guarantee a return. Unreachable statements after a guaranteed return are rejected.

Braces delimit blocks. Semicolons are optional except when a bare `return` needs disambiguation before another token; prefer `return;` for Unit. Newlines are whitespace, not statement terminators. `else` requires braces; use a nested `if` for an else-if chain. Shadowing is rejected in v0 to keep bindings unambiguous.

## Types and expressions

| Type | Meaning |
| --- | --- |
| `Int` | Signed 64-bit integer, including −9,223,372,036,854,775,808 through 9,223,372,036,854,775,807 |
| `Bool` | `true` or `false`; integers do not implicitly convert to Boolean |
| `Text` | Immutable byte-length-tracked UTF-8 text, with static literal or owned heap storage |
| `List<Int>` | Owned contiguous mutable integer buffer; literal syntax `[1, 2]` or `[]` |
| `Option<Int>` | Copyable tagged value: `Some(Int)` or `None` |
| `Result<Int, Text>` | Owned tagged value: `Ok(Int)` or `Err(Text)` |
| `Unit` | No result; cannot be stored in locals or parameters |

These are specific built-in types, not arbitrary generic instantiations. There
are no user-defined records/unions, null, closures, or implicit type conversions.

`let` creates an immutable binding. `var` permits rebinding and explicit exclusive
borrowing through `edit`. Shadowing is rejected. `result` is reserved for
postconditions and cannot name a local or parameter, even without contracts;
use a name such as `output` instead. Optional local type annotations constrain inference:

```text
let limit: Int = 100
var count = 0
while count < limit {
    count = count + 1
}
```

Arithmetic operators are `+ - * / %` and unary `-`. Overflow always terminates execution with an `overflow` diagnostic, including negation of minimum Int, minimum Int divided by −1, and minimum Int remainder −1. Division truncates toward zero. Remainder has the sign of the dividend. Division or remainder by zero produces `division_by_zero`. Wrapping and checked-result arithmetic APIs are future additions.

`==` and `!=` compare values of the same non-Unit type. Text equality compares
bytes, list equality compares elements in order, and Option/Result equality
compares tags and active payloads, not allocation identity. Ordering operators
`< <= > >=` accept Int. `!`, `&&`, and `||` accept Bool; the latter two
short-circuit. Expressions and call arguments evaluate from left to right.
Precedence, highest first: unary; multiplication/division/remainder;
addition/subtraction; ordering; equality; `&&`; `||`.

No implicit conversions or Text concatenation operators exist. Use named library functions.

## Ownership

Int, Bool, and Option<Int> copy by value and use ordinary value parameters. Text,
List<Int>, and Result<Int, Text> parameters must declare `read`, `edit`, or `take`:

```text
fn length(value: read Text) -> Int { return text.len(value) }
fn consume(value: take Text) -> Text { return value }
fn append(values: edit List<Int>, value: Int) { list.push(edit values, value) }
```

`read` grants a temporary immutable borrow for the call. It cannot be returned
or turned into an owning local. `edit` grants exclusive access to a mutable
binding, with an explicit `edit variable` call argument. An edit parameter can
be replaced with newly owned storage but cannot be moved out or returned as
owned. `take` transfers an owned value to the callee; an existing variable must
be explicitly moved at the call site:

```text
let original = text.clone("hello")
let copied = text.clone(original)
let output = consume(take copied)
assert original == output
// Reading copied here is a static use-after-move error.
```

Constructed temporaries and literals can be passed directly to consuming
parameters. Assigning an owned variable to another binding requires `take` or an
explicit clone. Returning an owned local or consuming parameter transfers it
automatically. Borrowed text and lists must be cloned to return owned values.
There is currently no generic cloning operation for Result. Text literal storage
is static and never freed; transfers still obey the static ownership rules.

Read/edit loans last through later argument evaluation and the call. A live read
loan prevents moving or mutably borrowing the same value; a live edit loan
prevents all other access to it. The left owned operand of an equality
comparison stays read-borrowed while its right operand is evaluated.
For example, `list.push(edit values, list.len(values))` is rejected; compute
the length in a local before borrowing `values` exclusively.

A conditional leaves an outer variable moved if any continuing branch moved it. A returning branch does not restrict the other continuing branch. A loop cannot leave an initially available outer value moved at the next iteration: restore ownership on every path or choose another representation. Analysis is deliberately conservative, with no path-sensitive theorem solving.

Owned locals are destroyed at their lexical block boundary, on rebinding, or at
function exit. Consuming parameters are destroyed on function exit unless
transferred. Expression temporaries are destroyed after their enclosing
statement/condition, except loop inputs and match scrutinees, which remain live
through their bodies. Traps terminate the process without unwinding cleanup.
Native execution performs no hidden deep copies. Text uses pointer/length/owned
metadata, lists own pointer/length/capacity storage, and Results own their error
payload. Compiler-generated shallow read copies are borrows. The reference
evaluator may copy values internally and is not a runtime performance model.

Escaping references, shared ownership, closures, arbitrary composite types, and
suspension remain unsupported. There is no general-purpose collector or
reference counting in generated applications.

## Collections, matching, and recoverable errors

`for value in values { ... }` read-borrows a List<Int> and copies each element into
an immutable local. Its body cannot replace, move, or edit the iterated list.
`list.get` returns Option<Int>; `list.at` and `list.set` trap on negative or
out-of-range indices with source-linked bounds diagnostics.

```text
fn parsed_or_zero(raw: read Text) -> Int {
    match text.parse_int(raw) {
        Ok(value) => { return value }
        Err(message) => { assert text.len(message) > 0 return 0 }
    }
}

test "optional indexing" {
    match list.get([17], 0) {
        Some(value) => { assert value == 17 }
        None => { assert false }
    }
}
```

Matches must contain exactly both variants of their scrutinee type. Missing,
duplicate, foreign, or malformed arms are compile errors. Payload bindings are
local to their arm; Err text is read-borrowed and must be cloned to escape.
Owned scrutinees remain borrowed throughout matching. There are no wildcard
patterns, guards, user-defined variants, match expressions, or propagation syntax.

`text.parse_int` accepts an optional minus followed by ASCII decimal digits,
including leading zeros and the full signed 64-bit range. Empty text, plus signs,
whitespace, nondecimal notation, invalid characters, and overflow return Err
instead of trapping. See [collection semantics and examples](features.md) for
the full specialized API and stable-deduplication example.

## Effects and authority

Functions have no external effects unless they declare them:

```text
fn main() effects { io.stdout } {
    io.println("hello")
}
```

Only `io.stdout` and `net.listen` exist. Callers must permit every effect declared
by their callees. Declarations are conservative upper bounds; unnecessary effects
are allowed and may receive a lint warning. Local allocation and mutation do not
count as external effects. Contracts cannot call effectful, consuming, or editing
functions. Tests have no external effects; network integration tests belong to
the host regression suite.

Effect declarations grant no runtime authority. The executable receives
`--allow-stdout` and/or `--allow-net=127.0.0.1:PORT` from its launcher. `keel run`
can translate a separately supplied `--policy keel.policy.json` into these grants;
policy and direct permission overrides cannot be combined. Generated policies
deny authority by default. The network host checks the exact port before opening
a socket. This is a local policy binding, not an OS sandbox or independently
administered deployment system. A party controlling launch arguments or the
policy file can grant permissions. Typed capability values remain future work.

## Built-in library

| Function | Signature / behavior |
| --- | --- |
| `text.clone` | `(read Text) -> Text`; allocates an explicit byte copy |
| `text.concat` | `(read Text, read Text) -> Text`; allocates the concatenation |
| `text.len` | `(read Text) -> Int`; **byte** length, not character count |
| `text.from_int` | `(Int) -> Text`; allocates decimal representation |
| `text.parse_int` | `(read Text) -> Result<Int, Text>`; recoverable strict decimal parsing |
| `list.new` / `list.clone` | `() -> List<Int>` / `(read List<Int>) -> List<Int>`; empty buffer / explicit owned copy |
| `list.len` / `list.contains` | `(read List<Int>) -> Int` / `(read List<Int>, Int) -> Bool`; length / linear membership |
| `list.at` / `list.get` | `(read List<Int>, Int) -> Int` / `Option<Int>`; trapping / recoverable indexing |
| `list.push` / `list.set` | `(edit List<Int>, Int) -> Unit` / `(edit List<Int>, Int, Int) -> Unit`; append / checked replacement |
| `option.some` / `option.none` | `(Int) -> Option<Int>` / `() -> Option<Int>` |
| `result.ok` / `result.err` | `(Int) -> Result<Int, Text>` / `(take Text) -> Result<Int, Text>` |
| `io.println` | `(read Text) -> Unit`; requires `io.stdout` and runtime permission |
| `http.response` | `(Int, read Text) -> Text`; allocates a complete HTTP/1.1 text/plain response; status 100–599 |
| `http.status` | `(read Text) -> Int`; extracts the status from the supported response form, or 0 |
| `http.body` | `(read Text) -> Text`; copies bytes after the first header separator, or empty Text |
| `http.serve` | `(Int, named_handler) -> Unit`; special intrinsic requiring `net.listen` |

The HTTP handler must be a named pure function `(read Text) -> Text`. It receives
the request path with the query removed, without URL decoding, and returns the
complete response. Function values are otherwise unsupported. Request/response
helpers are demonstration primitives, not general HTTP validation libraries.
Invalid status codes or ports still trap; those host APIs do not yet return typed
errors. Retrieve ordinary built-in signatures with `keel api NAME --json`;
`http.serve` is a special compiler intrinsic.

## Contracts, holes, and tests

`requires` runs on entry. `ensures` runs before function cleanup and return, with
an immutable `result` binding. Both must be Boolean and externally pure. They are
executable checks, never proofs or optimizer assumptions. A postcondition cannot
read an owned parameter that the implementation moved. Validate external input
with typed errors where supported; preconditions must not hide malformed inputs
from the test domain. The current Result specialization does not cover arbitrary
domain error types.

`hole("identifier")` adopts an expected type. Return position, annotated binding, or a typed call argument can supply it. Inspection reports the expected type, available bindings, allowed effects, and source position. A reached hole aborts its test worker with `BLOCKED`; it does not fabricate a value. Static checking reports `INCOMPLETE`, and executable builds reject all holes, including those in tests. A filtered unaffected test can pass while the report still lists remaining holes.

```text
property "double is even" (n in gen.int(min: -1000, max: 1000)) {
    assert (n * 2) % 2 == 0
}
```

v0 generators have one Int variable and inclusive constant bounds. The runner visits bounds and valid 0/−1/1 cases first, then uses seeded xorshift64 samples. Seed 0 is normalized to 1. This is deterministic testing randomness, not cryptographic randomness. No precondition-based filtering is performed: a violated precondition is a failure.

The default native engine runs each test declaration in its own subprocess;
all samples of a property share that process. Defaults are 100 samples, seed 1,
2,000 ms per test declaration, and a 30,000 ms suite execution budget.
`--cases`, `--seed`, `--timeout-ms`, and `--budget-ms` configure those values.
Native compilation has a separate 30,000 ms default timeout, configurable through
`KEEL_BUILD_TIMEOUT_MS`, and is outside the suite execution budget.

Native workers default to a 256 MiB address-space limit on Linux. `--memory-mib 0`
disables it; macOS does not enforce this address-space budget. Process groups,
bounded stderr capture, and CPU/wall deadlines constrain worker execution but do
not provide a syscall sandbox.

`--engine reference` runs the independent evaluator without a native compiler or
host effects. It enforces the time budgets plus per-case limits of 1,000,000
steps, 64 nested function calls, 128 nested expression evaluations, and 32 MiB of
cumulative owned payload allocation. That counter is not a total heap limit and
does not model native allocation cost. `--engine both` runs each engine with its
own budgets and compares test status and failure kind/location/input. Mismatches
fail; inconclusive runs remain UNKNOWN. Agreement is sampled evidence, not proof.

A failed property may receive up to 32 shrinking attempts, each bounded by its
worker timeout and remaining suite budget. Shrinking stays within the generator
domain and requires the same failure kind and source offset. It seeks a smaller
absolute integer without claiming a globally minimal counterexample. Replay
requires selecting exactly one property and an in-range `--value`; an exact
`--filter` name takes priority over substring matching. List/record generators,
precondition filtering, and stateful simulation are not implemented.

Statuses are `TESTED`, `FAILED`, `BLOCKED`, and `UNKNOWN`. Timeouts, resource limits,
and empty selections are UNKNOWN; reached holes are BLOCKED. `cases` is the
completed count on success and generally the requested budget on failure,
marked by `case_count_is_budget`; native tests skipped after suite exhaustion
report zero. Exit status is nonzero for failures, blocking, uncertainty, and
incomplete static checks. Tests cannot return early. Assertions and contracts
run in optimized builds as well.

For project setup, revision-bound transactions, managed agent instructions,
formatting/linting, and the persistent service, see the
[agent protocol](agent-protocol.md) and [developer workflow](getting-started-review.md).
