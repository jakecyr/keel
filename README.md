<p>
  <img src="docs/assets/keel-logo.svg" alt="Keel — retro pixel K icon and wordmark" width="380" height="128">
</p>

# Keel

An experimental language for readable code, native programs, and coding agents.

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

## Keep going

- [Keel website](https://jakecyr.github.io/keel/) — explore the agent workflow and recorded benchmarks
- [Language tour](LANGUAGE.md) — learn the syntax with examples
- [Development workflows](docs/workflows.md) — build, test, use agent tools, and try the web-server example
- [Installation guide](docs/installation.md) — platform support, upgrades, and editor setup
- [Contributing](AGENTS.md) — work on Keel itself
- [Release gaps](docs/release-gaps.md) — what's supported and what's still experimental
- [Icon and design](docs/design.md) — the retro visual identity
