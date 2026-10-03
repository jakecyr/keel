# Developing Keel

Keel is an experimental native language, implemented by a Rust compiler and a trusted C runtime. Do not describe it as production-ready or claim agent-efficiency gains without the release evidence in docs/release-gaps.md.

## Commands

Build/install the development toolchain with `cargo install --path . --locked`. From source without installing, use `cargo run --locked -- <arguments>`. Start language work with `keel agent context --json`; read exact supported syntax via `keel agent spec language`, `keel agent spec collections`, and `keel api BUILTIN --json`.

Before handing off changes, run the relevant compiler tests and `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, and `cargo test --locked --all-targets`. Benchmark methodology tests run with `python3 -m unittest discover -s benchmarks -p 'test_*.py'`. Installer tests run with `python3 -m unittest discover -s scripts/tests -p 'test_*.py'`.

## Language implementation invariants

Root `LANGUAGE.md` is the developer/agent syntax tour; `docs/language.md` is the detailed reference. Keep both aligned with embedded guides. Runnable `keel` code fences in the root tour are checked by both native and reference engines in the test suite.

- Preserve defined left-to-right evaluation, checked integer semantics, exhaustive handling, and ownership rules in both native and reference engines.
- Add negative compiler tests, native tests, and differential/sanitizer tests for ownership, runtime, or lowering changes.
- Static success, TESTED, ENFORCED, BLOCKED, UNKNOWN, and PROVEN are distinct. No tool currently establishes PROVEN.
- Acceptance assertions, specification helpers, generator domains, and expected outputs are independent of implementations. Do not weaken them to pass a failing change.
- Native extensions, the C runtime, and the system compiler are trusted components, not formally verified code.
- Keep documents embedded by src/agent.rs aligned with actual supported features. Do not teach proposed syntax as implemented syntax.
- Do not publish releases, install globally, change authentication, or invoke paid agent evaluations unless the user's task authorizes it. CI has no live inference calls.

## Benchmark honesty

Read benchmarks/README.md before changing evaluation. Record raw timings/environment, include failed attempts in cost, compare acceptance separately, and preserve UNKNOWN for absent billing or independent evaluation evidence. A scripted patch benchmark is not an agent benchmark. The 25% cost-improvement target is a gate, not a promised result.

## Reporting gaps, bugs, and inefficiencies

Create a repository issue when you discover a missing language capability, bug, or inefficiency. Search existing issues first; add new evidence to a matching issue instead of creating a duplicate.

- Use a specific title and describe the problem, affected components, and impact on users or development workflows.
- Include a minimal Keel example where possible, exact reproduction commands, expected versus actual behavior, and relevant diagnostics. Record the commit or toolchain version and environment needed to reproduce the problem.
- For language gaps, explain the use case, current workaround, and desired behavior. Clearly label proposed syntax as unimplemented.
- For inefficiencies, include a representative workload and measurements where available; distinguish suspected bottlenecks from measured results and follow the benchmark honesty rules above.
- Link relevant source files, tests, documentation, or related issues, and suggest concrete acceptance criteria where possible.

If issue creation is unavailable, provide a ready-to-file issue title and body in the handoff and state that it was not filed.
