# Working with Keel

[Back to the quick start](../README.md)

## Build, format, lint, and test

From your project directory:

```sh
keel fmt .                         # Format source indentation
keel check .                       # Types, ownership, and effects
keel lint .                        # Unused bindings and effects
keel test .                        # Native examples and properties
keel test . --engine both           # Compare native and reference execution
keel build . -o build/hello
./build/hello --allow-stdout
```

The `.` is optional: project commands default to the current directory. Each command also accepts a single `.keel` file or another project path. Add `--json` for structured diagnostics. Use `keel fmt --check` and `keel lint --deny-warnings` in CI.

Property-test budgets and replay are explicit:

```sh
keel test . --cases 1000 --seed 42 --timeout-ms 2000 --budget-ms 30000
```

For an existing failing property, replay with `keel test --filter "exact property name" --value 17 --json`. Replace the name and input with the reported case; the starter project contains an example test, not a property.

`TESTED` means the recorded cases passed. Reached holes are `BLOCKED`; timeouts are `UNKNOWN`. Neither counts as a pass. Linux workers enforce the configured memory limit; macOS currently reports that limit as unenforced.

## Coding agents

Tell your agent: **“Read AGENTS.md and run `keel agent context . --json`.”** The generated `AGENTS.md` and `CLAUDE.md` already contain this instruction.

The installed CLI includes versioned, offline language documentation:

```sh
keel agent context . --json
keel agent context . --symbol greet --json
keel agent spec language
keel agent spec collections
keel agent commands --json
keel api list.get --json
```

Context includes supported syntax, available commands, the current source revision, the requested function, dependency signatures, callers, contracts, and holes. It marks truncated implementation snippets. Agents can ask for more without reading the whole repository.

Agents can use ordinary edits or revision-bound `keel edit` transactions. Test files named in the manifest—including their oracle/helper functions—are protected from structural edits. Stale or invalid transactions leave source unchanged. For integrations that keep a process open, `keel serve` provides a JSON-lines interface with bounded snapshot caching. [Agent protocol and edit examples →](agent-protocol.md)

## Try the web server

From the compiler repository:

```sh
keel test examples/web --engine both --cases 1000 --seed 42
keel run examples/web --policy examples/web/keel.policy.json
```

In another terminal:

```sh
curl http://127.0.0.1:8080/
curl http://127.0.0.1:8080/health
curl http://127.0.0.1:8080/square
curl -i http://127.0.0.1:8080/missing
```

The routes return a greeting, `ok`, `144`, and a 404. Stop the server with Ctrl-C. The example has separate implementation and acceptance-test files. Its HTTP host is a small sequential localhost demonstration, not a production web framework.

## Contribute and evaluate

```sh
cargo test --locked --all-targets
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
python3 -m unittest discover -s benchmarks -p 'test_*.py'
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
```

GitHub Actions runs compiler, native/sanitizer, CLI, HTTP, tooling, installer, and benchmark-methodology tests on Linux/macOS, on both x86_64 and ARM64. Version tags publish a release only after all four validation jobs pass, then test the real download-to-init workflow on every platform. Ordinary pushes do not publish releases. CI does not run paid agent evaluations.

[Benchmark instructions](../benchmarks/README.md) separate native/iteration timings from real-agent evaluation. Missing billing, comparable baseline tooling, or independent acceptance evidence remains `UNKNOWN`; faster scripted edits do not prove lower cost per accepted agent change. [Release-readiness audit →](release-gaps.md)

The [recorded pilot](../benchmarks/results/README.md) does **not** establish the proposed 25% agent-cost advantage: all 12 repairs passed behavioral assertions but exceeded the registered token budget. Keel's protocol condition used more reported tokens than the improved C baseline. [Validation coverage →](validation.md)
