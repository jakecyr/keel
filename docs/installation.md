# Installing Keel

[Back to the quick start](../README.md#1-install)

Download a prebuilt compiler—**no Git clone or Rust installation required**:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/jakecyr/keel/master/scripts/install.sh | bash
export PATH="$HOME/.local/bin:$PATH"
keel --version
keel doctor
```

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

The [Keel language extension](../editors/vscode/README.md) adds highlighting for
`.keel` files, comment toggling, and bracket/quote pairing. Open `editors/vscode`
in VS Code and press **F5** to preview it, or follow its README to package and
install it locally.
