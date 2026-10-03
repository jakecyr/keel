# Agent protocol

The installed CLI carries its documentation, so agents do not need repository checkout paths or web access:

```sh
keel agent commands --json
keel agent context --json
keel agent context . --symbol route --json
keel agent spec language
keel agent spec collections
keel agent spec protocol
keel api list.get --json
```

`agent context` combines the compact language reference, command catalog, optional project metadata, and a bounded `inspect` bundle. Without a project argument it returns bootstrap instructions. `--max-chars N` controls implementation snippets; metadata and the reference are additional. `incomplete` indicates source truncation. Test-file protection, holes, and unsupported language features are explicit. These documents are versioned with the binary; `keel --version` identifies the toolchain.

`agent spec` is read-only. Topics `language`, `collections`, and `protocol` select the embedded references. `agent commands` provides machine-readable command purposes. There is no model provider dependency.

## Persistent service

Run `keel serve --max-cache-mib 64`. Send one JSON object per line; stdout returns one JSON response per line. Diagnostics and normal results are under `result`; invalid protocol requests use `error`. IDs are echoed. Example requests:

```json
{"id":1,"method":"check","path":"."}
{"id":2,"method":"inspect","path":".","args":["--symbol","route","--max-chars","4000"]}
{"id":3,"method":"test","path":".","args":["--cases","1000","--seed","42","--engine","both"]}
{"id":4,"method":"check","source":"fn main() {}"}
{"id":5,"method":"stats"}
{"id":6,"method":"shutdown"}
```

Methods are `check`, `inspect`, `test`, `lint`, `format`, `stats`, and `shutdown`. Provide either `path` or `source`, never both. A path may be a file, a project directory, or `keel.json`. `args` contains the corresponding CLI options. Service `format` returns formatted source without writing. Structural writes use the revision-bound CLI edit operation.

The service caches exact-source snapshots; text edits are cache misses. Cache byte accounting is conservative estimation, not an OS memory reservation. `stats` reports hits, misses, estimated bytes, and entry count. Inputs are limited to 4 MiB of source and 8 MiB per JSON line. Tests execute with per-worker and suite time budgets; Linux workers also enforce the configured address-space limit. Platform limitations are reported.

## Project initialization

`keel init PATH` creates a CLI project with `keel.json`, `src/main.keel`, `tests/acceptance.keel`, a deny-by-default runtime policy, and guidance in `AGENTS.md` and `CLAUDE.md`. It supports an existing directory if the generated source/manifest paths are unused. Existing human instructions are preserved; only a clearly marked Keel-managed block is added or updated. Re-running init on an existing Keel project refreshes that block without rewriting source or tests.

The generated guidance directs agents to `keel agent context . --json` first. It does not embed a long, stale duplicate specification or require a vendor-specific plugin. The installed compiler remains the authoritative supported-language reference.
