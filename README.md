# Keel

Readable code. Native executables. A compiler that helps both developers and coding agents inspect, change, and test small units of code.

**Status:** working experimental language. Use it to evaluate the language and build small projects; it is not yet a production-certified toolchain. [Supported features](#what-can-i-write-today) and [release gaps](docs/release-gaps.md) are explicit.

## 1. Install the CLI

Download a prebuilt compiler—**no Git clone or Rust installation required**:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/jakecyr/keel/master/scripts/install.sh | bash
export PATH="$HOME/.local/bin:$PATH"
keel --version
keel doctor
```

The installer selects your platform, verifies SHA-256, and installs to `~/.local/bin` without `sudo`. It prints a copyable PATH command and leaves your shell files unchanged. Add the `export PATH` line above to `~/.zshrc` or `~/.bashrc` to keep it across terminals. Prefer [reviewing the script](scripts/install.sh) before running it.

Supported release targets: Linux x86_64 (glibc 2.35+, e.g. Ubuntu 22.04+), Linux ARM64 (glibc 2.39+, e.g. Ubuntu 24.04+), and macOS 15+ on Apple Silicon or Intel. Windows and Alpine/musl are not supported. macOS binaries are not Apple-notarized. Checksums verify integrity, not an independent publisher signature.

You still need a **C compiler to build Keel programs**. On macOS, run `xcode-select --install`; on Debian/Ubuntu, install `build-essential`. `keel doctor` checks this prerequisite. The resulting application binaries do not need Rust or Keel to run.

### Upgrade, pin a version, or choose a directory

Re-run the install command to upgrade. To install a specific [published release](https://github.com/jakecyr/keel/releases) or a different directory:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/jakecyr/keel/master/scripts/install.sh | bash -s -- --version v0.1.0 --prefix "$HOME/.local"
```

`--prefix DIRECTORY` installs into `DIRECTORY/bin`; `--bin-dir DIRECTORY` chooses the exact binary directory. Both require absolute paths. Forks can use `--repo OWNER/REPOSITORY`. Use `bash -s -- --help` to view installer options without downloading a compiler. To uninstall the default download installation, remove only `~/.local/bin/keel`; projects are separate and remain untouched.

### Build from source (contributors)

Source builds additionally require Rust/Cargo. From an existing checkout, run `cargo install --path . --locked`; for a fresh checkout:

```sh
git clone https://github.com/jakecyr/keel.git
cd keel
cargo install --path . --locked
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
```

Use `--force` when reinstalling an updated source checkout, and `cargo uninstall keel` to remove a Cargo installation. If you have both installations, `command -v keel` shows which one your PATH selects.

### VS Code syntax highlighting

The [Keel language extension](editors/vscode/README.md) adds highlighting for
`.keel` files, comment toggling, and bracket/quote pairing. Open `editors/vscode`
in VS Code and press **F5** to preview it, or follow its README to package and
install it locally.

## 2. Create your first project

```sh
keel init hello-keel
cd hello-keel
keel test --engine both
keel run --allow-stdout
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

Already in an existing repository? Run `keel init`. It refuses to overwrite existing source files and preserves human-written agent instructions. Re-running init refreshes the marked Keel guidance without replacing your program or tests. Edit `src/main.keel` to change your program; keep approved acceptance checks in `tests/acceptance.keel` independent of the implementation.

Read the root [syntax and semantics guide](LANGUAGE.md) next. Use `keel help init`, `keel test --help`, or `keel agent spec language` for help without a checkout or internet connection.

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

The `.` is optional: project commands default to the current directory. Each command also accepts a single `.keel` file or another project path. Add `--json` for structured diagnostics. Use `keel fmt --check` and `keel lint --deny-warnings` in CI.

Property-test budgets and replay are explicit:

```sh
keel test . --cases 1000 --seed 42 --timeout-ms 2000 --budget-ms 30000
```

For an existing failing property, replay with `keel test --filter "exact property name" --value 17 --json`. Replace the name and input with the reported case; the starter project contains an example test, not a property.

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

GitHub Actions runs compiler, native/sanitizer, CLI, HTTP, tooling, installer, and benchmark-methodology tests on Linux/macOS, on both x86_64 and ARM64. Version tags publish a release only after all four validation jobs pass, then test the real download-to-init workflow on every platform. Ordinary pushes do not publish releases. CI does not run paid agent evaluations.

[Benchmark instructions](benchmarks/README.md) separate native/iteration timings from real-agent evaluation. Missing billing, comparable baseline tooling, or independent acceptance evidence remains `UNKNOWN`; faster scripted edits do not prove lower cost per accepted agent change. [Release-readiness audit →](docs/release-gaps.md)

The [recorded pilot](benchmarks/results/README.md) does **not** establish the proposed 25% agent-cost advantage: all 12 repairs passed behavioral assertions but exceeded the registered token budget. Keel's protocol condition used more reported tokens than the improved C baseline. [Validation coverage →](docs/validation.md)
