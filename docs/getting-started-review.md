# Developer and agent workflow review

The installed binary should be sufficient to discover supported syntax and
commands. Agents should not need this repository's source paths, a model-specific
plugin, or an internet search to learn the installed language version.

## The path a new developer should follow

From a source checkout, install with `cargo install --path . --locked`; ensure
Cargo's reported binary directory is on PATH. Source installation needs Rust and
Cargo. Native Keel builds additionally need a supported POSIX host and a
GCC/Clang-compatible C compiler. `keel doctor --json` checks that compiler; a
working static checker alone does not establish that native builds can run.

```sh
keel --version
keel doctor --json
keel init hello
cd hello
keel agent context . --json
keel fmt . --check
keel check . --json
keel lint . --json
keel test . --engine both --json
keel run . --allow-stdout
```

The explicit `cd hello` matters: the initialization result's project directory is
the working directory for subsequent `.` commands. Running `keel init .` inside
an existing directory is supported. Initialization must preserve unrelated files
and human instructions and must never replace an existing entry source or
acceptance source to make a scaffold fit.

The scaffold's runtime policy denies authority by default. The greeting example
needs the explicit stdout grant above; do not imply that `effects { io.stdout }`
or `keel run . --policy keel.policy.json` automatically grants it. Inspect or
change deployment policy only within the user's authorization.

## Agent bootstrap and task context

`AGENTS.md` and `CLAUDE.md` should contain concise managed guidance pointing to
the installed compiler, with existing human content outside the managed markers
preserved byte-for-byte. Reinitialization updates that one block and leaves
source, tests, manifest, and policy unchanged. Duplicate, reversed, incomplete,
or nested managed-marker pairs are ambiguous and should be rejected before
changing either guidance file.

```sh
keel agent commands --json
keel agent spec language
keel agent spec collections
keel agent spec protocol
keel api list.get --json
keel agent context . --symbol greet --max-chars 4000 --json
```

The reference topics are embedded in the binary and include its version in JSON
responses. They describe supported features, including restrictions; the original
aspirational design is not an API reference. The protocol schema version and
compiler package version serve different purposes and should remain explicit.

Without a project argument, `keel agent context --json` returns bootstrap
instructions. It does not infer that the current directory is a project. Include
`.` deliberately when project source is wanted.

`--symbol` selects one implementation. `--max-chars` limits implementation
snippets, measured as Unicode characters, not the entire response. Language
reference, command catalog, signatures, callers, project metadata, and test names
are additional material. The returned `incomplete` flag reports source
truncation. A caller seeking a strict total token budget must budget these extra
fields separately; there is no total-response token-budget guarantee today.

Use the revision inside `context` for structural edits. Truncating the displayed
source does not change the revision of the underlying complete project. Read the
complete target before proposing edits. Acceptance source and failing case
evidence may still require separate file reads and test commands; the context
command does not automatically run tests or infer approved requirements.

Errors remain useful during repair: a nonzero exit and the installed language
guide accompany static errors and project-load failures. Static diagnostics have
source locations; project-load errors identify the offending file in their
message. Context output without `--json` also retains these details instead of
collapsing into an unexplained `FAILED` line.

## Verification added in this review

`tests/agent_workflow.rs` drives the public installed-style executable in isolated
temporary directories. It covers:

- Offline command discovery, exact built-in lookup, versioned reference topics,
  and matching plain-text reference output, with PATH empty and CC unavailable.
- Scaffold initialization, both generated guidance files, focused context,
  formatting, static checks, lint, native/reference tests, native build, and run.
- `init .`, existing human guidance and ignore rules, idempotent managed updates,
  and preservation of source, tests, policy, manifest, and unrelated files.
- Conflicting source and ambiguous managed markers rejected before partial writes.
- Unicode-safe context truncation, revision-bound edit using context output,
  approved test execution, and static/project-load diagnostics alongside the guide
  in both JSON and default context output.

Empty PATH demonstrates that documentation and static context do not invoke an
external compiler or repository tooling. It is not a network sandbox or a formal
proof that all possible command paths are offline.

The repository includes an offline-tested release-download installer and CI
archive/checksum packaging. Downloads still require published release assets;
this implementation did not publish them. Future distribution work includes
signed release artifacts, broader platform validation, rollback instructions,
protocol compatibility tests across versions, and editor integrations. The
current source installation and local agent CLI can be useful before those
release criteria are complete.
