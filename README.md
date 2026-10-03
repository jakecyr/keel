# Keel

A working, experimental implementation of an agent-oriented language: readable source, static checks, native executables, and structured development feedback.

The repository contains a Rust compiler, a small trusted C runtime, a localhost web server written in Keel, and compiler/runtime regression tests. It implements a deliberately small **v0**, not the entire proposed language. The exact semantics and remaining design are in [docs/language.md](docs/language.md) and [docs/architecture.md](docs/architecture.md).

## Run the web server

Requires Rust/Cargo and a POSIX environment with a GCC/Clang-compatible C compiler. Tested on macOS ARM64. The regression suite also uses AddressSanitizer and UndefinedBehaviorSanitizer.

```sh
cargo build --release --locked
./target/release/keel check examples/web_server.keel
./target/release/keel test examples/web_server.keel --cases 1000 --seed 42
./target/release/keel run examples/web_server.keel --allow-net=127.0.0.1:8080
```

From another terminal:

```sh
curl -i http://127.0.0.1:8080/
curl -i http://127.0.0.1:8080/health
curl -i http://127.0.0.1:8080/square
curl -i http://127.0.0.1:8080/missing
```

These return a greeting, `ok`, `144`, and a 404. Stop the server with Ctrl-C. The binary is `build/web_server`; it runs independently of the compiler. Running it without the permission flag fails before opening the listening socket.

The router is ordinary Keel:

```text
pub fn route(path: read Text) -> Text {
    if path == "/health" {
        return http.response(200, "ok\n")
    }
    return http.response(404, "not found\n")
}

fn main() effects { net.listen } {
    http.serve(8080, route)
}

test "health returns ok" {
    let response = route("/health")
    assert http.status(response) == 200
    assert http.body(response) == "ok\n"
}
```

The HTTP adapter is a bounded, sequential demonstration host: localhost only, GET requests, a 16 KiB request-header buffer, two-second socket timeouts, and one response per connection. It is not a production web framework. There is no TLS, request body API, async scheduling, authentication, or graceful shutdown protocol.

## Agent development loop

```sh
# Small source bundle, signatures, dependencies, callers, effects, and holes.
./target/release/keel inspect examples/web_server.keel --symbol route --json

# Static checks produce source-linked diagnostics.
./target/release/keel check examples/web_server.keel --json

# Deterministic boundary-first integer properties; isolated test workers.
./target/release/keel test examples/web_server.keel --cases 1000 --seed 42 --json

# Emit the readable intermediate C alongside a native executable.
./target/release/keel build examples/web_server.keel -o build/server --emit-c build/server.c
```

Structural edits consume JSON and replace one function body. Copy the revision from `inspect` into a request file:

```json
{
  "base_revision": "COPY_THE_INSPECT_REVISION_HERE",
  "target": "fn:is_fresh",
  "operation": "replace_body",
  "source": "{\n    return now < deadline\n}",
  "run": "affected_checks_and_tests"
}
```

Then, on a copy of the deliberately broken example:

```sh
mkdir -p build
cp examples/counterexample.keel build/cache.keel
./target/release/keel inspect build/cache.keel --symbol is_fresh --json
# Save the request above as build/fix.json with the returned revision.
./target/release/keel edit build/cache.keel --request build/fix.json --json
./target/release/keel review build/cache.keel --against examples/counterexample.keel --json
```

The edit is checked before writing. With `affected_checks_and_tests`, v0 conservatively runs **all** tests. A stale revision, invalid replacement, failed/blocked test, or timeout prevents the write. The operation cannot replace signatures, contracts, or tests; ordinary filesystem access is outside this protection. Comments outside the replaced body are preserved exactly. The new body retains the supplied layout; canonical formatting is not implemented yet.

Other useful examples:

```sh
./target/release/keel run examples/ownership.keel --allow-stdout
./target/release/keel test examples/holes.keel --json
./target/release/keel test examples/counterexample.keel --json
```

The last two intentionally exit nonzero: one reports a `BLOCKED` test at a typed hole; the other reports the deadline counterexample. Replay a generated failure using its test name and input:

```sh
./target/release/keel test examples/counterexample.keel \
  --filter "expired values are never fresh" --value 0 --json
```

`explain FILE --offset N` shows the surrounding source at a diagnostic's UTF-8 byte offset. It is source context, not an execution trace.

## What works

| Area | Implemented in v0 |
| --- | --- |
| Syntax | Functions, explicit signatures, `let`/`var`, `if`/`else`, `while`, return, assertions, comments |
| Types | Signed 64-bit `Int`, `Bool`, immutable owned `Text`, `Unit`; local inference |
| Arithmetic | Checked overflow, checked division/remainder, defined left-to-right evaluation, short-circuit Boolean operators |
| Ownership | Explicit `take`, call-scoped `read`, explicit cloning, move checking, branch joins, conservative loop checks, deterministic cleanup |
| Effects | Transitive declared-effect checks, pure contracts, runtime network/stdout permission gates |
| Tests | Native examples and bounded integer properties, seeded replay, counterexample shrinking, per-test process/time isolation |
| Contracts | Runtime `requires` and `ensures`; never used as optimizer assumptions |
| Agent tools | `inspect`, `edit`, `check`, `test`, `explain`, `review`, with JSON output |
| Backend | Rust frontend → C11 → installed native compiler; no interpreter in the executable |

Not implemented: records/unions, `Option`/`Result`, collections/generics, `edit` borrows, closures, modules/packages, Cranelift, incremental compilation, canonical formatting, capability objects, stateful simulation, formal proofs, full traces, or memory-budget enforcement. No claim is made that Keel beats existing languages or meets the original latency targets.

## Verification

```sh
cargo test --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
```

Tests cover rejected programs, ownership and effects, native integer semantics against a Rust reference, runtime contracts, hole blocking, shrinking/replay, test timeouts, structural edit rollback, real HTTP requests and permission denial, and text lifetimes under native sanitizers. See [docs/validation.md](docs/validation.md) for the recorded validation and its limits.

The crate uses pinned Serde/JSON dependencies and `Cargo.lock`. If those crates are already cached, add `--offline` to Cargo commands. Generated application binaries do not link Rust, Serde, the compiler, or the test generator.
