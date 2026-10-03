# Collections, optional values, and recoverable parsing

The native compiler supports `List<Int>`, `Option<Int>`, `Result<Int, Text>`, and `Result<Text, Text>`.
These are concrete built-in types. They do not imply support for arbitrary
generic arguments, user-defined records, or user-defined tagged unions.

## Owned contiguous integer lists

`[4, 2, 4]`, `[]`, and `list.new()` create owned contiguous buffers. List literals
evaluate their elements left to right. Buffers grow geometrically; `list.clone`
is the only implicit-size deep copy operation, and must be requested explicitly.
`==` and `!=` compare elements in order. Lists are destroyed on normal scope exit,
function return, or replacement. A runtime trap terminates the process.

```text
fn append(values: edit List<Int>, value: Int) {
    list.push(edit values, value)
}

fn length(values: read List<Int>) -> Int { return list.len(values) }
fn identity(values: take List<Int>) -> List<Int> { return values }

fn main() {
    var values = [1, 2]
    append(edit values, 3)
    let owned = identity(take values)
    assert length(owned) == 3
}
```

Owned parameters require `read`, `edit`, or `take`. Mutable borrowing requires an
explicit `edit variable` call argument and a mutable binding. Borrows last for
the entire call, including later argument evaluation. Thus
`list.push(edit values, list.len(values))` is rejected: calculate the length into
a local first. `read` and `edit` parameters cannot be moved out or returned as
owned values. An `edit` parameter may be replaced with a newly owned value.
Borrowed values cannot be stored, and the language still has no escaping closures.

`for value in values { ... }` borrows the list throughout iteration and copies
each integer into an immutable loop binding. The body cannot replace, move, or
mutably borrow the iterated list. Iterating a newly created list keeps it alive
until the loop exits. Consuming unrelated outer values in repeatable loop bodies
requires restoring them on every iteration.

| Operation | Behavior |
| --- | --- |
| `list.new()` | Empty `List<Int>` |
| `list.clone(values)` | Independent owned copy |
| `list.len(values)` | Length as `Int` |
| `list.contains(values, value)` | Linear membership search |
| `list.at(values, index)` | Read; negative/out-of-range index traps |
| `list.get(values, index)` | Read; out-of-range index returns `None` |
| `list.push(edit values, value)` | Append |
| `list.set(edit values, index, value)` | Replace; out-of-range index traps |

The [collection example](../examples/collections.keel) implements stable
deduplication with independent executable contracts for distinctness, coverage,
and first-occurrence order. It uses linear membership search and is quadratic
in the worst case. There is no hash set implementation yet.

## Exhaustive matching

`Option<Int>` is copyable. `Result<Int, Text>` owns its possible error text and
uses the same explicit ownership rules as lists and text. Constructors are
`option.some(n)`, `option.none()`, `result.ok(n)`, and `result.err(text)`.
An existing error text must be explicitly moved or cloned into `result.err`.

```text
fn parsed_or_zero(raw: read Text) -> Int {
    match text.parse_int(raw) {
        Ok(value) => { return value }
        Err(message) => { assert text.len(message) > 0 return 0 }
    }
}
```

Every match must handle exactly both variants (`Some(value)` / `None`, or
`Ok(value)` / `Err(message)`). Duplicate, missing, mistyped, and malformed arms
are compile errors. Payload bindings are local to their arm. Integer payloads
copy; error text is a read borrow and must be cloned to escape the arm. The
matched owned value remains read-borrowed through all arms.

`text.parse_int` accepts an optional minus followed by ASCII decimal digits,
including leading zeros and the entire signed 64-bit range. It rejects empty
strings, plus signs, whitespace, decimal/exponent notation, non-ASCII digits,
and overflow with a typed error. It never traps on malformed input.

## Guardrails and current limits

Parsing rejects sources over 4 MiB, nesting over 64 parser levels, and more than
64 consecutive infix operators at one precedence level with `resource_limit`
diagnostics. Split complex expressions into named locals. These bounds protect
compiler stack depth; they are not an incremental cache or a production memory
budget implementation.

Bounds checks and integer-overflow checks remain mandatory. Native allocation
failure terminates with a structured diagnostic. Bounds diagnostics identify
the failing call's source offset.

The implemented subset does not yet include generic lists, arbitrary records or
unions, pattern matching on user-defined types, error propagation syntax,
list-valued property generators/shrinkers, general structured tasks/channels, or capability simulation.
Scoped pure integer `parallel.map` and recoverable text results are implemented;
see the [standard-library guide](stdlib.md) (`keel agent spec stdlib`).
