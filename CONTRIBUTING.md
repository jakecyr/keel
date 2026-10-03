# Contributing to Keel

Keel is an experimental native language with a Rust compiler and a trusted C
runtime. Contributions to the compiler, runtime, tooling, documentation, examples,
and tests are welcome. Read [AGENTS.md](AGENTS.md) for repository instructions and
[release gaps](docs/release-gaps.md) for current limits and required evidence.

## Set up a development checkout

Fork the repository if you need your own push access, clone your fork or the
upstream repository, and create a branch for your change:

```sh
git clone https://github.com/jakecyr/keel.git
cd keel
git switch -c describe-your-change
```

Development requires stable Rust/Cargo with `rustfmt` and `clippy`, a C compiler
(CI uses Clang), and Python 3 for the benchmark and installer tests. CI uses
Python 3.13 and tests Linux and macOS on ARM64 and x86_64; see the
[CI workflow](.github/workflows/ci.yml) for the current matrix.

The native standard-library tests also use `pkg-config`, libcurl, and libxml2.
On Debian/Ubuntu, install the dependencies with:

```sh
sudo apt-get update
sudo apt-get install -y build-essential clang pkg-config libcurl4-openssl-dev libxml2-dev
```

On macOS, install the Xcode Command Line Tools (`xcode-select --install`) if
needed. CI uses Homebrew `pkg-config` and `curl`, with
`PKG_CONFIG_PATH` pointing to `$(brew --prefix curl)/lib/pkgconfig`.
See [standard-library validation](docs/stdlib-validation.md#websocket-compatibility-evidence)
for the optional WebSocket dependency requirements and how to require its wire test.

Build and run directly from the checkout without installing globally:

```sh
cargo build --locked
cargo run --locked -- doctor
cargo run --locked -- agent context --json
cargo run --locked -- agent spec language
cargo run --locked -- agent spec collections
```

Use `cargo run --locked -- api BUILTIN --json`, replacing `BUILTIN` with a
built-in name, to inspect its supported signature. If you want to install your
development checkout, use `cargo install --path . --locked`; the
[installation guide](docs/installation.md) explains PATH setup and removal.

## Find your way around

| Area | Starting points |
| --- | --- |
| Syntax and static checks | `src/syntax.rs`, `src/check.rs` |
| Native lowering and runtime | `src/native.rs`, `src/runtime.c` |
| Reference evaluation and standard library | `src/eval.rs`, `src/stdlib.rs`, `src/stdlib.c` |
| CLI, project handling, and agent tools | `src/main.rs`, `src/cli.rs`, `src/project.rs`, `src/agent.rs`, `src/service.rs` |
| Regression tests | `src/*tests.rs`, `tests/` |
| Examples and language documentation | `examples/`, [language tour](LANGUAGE.md), [language reference](docs/language.md) |
| Evaluation and distribution | `benchmarks/`, `scripts/`, `.github/workflows/` |

Read the [architecture guide](docs/architecture.md) for component responsibilities
and the [development workflows](docs/workflows.md) for CLI examples.

## Make a focused change

Search [existing issues](https://github.com/jakecyr/keel/issues) before starting.
For a substantial language or architecture change, describe the use case and
proposed behavior in an issue so the design can be discussed before implementation.
Clearly label proposed syntax as unimplemented.

Preserve these implementation rules:

- Keep defined left-to-right evaluation, checked integer arithmetic, exhaustive
  handling, and ownership semantics consistent in native and reference engines.
- For ownership, runtime, or lowering changes, add negative compiler tests,
  native tests, and differential/sanitizer coverage. Include a regression that
  exercises the reported failure.
- Keep acceptance assertions, specification helpers, generator domains, and
  expected outputs independent of implementations. Do not weaken them to make a
  failing change pass.
- Update [LANGUAGE.md](LANGUAGE.md), [docs/language.md](docs/language.md), and
  relevant guides embedded by `src/agent.rs` together when supported behavior
  changes. Runnable `keel` fences in the root tour are tested in both engines.
- Distinguish static success, TESTED, ENFORCED, BLOCKED, UNKNOWN, and PROVEN.
  No current tool establishes PROVEN. Native extensions, the C runtime, and the
  system compiler remain trusted components rather than formally verified code.

Read [benchmarks/README.md](benchmarks/README.md) before changing evaluation.
Retain raw timings and environment details, include failed attempts in cost, and
report acceptance separately. Missing billing or independent evaluation evidence
must remain UNKNOWN. A scripted patch benchmark is not an agent benchmark, and
the 25% cost-improvement objective is a gate, not a promised result. Do not claim
production readiness or agent-efficiency gains without the required release evidence.

## Validate your change

Run focused compiler tests while iterating, then run these checks before handing
off a change or opening a pull request:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
python3 -m unittest discover -s benchmarks -p 'test_*.py'
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
```

`make verify` runs these checks and additional example tests. To compare native
and reference behavior on a supported example directly:

```sh
cargo run --locked -- test examples/collections.keel --engine both
```

See [validation](docs/validation.md) for coverage and limitations. Record failures,
skipped checks, and platform restrictions explicitly; a local pass does not
establish results on every CI platform. CI has no live inference calls. Paid
agent evaluations, global installs, authentication changes, and releases require
explicit authorization when working through an agent.

## Report a problem

When you discover a missing capability, bug, or inefficiency, search existing
issues first and add evidence to a matching issue instead of creating a duplicate.
Otherwise, [open an issue](https://github.com/jakecyr/keel/issues/new) with:

- A specific title, affected components, and impact on users or development.
- A minimal Keel example where possible, exact reproduction commands, expected
  behavior, actual behavior, and relevant diagnostics.
- The commit (`git rev-parse HEAD`) or Keel version, OS/architecture, and relevant
  Rust and C compiler versions.
- For language gaps, the use case, current workaround, and desired behavior.
  For inefficiencies, a representative workload and measurements when available;
  distinguish suspected bottlenecks from measured results.
- Links to relevant source, tests, documentation, or related issues, plus concrete
  acceptance criteria where possible.

If you cannot file an issue, include a ready-to-file title and body in your handoff
and state that it was not filed. Remove credentials and private data from examples
and logs before sharing them.

## Open a pull request

Keep the change focused and link any related issue. Explain the problem, the
resulting behavior, and the tests or examples that demonstrate it. Include the
validation commands you ran and their outcomes, along with remaining limitations
or follow-up work. For language changes, describe the semantic choices and updated
documentation; for measurements, link the raw evidence and methodology.

Review your diff for unrelated changes, generated build output, and secrets before
submitting. Keep release publishing separate from an ordinary contribution.
