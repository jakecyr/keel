# Keel for VS Code

Syntax highlighting for the experimental Keel language. Opening a `.keel` file
selects the **Keel** language mode automatically.

Highlights functions, control flow, ownership modes, contracts, effects, built-in
types and calls, match variants, integers, strings, escapes, and `//` comments.
Also provides comment toggling, bracket matching, and bracket/quote pairing.
Strings can span lines, as supported by the compiler. Only Keel's supported
escapes (`\n`, `\r`, `\t`, `\"`, and `\\`) receive escape highlighting;
unsupported escapes are marked invalid.

This is a declarative TextMate extension with no runtime dependencies. It does
not provide compiler diagnostics, completion, formatting, or a language server.
Colors follow the selected VS Code theme.

## Preview from this checkout

Open this directory in VS Code and press **F5**:

```sh
code editors/vscode
```

The included launch configuration opens the repository's examples in an
Extension Development Host. Open any `.keel` example. Use **Developer: Inspect
Editor Tokens and Scopes** from the Command Palette to inspect highlighting.
No build or npm install is required for this preview.

## Package and install locally

With Node.js 22 or newer, npm, and the VS Code `code` command available, run:

```sh
cd editors/vscode
npx @vscode/vsce package
code --install-extension keel-language-0.1.0.vsix
```

Alternatively, select **Extensions: Install from VSIX...** in VS Code and choose
the generated file. Reload VS Code if needed. Packaging creates a local artifact;
the extension has not been published to the Marketplace.

## Maintaining the grammar

Keep `syntaxes/keel.tmLanguage.json` aligned with `src/syntax.rs`, `src/check.rs`,
and the embedded references (`keel agent spec language` and
`keel agent spec collections`). Preview the examples after grammar edits,
including multiline strings, escaped quotes, comments containing keywords, and
contracts using `result` alongside calls to `result.ok` and `result.err`.

See VS Code's [syntax highlighting guide](https://code.visualstudio.com/api/language-extensions/syntax-highlight-guide)
and [language configuration guide](https://code.visualstudio.com/api/language-extensions/language-configuration-guide).
