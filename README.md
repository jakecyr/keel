<p>
  <img src="docs/assets/keel-logo.svg" alt="Keel — retro pixel K icon and wordmark" width="380" height="128">
</p>

# Keel

An experimental language for readable code, native programs, and coding agents.

<!-- efficiency-banner:start -->
> **Measured progress: 50.8% fewer agent tokens** with compact context and combined validation (six-task Keel workflow experiment). Reserved-task validation: **61.8% fewer tokens**, **2/4 accepted** versus 0/4 for the Keel protocol baseline.
>
> **Local compiler feedback:** CLI checks were **4.1× faster than C** and **15.2× faster than Rust** on the recorded synthetic ~10,000-line fixture. A cross-language agent-token or dollar-cost advantage is **not established**. [Workflow evidence](benchmarks/results/efficiency.md) · [Compiler methodology](benchmarks/results/README.md)
<!-- efficiency-banner:end -->

## 1. Install

On macOS or Linux:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/jakecyr/keel/master/scripts/install.sh | bash
export PATH="$HOME/.local/bin:$PATH"
keel doctor
```

You'll also need a C compiler: run `xcode-select --install` on macOS or `sudo apt-get install build-essential` on Debian/Ubuntu. `keel doctor` checks that you're ready.

Add the `export PATH` line to `~/.zshrc` or `~/.bashrc` to keep Keel available in new terminals. See the [installation guide](docs/installation.md) for supported platforms, upgrades, and source builds, or [review the installer](scripts/install.sh).

## 2. Create your project

```sh
keel init hello-keel
cd hello-keel
keel test --engine both
keel run --allow-stdout
```

You'll see `Hello from Keel!`. Open **`src/main.keel`** to start writing; your tests live in `tests/acceptance.keel`. The `--allow-stdout` flag lets your program print.

Already in a project folder? Run `keel init` there. It preserves existing source files and human-written agent instructions.

## 3. Start your coding agent

Open your new project in your coding agent, then paste:

```text
Read AGENTS.md and run `keel agent context . --json` before editing.
Use Keel's built-in language guides rather than guessing the syntax.
Keep approved acceptance tests independent of the implementation.
After changes, format, check, lint, and run `keel test . --engine both`.
Report any failures or incomplete results.
```

Then tell it what you'd like to build. Keel creates `AGENTS.md` and `CLAUDE.md` for you and includes offline language guides. No Keel-specific plugin is needed—just an agent that can read files and run terminal commands.

## Terminal output

Keel uses a retro terminal theme with colored status markers, source-linked errors
and warnings, and an animated activity indicator for builds, linting, tests, and
other project commands. The indicator shows the current phase and elapsed time;
short operations finish without flicker. `keel run` clears it before starting your
program, preserving the program's own output.

Color is automatic on terminals. Piped output, CI, and `TERM=dumb` use plain text;
`--json` and `keel serve` retain their machine-readable output without decoration.

- `KEEL_COLOR=always` forces color (including redirected output); `KEEL_COLOR=never`
  disables it. The default is `auto`.
- A nonempty `NO_COLOR` or `TERM=dumb` overrides forced color.
- `KEEL_PROGRESS=off` disables animation independently of color. Animation requires
  both stdout and stderr to be terminals and is disabled in CI.

```sh
KEEL_PROGRESS=off keel build
NO_COLOR=1 keel lint
keel check --json
```

## Keep going

- [Keel website](https://jakecyr.github.io/keel/) — explore the agent workflow and recorded benchmarks
- [Language tour](LANGUAGE.md) — learn the syntax with examples
- [Development workflows](docs/workflows.md) — build, test, use agent tools, and try the web-server example
- [Installation guide](docs/installation.md) — platform support, upgrades, and editor setup
- [Contributing](CONTRIBUTING.md) — work on Keel itself
- [Release gaps](docs/release-gaps.md) — what's supported and what's still experimental
- [Icon and design](docs/design.md) — the retro visual identity
