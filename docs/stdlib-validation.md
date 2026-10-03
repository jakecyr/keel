# Standard-library development evidence

Development validation on macOS ARM64, October 2026; Keel 0.1.0 development
changes based on `203350f`. This is sampled local evidence, not a release,
production security audit, or agent-efficiency benchmark.

- `cargo fmt --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo test --locked --all-targets`: 61 unit/compiler tests, 11 agent workflow
  tests, and 19 CLI integration tests passed in the completed standard-library
  validation run.
- Benchmark methodology: 38 Python tests passed. Installer: 17 Python tests passed.
- New tests cover text-result ownership/exhaustiveness, effect denial, native vs
  independent reference parsing, invalid JSON and nesting bounds, CSV/XML/SSE,
  parallel mapping, file/environment permissions, loopback HTTP/TCP/UDP,
  non-2xx status handling, disabled redirects, and resource-limit uncertainty.
- Address/undefined-behavior sanitizer tests passed for the pure standard-library
  corpus, including text-result cleanup and nested parallel maps.
- Jev offline tests passed with `--engine both`; formatting and linting passed.
  One live synthetic request returned HTTP 200. Its selected fields and token
  usage are recorded in `examples/jev/sample-output.txt`; no credential was logged.
- Cargo package inventory excluded `examples/jev/.env` and local build artifacts.

## WebSocket compatibility evidence

The system libcurl 8.7.1 lacks ws support; the recoverable unavailable path passed.
An isolated ws-enabled 8.7.1 build exposed incorrect continuation semantics.
An isolated 8.13.0 build failed a fragmented response with an interleaved ping.
Source inspection confirmed that the control frame cleared continuation state.
The adapter therefore requires both headers and runtime libcurl 8.16 or newer,
with ws enabled. The original failing fixture was retained.

The same masked-request/fragmented-response/interleaved-ping fixture passed
against a locally built libcurl 8.16.0. Local validation libraries were installed
only below ignored `build/`; no system library was replaced. This isolated build
had TLS disabled for the loopback ws test, so it is not wss/TLS validation. The
separate live Jev call exercised HTTPS using the system's TLS-enabled libcurl.

To require rather than conditionally skip the wire test, point `PKG_CONFIG_PATH`
at a suitable libcurl and run:

```sh
KEEL_REQUIRE_WEBSOCKET_TEST=1 cargo test --locked websocket_capability_and_loopback -- --nocapture
```

CI configuration installs a recent Homebrew curl on macOS and requires this test.
That updated remote CI configuration has not been executed in this session.
