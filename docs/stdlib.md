# Standard library and practical application gaps

Keel is experimental. These built-ins ship with the compiler; no imports, package
registry, implicit downloads, or source-level FFI are involved. Discover this guide
with `keel agent spec stdlib` and exact signatures with `keel api NAME --json`.
The existing ownership, left-to-right argument evaluation, effect checking, and
exhaustive matching rules apply to every operation below.

## Small JSON example

```text
match json.text(body, "/answers/department/choice") {
    Ok(team) => { io.println(team) }
    Err(error) => { io.println(error) }
}
```

`Result<Text, Text>` owns either an `Ok(Text)` or `Err(Text)`. Both payloads are
read borrows inside their match arms; use `text.clone` to keep one. Constructors
are `result.text_ok(take value)` and `result.text_err(take error)`. Existing
`result.ok`/`result.err` retain their integer result type. There is no implicit
unwrap, exception, or `?` propagation.

## Data and configuration

All Text arguments below are borrowed (`read`). Operations are pure unless an
effect is listed. Text-returning recoverable operations return
`Result<Text, Text>`; malformed input does not trap.

| Built-in | Arguments → result | Behavior |
| --- | --- | --- |
| `json.parse` | Text → text result | Validate a complete JSON document and return an owned copy of its original spelling. |
| `json.get` | document, pointer → text result | Select a value using an RFC 6901 JSON Pointer; return its JSON source spelling. Empty pointer selects the root. `~0` means `~`, `~1` means `/`. |
| `json.text` | document, pointer → text result | Select and decode a JSON string, including Unicode escapes. Other types return Err. |
| `json.int` | document, pointer → `Result<Int, Text>` | Select an integer and check the signed 64-bit range; fractions/exponents are errors. |
| `json.quote` | Text → Text | Escape text as a JSON string, including quotes and control characters. |
| `csv.get` | document, row: Int, column: Int → text result | Zero-based cell; comma delimiter, LF/CRLF records, doubled quotes, embedded newlines. Validates the entire input. No header inference. Empty input has zero records; trailing delimiters create empty cells. Bare CR and malformed quotes are errors. |
| `xml.text` | document, path → text result | Text content of the first matching element at each step, e.g. `/root/name`; includes descendant text. UTF-8 XML with DTDs disabled. Paths only select elements without namespaces; this is not XPath. |
| `sse.data` | buffered stream, index: Int → text result | Zero-based dispatched event's joined `data` lines. Handles CR/LF/CRLF, comments, empty data, BOM. An event must end with a blank line; unfinished events are not dispatched. |
| `dotenv.get` | document, key → text result | Read `KEY=value` lines; surrounding whitespace and matching single/double quotes are removed. Whole-line `#` comments are ignored; duplicate keys use the last value. No shell execution, interpolation, escapes, inline comments, `export`, or multiline values. |
| `fs.read_text` | path → text result | Read a regular UTF-8 file, with `fs.read` effect and exact path permission. Reject final symlinks and nonregular files. |
| `fs.write_text` | path, text → `Result<Int, Text>` | Atomically replace a regular file using a sibling temporary file; return byte count. Requires `fs.write` and an exact write grant. Parent directory must exist. Reject an existing symlink/nonregular target. |
| `env.get` | name → text result | Read a UTF-8 environment value, with `env.read` effect and exact name permission. Missing values are Err. |
| `http.json_response` | status: Int, JSON body → text result | Validate JSON and construct an HTTP/1.1 response with `Content-Type: application/json`. Status must be 100–599. |

JSON input is capped at 1 MiB and 64 nesting levels. Strings reject invalid UTF-8
and lone UTF-16 surrogates. Duplicate object keys select the last member; all
members must still be valid. Missing paths, JSON null, and empty strings are
separate outcomes. `json.get` preserves decimals/exponents as Text without loss;
Keel does not yet have a floating-point or decimal arithmetic type. Each lookup
parses again; retained typed JSON trees and record decoding are future work.
`json.quote` accepts at most 1 MiB and traps with `resource_limit` above that bound.
CSV, XML, SSE, dotenv, file contents, and environment values are capped at 1 MiB.
XML rejects DTD declarations, external entities, malformed documents, and paths
that are not absolute element paths. This small API omits schema validation,
namespace queries, attributes, and XPath. XML element depth is capped at 64.

## Fetching and protocols

All clients require `effects { net.connect }` and separate runtime permission.
Their Text inputs are borrowed; results are `Result<Text, Text>`. They are
synchronous, bounded convenience adapters, not persistent connection objects.

| Built-in | Arguments | Successful result |
| --- | --- | --- |
| `http.get` | URL | HTTP response accepted by `http.status` and `http.body`. |
| `http.post_json` | URL, JSON body, bearer token | HTTP response; an empty token omits Authorization. |
| `http.post_json_timeout` | URL, JSON body, bearer token, timeout_ms: Int | Same response, with an explicit total transfer limit of 1..120000 ms. Useful for bounded AI worker requests. |
| `tcp.exchange` | numeric IPv4 address, port: Int, request | Send request, half-close writes, read UTF-8 reply until EOF. The peer must close its write side. |
| `udp.exchange` | numeric IPv4 address, port: Int, request | Send one datagram and receive one UTF-8 datagram from the connected peer. |
| `websocket.exchange` | ws/wss URL, text message | Send one text message and receive one complete text message; reassemble fragments. Requires libcurl 8.16+ built with WebSocket support. |

Clients default to a 10-second operation deadline, 1 MiB response/message bound, and
recoverable transport/UTF-8 errors. UDP payloads are limited to 65,507 bytes.
`http.post_json_timeout` explicitly overrides the HTTP total deadline; invalid
timeouts return Err. Run long calls in background workers to keep serving frames.
HTTP connection timeout is 5 seconds within the total transfer timeout. DNS
resolution timeout behavior depends on libcurl's resolver build. TLS verification
is enabled. Redirects and ambient proxies are disabled. Non-2xx HTTP responses
remain successful *transfers*: check `http.status` before consuming the body.
Returned HTTP responses are normalized containers for status/body, not original
wire bytes; response headers are not exposed. The bearer token is never written
to diagnostics or command arguments by the adapter. Text is not a secret type;
application code can still print it.

SSE currently supports parsing finite buffered responses: fetch with `http.get`,
extract `http.body`, then call `sse.data`. There is no live streaming callback,
reconnect, Last-Event-ID storage, backpressure, or long-lived SSE server API.
WebSocket is one exchange, not a session API; binary messages, subprotocols,
user headers, authentication helpers, reconnection, and a server API are absent.
Hosts without WebSocket-enabled libcurl return Err; this is not a successful
protocol test. TCP/UDP support IPv4 request/reply only, with no DNS, listening,
TLS, binary buffers, or reusable socket handles.

## Application server and static assets

`http.serve_app(port, static_root, handler)` is a special compiler intrinsic.
The named handler must have signature
`fn(method: read Text, path: read Text, body: read Text) -> Text`. It may declare
effects; the caller must declare those effects plus `net.listen` and `fs.read`.
The handler receives the HTTP method, path before `?` without URL decoding,
and the complete UTF-8 body. Use `http.json_response` for JSON APIs. Existing
`http.serve` retains its original pure GET/path-only behavior.

A 404 returned by the handler on GET/HEAD falls back to `static_root`. An empty
root disables files. Directory requests serve `index.html`; HTML, CSS, JS/MJS,
JSON, SVG, PNG/JPEG/GIF/WebP/ICO, WOFF/WOFF2, WASM, PDF, and TXT have MIME types;
other extensions use `application/octet-stream`. HEAD sends headers without a
body. Binary file bytes stay inside the trusted host, not in language Text.

The launcher must grant the exact nonempty root using `--allow-read=public` or
policy `read: ["public"]`, as well as the listen address. This root grant allows
the host to serve files below that directory; `fs.read_text` still uses exact
file grants. Only place public assets in the root. Paths are opened relative to
an open root descriptor, rejecting dot segments, dot files, encoded separators,
symlinks and nonregular files. The root itself cannot be a symlink; root parent
directories and local file modifications remain trusted.

The server listens on localhost and handles one request per connection,
sequentially. Headers are capped at 16 KiB, request bodies at 1 MiB, paths at
4096 bytes, and static files at 8 MiB. Reception and transmission each have a
two-second total I/O deadline; handler execution is not deadline-limited.
Content-Length framing is supported; duplicate lengths, Transfer-Encoding,
Expect, malformed headers, invalid UTF-8 bodies, and NUL bytes are rejected.
There is no TLS, keep-alive, streaming, multipart parsing, header/query API,
WebSocket server, or general concurrency. See `examples/http_app` for a browser
page and JSON POST endpoint without an external HTTP host. This remains an
experimental adapter, not a production web framework.

Requests must include a Host of `localhost:PORT` or `127.0.0.1:PORT` (the port may
be omitted only on port 80). Browser Origin, when present, must match that exact
HTTP origin; cross-site Fetch Metadata is rejected. This blocks browser
cross-origin mutation and rebinding hostnames before effectful handlers run.
It is not authentication against other local processes.

## Native application orchestration

These POSIX adapters let Keel own a local application's state and worker
processes. Arguments to processes are a JSON array of strings because general
`List<Text>` is not implemented. There is no automatic shell, interpolation,
PATH search, or package installation.

| Built-in | Signature | Behavior |
| --- | --- | --- |
| `process.run` | `(read Text, read Text) -> Result<Text, Text>` | Executable path and JSON argv; wait up to 30 seconds, capture at most 1 MiB combined stdout/stderr. Ok contains output on exit 0; Err contains captured output on a nonzero exit, or a host error. |
| `process.run_timeout` | `(read Text, read Text, Int) -> Result<Text, Text>` | Like `process.run`, with an explicit deadline of 1..30000 ms. Invalid limits return Err. Timeout kills the owned process group and returns Err. |
| `process.spawn` | `(read Text, read Text) -> Result<Int, Text>` | Launch a background child with stdin/stdout/stderr redirected to `/dev/null`; return an opaque positive handle. Successful spawn does not imply successful execution: poll its exit status. |
| `process.poll` | `(Int) -> Result<Int, Text>` | For an owned handle, return -1 while running, otherwise its exit status and release the handle. Signaled status is 128 + signal number. Unknown/released handles return Err. |
| `process.terminate` | `(Int) -> Result<Int, Text>` | Kill an owned child's process group, wait, return status, and release the handle. |
| `clock.millis` | `() -> Int` | Monotonic milliseconds from a system-defined origin; useful for elapsed time, not calendar timestamps. |

Process functions require `process.exec`; run/spawn additionally require an
exact `--allow-exec=PATH` grant (including `run_timeout`). Executable paths must contain `/`. There are at
most 64 arguments, 64 KiB of argument JSON, and 32 outstanding process handles.
Poll finished children to release their handles. Background workers have no
automatic duration limit: the application must track a deadline and terminate
them. Owned process groups are cleaned up when collected, on normal process
exit, and on SIGINT/SIGTERM. This covers the immediate child group, not a recursive
process tree: nested Keel process calls create separate groups and may outlive
a terminated parent worker. SIGKILL, crashes, and other detached descendants are
also outside this cleanup guarantee. Give nested tools their own deadlines.

Children inherit the current directory and environment. An executable grant
authorizes that executable with caller-selected arguments, not an OS sandbox
or restriction on its transitive behavior. A compiler/tool grant can be broad.
Pass child Keel runtime grants explicitly in argv, and keep generated ability
binaries separate from workers authorized to read API credentials. Captured
process output can include private data; do not blindly expose it in a browser.

`clock.millis` requires `clock.read` and `--allow-clock=monotonic`.
`fs.write_text` requires `fs.write` and `--allow-write=PATH`, accepts at most
1 MiB of Text, uses a 0600 sibling temporary file, and atomically renames it over
the destination. It does not create parent directories or promise fsync crash
durability. Parent paths/local filesystem writers remain trusted. The reference
engine returns BLOCKED for all these host operations; offline pure application
logic can still be tested by both engines.

## Scoped threading

`parallel.map(values, worker)` takes a borrowed `List<Int>` and a named pure
`fn worker(value: Int) -> Int`; it returns an owned ordered `List<Int>`. Up to four
native pthread workers copy integer inputs into isolated function calls. All
workers join before returning. Nested maps execute serially inside workers;
inputs are capped at 100,000 elements. Worker scheduling is unspecified, while
argument evaluation and returned element order remain deterministic. A worker
trap terminates the process; there is no cancellation or recovery from traps.
The reference engine maps serially as the independent functional oracle.

There are no escaping threads, shared mutable values, channel objects, async
functions, captured closures, task cancellation, or scheduler simulation yet.
Pure workers cannot access files, networking, environment values, or stdout.

## Runtime policy and native dependencies

Permissions belong to the launcher, not the source. Existing policies remain
valid. New optional policy arrays map to repeatable command-line grants:

```json
{
  "schema": 1,
  "stdout": true,
  "listen": [],
  "read": [".env", "request.json"],
  "write": ["state.json"],
  "exec": ["./build/worker"],
  "clock": ["monotonic"],
  "env": ["JEV_API_KEY"],
  "connect": ["https://api.typesafe.ai", "tcp://127.0.0.1:9000", "udp://127.0.0.1:9001"]
}
```

Equivalent flags are `--allow-read=PATH`, `--allow-write=PATH`,
`--allow-exec=PATH`, `--allow-clock=monotonic`, `--allow-env=NAME`, and
`--allow-connect=ORIGIN`, plus the existing stdout/listen flags. Policy and flag
overrides cannot be combined. At most 256 new scoped grants are supported.
Origins match exactly: scheme, host, and explicit port if present. URL userinfo
and ambiguous/control-character authority forms are rejected. Files match exact
path strings relative to the process working directory, not the manifest's
location. Parent symlinks and DNS rebinding are not sandboxed; these adapters do
not establish an OS security boundary or a destination-IP policy.

HTTP/WebSocket applications require `pkg-config` and libcurl development headers
and libraries. XML applications require `pkg-config` and libxml2. Other programs
do not link those libraries. Builds fail with a dependency diagnostic rather than
installing packages. On Debian/Ubuntu the packages are `pkg-config`,
`libcurl4-openssl-dev`, and `libxml2-dev`. The macOS SDK supplies curl/XML;
`pkg-config` must be able to locate them. Curl 8.16+ is required for fragmented WebSocket messages with interleaved control frames; a version number alone does not guarantee compiled-in ws support.
These system libraries join the trusted C runtime/system compiler boundary.

## Review: what remains missing

| Need | Implemented now | Remaining gap |
| --- | --- | --- |
| JSON applications | Validation, pointers, string/int decoding, escaping, JSON responses | Typed records, retained JSON values, numeric types, mutation/builders, Result propagation. |
| Files and data formats | Bounded UTF-8 reads/atomic writes, CSV cells, XML element text, dotenv/env | Directories, binary language I/O, streaming parsers, general text collections. |
| Network applications | HTTP GET/JSON POST, TCP/UDP exchanges, conditional WebSocket exchange, buffered SSE | Session handles, protocol servers, streaming, richer request/response types, cancellation/backpressure. |
| Threading | Scoped pure integer map | General structured tasks, channels, shared ownership, race/lifetime review and deterministic simulation. |
| Libraries/packages | Compiler-known built-ins with embedded docs | Namespaced modules, dependency resolution, lockfiles, package registry and ABI stability. |
| Agent ergonomics | Offline API lookup, small pointer expressions, explicit errors | General records/generics, `?`, fluent builders, structured error types and secret types. |

See `examples/stdlib.keel` for offline regression examples and `examples/jev` for
an end-to-end real API call. The reference engine never performs host I/O;
loopback native integration tests cover transport and permissions. Passing these
tests is sampled evidence, not production certification or formal proof.

Remaining application-library work is tracked in [issue #1](https://github.com/jakecyr/keel/issues/1).

See [local validation evidence](stdlib-validation.md) for test scope and WebSocket version checks.
