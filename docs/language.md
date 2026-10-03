# Keel language, version 0

This document specifies the implemented subset. Constructs listed as future work in the architecture document are not accepted language features.

## Source and declarations

Source is UTF-8. Identifiers use ASCII letters, digits, and underscores, and cannot start with a digit. `//` comments run to the end of the line. String literals support UTF-8 and `\n`, `\r`, `\t`, `\"`, and `\\` escapes. There is no interpolation.

A file is a single compilation unit containing function, example-test, and property declarations. Declarations may refer to functions appearing later in the file. Duplicate function or test names are errors. `pub` marks an externally intended interface in inspection; module visibility is deferred until modules exist.

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
| `Unit` | No result; cannot be stored in locals or parameters |

`let` creates an immutable binding. `var` permits rebinding; it does not introduce mutable references. Optional local type annotations constrain inference:

```text
let limit: Int = 100
var count = 0
while count < limit {
    count = count + 1
}
```

Arithmetic operators are `+ - * / %` and unary `-`. Overflow always terminates execution with an `overflow` diagnostic, including negation of minimum Int, minimum Int divided by −1, and minimum Int remainder −1. Division truncates toward zero. Remainder has the sign of the dividend. Division or remainder by zero produces `division_by_zero`. Wrapping and checked-result arithmetic APIs are future additions.

`==` and `!=` compare Int, Bool, or Text values of the same type. Text comparison checks bytes and length, not allocation identity. Ordering operators `< <= > >=` accept Int. `!`, `&&`, and `||` accept Bool; the latter two short-circuit. Expressions and call arguments evaluate from left to right. Precedence, highest first: unary; multiplication/division/remainder; addition/subtraction; ordering; equality; `&&`; `||`.

No implicit conversions or Text concatenation operators exist. Use named library functions.

## Ownership

Int and Bool copy by value. Text parameters must declare `read` or `take`:

```text
fn length(value: read Text) -> Int { return text.len(value) }
fn consume(value: take Text) -> Text { return value }
```

`read` grants a temporary immutable borrow for the call. It cannot be returned or turned into an owning local. `take` transfers an owned value to the callee; an existing variable must be explicitly moved at the call site:

```text
let original = text.clone("hello")
let copied = text.clone(original)
let output = consume(take copied)
assert original == output
// Reading copied here is a static use-after-move error.
```

Constructed temporaries and literals can be passed directly to consuming parameters. Assigning a Text variable to another binding requires `take` or an explicit clone. Returning an owned local or consuming parameter transfers it automatically. Returning a borrowed parameter requires an explicit clone. Literal storage is static and never freed; ownership transfer of a literal still obeys the static rules.

Read arguments remain borrowed throughout evaluation of the other arguments and the call. Moving the same value in a later argument is rejected. The same constraint applies to the left Text operand of an equality comparison while evaluating its right operand.

A conditional leaves an outer variable moved if any continuing branch moved it. A returning branch does not restrict the other continuing branch. A loop cannot leave an initially available outer value moved at the next iteration: restore ownership on every path or choose another representation. Analysis is deliberately conservative, with no path-sensitive theorem solving.

Owned locals are destroyed at their lexical block boundary, on rebinding, or at function exit. Consuming parameters are destroyed on function exit unless transferred. Expression temporaries are destroyed after the containing statement or condition. Traps terminate the process; cleanup on trap unwinding is not supported. These rules produce no hidden deep copies. Runtime representation uses a pointer, length, and ownership bit; compiler-generated shallow read copies are borrows.

Exclusive `edit` borrows, escaping references, shared ownership, closures, composite types, and suspension are outside v0. There is no general-purpose collector or reference counting.

## Effects and authority

Functions have no external effects unless they declare them:

```text
fn main() effects { io.stdout } {
    io.println("hello")
}
```

Only `io.stdout` and `net.listen` exist. Callers must permit every effect declared by their callees. Declarations are conservative upper bounds; unnecessary effects are allowed. Local allocation and mutation do not count as external effects. Contracts cannot call effectful or consuming functions. Tests have no external effects; network integration tests belong to the host regression suite.

Effect declarations grant no runtime authority. The executable receives `--allow-stdout` and/or `--allow-net=127.0.0.1:PORT` from its launcher. The network host checks the exact port before opening a socket. v0 permission flags are a demonstration policy binding, not an OS sandbox or a separately administered deployment-policy system. A person or agent who controls launch arguments can grant permissions.

## Built-in library

| Function | Signature / behavior |
| --- | --- |
| `text.clone` | `(read Text) -> Text`; allocates an explicit byte copy |
| `text.concat` | `(read Text, read Text) -> Text`; allocates the concatenation |
| `text.len` | `(read Text) -> Int`; **byte** length, not character count |
| `text.from_int` | `(Int) -> Text`; allocates decimal representation |
| `io.println` | `(read Text) -> Unit`; requires `io.stdout` and runtime permission |
| `http.response` | `(Int, read Text) -> Text`; allocates a complete HTTP/1.1 text/plain response; status 100–599 |
| `http.status` | `(read Text) -> Int`; extracts the status from the supported response form, or 0 |
| `http.body` | `(read Text) -> Text`; copies bytes after the first header separator, or empty Text |
| `http.serve` | `(Int, named_handler) -> Unit`; special intrinsic requiring `net.listen` |

The HTTP handler must be a named pure function `(read Text) -> Text`. It receives the request path with the query removed, without URL decoding, and returns the complete response. Function values are otherwise unsupported. Request and response helpers are demonstration primitives, not general HTTP validation libraries. Invalid status codes or ports trap until typed recoverable errors are introduced.

## Contracts, holes, and tests

`requires` runs on entry. `ensures` runs before cleanup and return, with an immutable `result` binding. Both must be Boolean and externally pure. They are executable checks, never proofs or optimizer assumptions. A postcondition cannot read an owned parameter that the implementation moved. Input validation for real services should eventually use typed errors; this prototype has no Result type yet.

`hole("identifier")` adopts an expected type. Return position, annotated binding, or a typed call argument can supply it. Inspection reports the expected type, available bindings, allowed effects, and source position. A reached hole aborts its test worker with `BLOCKED`; it does not fabricate a value. Static checking reports `INCOMPLETE`, and executable builds reject all holes, including those in tests. A filtered unaffected test can pass while the report still lists remaining holes.

```text
property "double is even" (n in gen.int(min: -1000, max: 1000)) {
    assert (n * 2) % 2 == 0
}
```

v0 generators have one Int variable and inclusive constant bounds. The runner visits bounds and valid 0/−1/1 cases first, then uses seeded xorshift64 samples. Seed 0 is normalized to 1. This is deterministic testing randomness, not cryptographic randomness. No precondition-based filtering is performed: a violated precondition is a failure.

Each example or property runs in its own native subprocess; all samples of a property share that subprocess. Timeout is per test declaration. A default budget of 100 samples, seed 1, and 2,000 ms can be changed. A failure may receive up to 32 additional shrinking attempts, each with the worker timeout. Shrinking keeps the input within its domain and requires the same failure kind and source offset. It aims for a smaller absolute integer, without claiming a globally minimal counterexample. A replay input must be within the declared generator range.

Statuses are `TESTED`, `FAILED`, `BLOCKED`, and `UNKNOWN`. Timeouts and empty selections are UNKNOWN. `cases` is the completed count on success and the requested budget on failure, marked by `case_count_is_budget`. Exit status is nonzero for failures, blocking, uncertainty, and incomplete static checks. Tests cannot return early. Assertions and contracts run in optimized builds as well.
