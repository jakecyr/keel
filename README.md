# Keel

Readable code. Native executables. A compiler that helps both developers and coding agents inspect, change, and test small units of code.

**Status:** working experimental language. Use it to evaluate the language and build small projects; it is not yet a production-certified toolchain. [Supported features](#what-can-i-write-today) and [release gaps](docs/release-gaps.md) are explicit.

## 1. Install the CLI

From an existing checkout, run `cargo install --path . --locked`. Once the
repository is published on GitHub, a fresh installation is:

```sh
git clone https://github.com/jakecyr/keel.git
cd keel
cargo install --path . --locked
keel --version
keel doctor
```

You need **Rust/Cargo and a C compiler**. On macOS, install the Xcode Command Line Tools with `xcode-select --install`. On Linux, install your distribution's C compiler toolchain. `keel doctor` checks that native compilation works. macOS and Linux are the current target platforms; Windows is not supported yet.

Cargo normally installs `keel` into `~/.cargo/bin`. If your shell cannot find it:

```sh
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
```

Add that line to your shell's configuration if needed. Re-run `cargo install --path . --locked --force` after pulling compiler updates. To uninstall a Cargo installation, use `cargo uninstall keel`.

### Download installation — after a GitHub release is published

Release installation requires published assets in [jakecyr/keel](https://github.com/jakecyr/keel/releases). This development work does not publish a release. CI builds archives and checksums; those are workflow artifacts, not public releases. You can inspect the installer locally:

```sh
sh scripts/install.sh --help
```

After the repository and a release are published:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/jakecyr/keel/main/scripts/install.sh | sh
```

Prefer reviewing the script first. To pin a published version, append `-s -- --version VERSION` to `sh`; forks can pass `--repo OWNER/REPOSITORY`. The installer checks the archive checksum, installs without `sudo`, and prints PATH instructions instead of changing your shell configuration. See `sh scripts/install.sh --help` for installation-directory options. Downloaded Keel compilers still need a local C compiler to build programs; the resulting application binaries do not need Rust or Keel to run.

## 2. Create your first project

```sh
keel init hello-keel
cd hello-keel
keel run . --allow-stdout
```

Output:

```text
Hello from Keel!
```

`keel init` creates:

```text
hello-keel/
├── keel.json                 # Explicit source and acceptance-test files
├── keel.policy.json          # Runtime permissions; starts with no grants
├── src/main.keel             # Your program
├── tests/acceptance.keel      # Independent behavioral checks
├── AGENTS.md                 # Instructions for coding agents
└── CLAUDE.md                 # Same entry point for Claude-based tools
```

Already in an existing repository? Run `keel init .`. It refuses to overwrite existing source files and preserves human-written agent instructions. Re-running init refreshes the marked Keel guidance without replacing your program or tests.

## 3. Build, format, lint, and test

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

Each command also accepts a single `.keel` file. Add `--json` for structured diagnostics. Use `keel fmt . --check` and `keel lint . --deny-warnings` in CI.

Property-test budgets and replay are explicit:

```sh
keel test . --cases 1000 --seed 42 --timeout-ms 2000 --budget-ms 30000
keel test . --filter "exact property name" --value 17 --json
```

`TESTED` means the recorded cases passed. Reached holes are `BLOCKED`; timeouts are `UNKNOWN`. Neither counts as a pass. Linux workers enforce the configured memory limit; macOS currently reports that limit as unenforced.

## 4. Use Keel with a coding agent

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

Agents can use ordinary edits or revision-bound `keel edit` transactions. Test files named in the manifest—including their oracle/helper functions—are protected from structural edits. Stale or invalid transactions leave source unchanged. For integrations that keep a process open, `keel serve` provides a JSON-lines interface with bounded snapshot caching. [Agent protocol and edit examples →](docs/agent-protocol.md)

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

## What can I write today?

```text
pub fn unique(values: read List<Int>) -> List<Int> {
    var output = []
    for value in values {
        if !list.contains(output, value) {
            list.push(edit output, value)
        }
    }
    return output
}

test "keeps first occurrences" {
    assert unique([4, 2, 4, 1, 2]) == [4, 2, 1]
}
```

Implemented: functions, checked 64-bit integers, Bool/Text, owned `List<Int>`, `Option<Int>`, `Result<Int, Text>`, exhaustive matching, restricted `read`/`edit`/`take` ownership, effects, executable contracts, holes, native property tests, shrinking/replay, and a reference evaluator.

The collection/result types are built-in specializations. General records/unions/generics, structured concurrency, capability simulation, Cranelift, and declaration-level incremental compilation remain open work. Native builds currently use generated C plus the installed system compiler. [Language reference →](docs/agent-language.md) · [Collection/error examples →](docs/features.md) · [Architecture →](docs/architecture.md)

## Contribute and evaluate

```sh
cargo test --locked --all-targets
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
python3 -m unittest discover -s benchmarks -p 'test_*.py'
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
```

GitHub Actions runs compiler, native/sanitizer, CLI, HTTP, tooling, installer, and benchmark-methodology tests on Linux/macOS. CI also exercises installation and retains compiled CLI archives plus validation reports. It does not run paid agent evaluations or publish releases automatically.

[Benchmark instructions](benchmarks/README.md) separate native/iteration timings from real-agent evaluation. Missing billing, comparable baseline tooling, or independent acceptance evidence remains `UNKNOWN`; faster scripted edits do not prove lower cost per accepted agent change. [Release-readiness audit →](docs/release-gaps.md)

The [recorded pilot](benchmarks/results/README.md) does **not** establish the proposed 25% agent-cost advantage: all 12 repairs passed behavioral assertions but exceeded the registered token budget. Keel's protocol condition used more reported tokens than the improved C baseline. [Validation coverage →](docs/validation.md)
