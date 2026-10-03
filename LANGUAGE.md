# Keel: syntax and semantics

This is the starting guide for developers and agents using **Keel 0.1.0**.
It describes implemented features, not the full proposed language. Keel is
experimental. See the [detailed language reference](docs/language.md),
[collections and errors](docs/features.md), and [remaining work](docs/release-gaps.md).

Without a checkout, the installed compiler provides versioned offline guidance:

```sh
keel agent spec language
keel agent spec collections
keel agent spec stdlib
keel agent spec protocol
keel api list.get --json
keel agent context . --symbol greet --json
```

## A complete program

Save this as `greeting.keel`; run `keel test greeting.keel --engine both`, then
`keel run greeting.keel --allow-stdout`. Runnable `keel` blocks in this guide are
compiled and tested in the repository's automated suite.

```keel
pub fn greet(name: read Text) -> Text {
    return text.concat("Hello, ", name)
}

fn main() effects { io.stdout } {
    io.println(greet("Keel"))
}

test "greeting" {
    assert greet("developer") == "Hello, developer"
}
```

Functions declare parameter types and return types (`Unit` if omitted). `pub`
marks an intended public interface, not module-level access control. `//` starts
a comment. Blocks use braces. Semicolons are optional; newlines are whitespace.
Names are ASCII identifiers. Strings are UTF-8 with ordinary escaped quotes,
backslashes, newline, carriage-return, and tab escapes.

## Values and control flow

- `let` creates an immutable binding; `var` permits assignment. Shadowing is rejected.
  `result` is reserved for postconditions; use `output` or another name for locals
  and parameters, including in functions without contracts.
- Types: `Int`, `Bool`, `Text`, `List<Int>`, `Option<Int>`, `Result<Int, Text>`, `Result<Text, Text>`, `Unit`.
  These container types are built-in specializations, not general generics.
- Use `if`/`else`, `while`, `for value in list`, exhaustive `match`, and `return`.
  A non-Unit function must return on every checked path. Use `return;` for Unit.
- `Int` is signed 64-bit. Overflow and division by zero trap, including in release
  builds. Division truncates toward zero. There are no implicit conversions.
- Evaluation is left to right; `&&` and `||` short-circuit. Text comparison is by
  bytes and length. `text.len` counts bytes, not Unicode characters.
- List indexing uses `list.get` (returns Option) or `list.at` (traps out of bounds).

## Ownership: read, edit, take

Int, Bool, and Option<Int> copy. Text, List<Int>, Result<Int, Text>, and Result<Text, Text> are owned:

- `read` borrows without mutation for a call.
- `edit` borrows exclusively; the caller writes `edit value` and needs a mutable binding.
- `take` transfers ownership; the caller writes `take value` for an existing binding.
  The old binding can no longer be read until reassigned.
- Returning an owned local transfers it. Borrowed values cannot escape; clone
  explicitly when an independent owner is needed. There are no hidden deep copies.
- Loans include later argument evaluation. A `for` loop read-borrows its input
  throughout the loop. Restore moved outer values on every iteration.
- Owned values are cleaned up on normal scope exit, replacement, and return.
  Traps terminate the process without unwinding cleanup.

```keel
fn unique(values: read List<Int>) -> List<Int> {
    var output = []
    for value in values {
        if !list.contains(output, value) {
            list.push(edit output, value)
        }
    }
    return output
}

test "first occurrences" {
    assert unique([4, 2, 4, 1, 2]) == [4, 2, 1]
}
```

## Recoverable errors

Handle both arms of Option/Result with `match`. There are no implicit nulls,
exceptions, wildcard arms, or `?` propagation yet. Err text is borrowed inside
its match arm; explicitly clone it to retain it. Strict integer parsing accepts
an optional minus and decimal digits; whitespace, plus signs, and overflow fail.

```keel
fn parsed_or_zero(raw: read Text) -> Int {
    match text.parse_int(raw) {
        Ok(value) => { return value }
        Err(message) => { assert text.len(message) > 0 return 0 }
    }
}

test "recoverable parsing" {
    assert parsed_or_zero("42") == 42
    assert parsed_or_zero("bad") == 0
}
```

## Effects, contracts, holes, and evidence

Functions are externally pure unless declaring effects such as `io.stdout`,
`net.listen`, `net.connect`, `fs.read`, `fs.write`, `env.read`, `process.exec`, or `clock.read`; local mutation/allocation are allowed. Effects propagate
through calls. Declarations grant no authority: a launcher must separately grant
permissions. `keel init` creates a deny-by-default policy. A greeting therefore
needs `keel run --allow-stdout`; policy files are never silently broadened.

`requires` checks entry conditions; `ensures` checks the returned `result`.
Contracts must be pure, cannot consume/edit borrowed inputs, and run in optimized
builds. They are executable checks, never proofs or optimizer assumptions. Use
typed errors for ordinary invalid external input, not precondition traps.

```keel
pub fn square(value: Int) -> Int
    requires value >= -1000 && value <= 1000
    ensures result >= 0
{
    return value * value
}

test "square" { assert square(12) == 144 }
property "nonnegative" (n in gen.int(min: -1000, max: 1000)) {
    assert square(n) >= 0
}
```

Properties currently support one bounded integer generator. Seeds and budgets
are explicit; failures may be shrunk and replayed. Independent specifications
and approved acceptance assertions must not be weakened to pass a change.

`hole("name")` represents unfinished work and needs an expected type. Inspection
reports its context; builds reject unresolved holes. `TESTED` means recorded cases
passed, reached holes are `BLOCKED`, and time/resource exhaustion is `UNKNOWN`.
None means `PROVEN`. Native/reference agreement is additional sampled evidence.

## Standard library: JSON, files, networking, and threading

Built-ins cover JSON validation/pointers/escaping, CSV cells, XML element text,
file and environment reads, dotenv, HTTP fetching/JSON responses, TCP/UDP
exchanges, conditional WebSocket exchanges, buffered SSE parsing, and scoped
`parallel.map`. Read the [standard-library guide](docs/stdlib.md) or run
`keel agent spec stdlib` for exact limits and native dependencies. These are
built-ins, not a package manager or a production networking framework.

```keel
fn square_item(value: Int) -> Int { return value * value }
test "JSON and scoped workers" {
    match json.text("{\"team\":\"billing\"}", "/team") {
        Ok(team) => { assert team == "billing" }
        Err(error) => { assert false }
    }
    assert parallel.map([3, 2, 1], square_item) == [9, 4, 1]
}
```

`Result<Text, Text>` owns its success or error text; match payloads are borrowed.
Use `text.clone` to return either payload. Host calls require separate exact
`--allow-read=PATH`, `--allow-env=NAME`, or `--allow-connect=ORIGIN` grants (or a
launcher policy). The [Jev example](examples/jev/README.md) queries an API and
uses a decoded JSON answer with no hand-written JSON parser.

## Projects and agent workflow

`keel init` scaffolds the current directory; `keel init NAME` creates a directory.
`keel.json` explicitly lists complete source/test files in one namespace; it is
not a module or package manager. No dependencies are downloaded during builds.

```sh
keel fmt --check
keel check
keel lint --deny-warnings
keel test --engine both
keel build
```

Project commands default to `.`. Use `keel COMMAND --help` for options. Agents
should read generated AGENTS.md/CLAUDE.md, inspect focused revision-bound context,
make small edits, run checks/tests, and retain the evidence. See the
[agent protocol](docs/agent-protocol.md) for protected structural transactions.

## Serve a browser app

`http.serve_app(port, static_root, handler)` serves static assets and API routes
from one native process. The named handler receives `method`, `path`, and UTF-8
`body` as three `read Text` parameters and returns an HTTP response. A 404 on
GET/HEAD falls back to the static root, including `index.html` and binary assets
with MIME types. Declare `net.listen`, `fs.read`, and any handler effects; grant
the listen address and exact static root separately in the launcher policy.
Use an empty root for an API-only server. `http.json_response` builds JSON replies.
See [the complete runnable example](examples/http_app/README.md) and
[HTTP limits and permissions](docs/stdlib.md#application-server-and-static-assets).
The existing path-only `http.serve` remains available.

For native application state and background workers, use `fs.write_text`,
`process.run`, `process.run_timeout`, `process.spawn`, `process.poll`, `process.terminate`, and
`clock.millis`. Each host operation requires its declared effect and separate
launcher grants. Process arguments are a JSON string array and never implicitly
run through a shell. Read [the orchestration reference](docs/stdlib.md#native-application-orchestration)
before granting executable authority or launching generated code.

## Not implemented

General records/unions/generics, modules/packages, shared ownership, closures,
general structured tasks/channels, capability simulation, formal proofs, and Cranelift are
not supported. Native compilation currently lowers through C and your system C
compiler. The HTTP host is an example, not a production web framework.
