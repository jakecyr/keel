# Keel 0.1.0 — experimental

First downloadable release of the implemented language subset, not a
production-ready implementation of the entire Keel design.

## Install without cloning or Rust

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/jakecyr/keel/master/scripts/install.sh | bash
export PATH="$HOME/.local/bin:$PATH"
keel doctor
keel init hello-keel
cd hello-keel
keel test --engine both
keel run --allow-stdout
```

The installer verifies SHA-256, installs into `~/.local/bin` without sudo, and
leaves shell profiles unchanged. Inspect the script before running it. Re-run to
upgrade; use `bash -s -- --version v0.1.0` to select this release.

Native program builds need a C compiler. Binaries target Linux x86_64 (Ubuntu
22.04/glibc 2.35 or newer), Linux ARM64 (Ubuntu 24.04/glibc 2.39 or newer), and
macOS 15 or newer on Intel/Apple Silicon. Windows and Alpine/musl are unsupported.
macOS binaries are not Apple-notarized. Checksums protect download integrity;
they are not independent publisher signatures.

Includes build/run/check/fmt/lint/test/init, offline agent context and references,
revision-bound edits, a native/reference test runner, project scaffolding and
agent guidance. See LANGUAGE.md and docs/release-gaps.md in the repository for
exact semantics and restrictions. The HTTP adapter is a localhost demonstration.

No 25% agent-cost advantage has been established; retained pilot evidence is in
benchmarks/results/. Large original-design features remain unimplemented.
