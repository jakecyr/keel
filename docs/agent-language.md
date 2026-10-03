# Keel agent language reference

For a repository-based syntax tour, read root `LANGUAGE.md`; its runnable examples are regression-tested. Project commands such as `keel check`, `keel test`, and `keel build` default to the current directory. Use `keel COMMAND --help` for focused options and examples. `keel agent context` without a path deliberately returns only bootstrap guidance; include `.` to inspect the project.

This guide is embedded in the installed compiler. It describes supported syntax, not future design promises. Start with `keel agent context . --json` and request a symbol with `--symbol NAME` to keep implementation context small. Use `keel agent spec collections` or `keel api BUILTIN --json` for exact interfaces.

## Workflow

1. Read the project's AGENTS.md/CLAUDE.md and approved acceptance criteria.
2. Run `keel agent context . --symbol NAME --json`; inspect the returned revision, source, dependencies, contracts, and permissions.
3. Make a small implementation edit. Prefer `keel edit . --request edit.json --json` with the current revision. Ordinary text edits are also supported.
4. Run `keel fmt . --check`, `keel check . --json`, `keel lint . --json`, and `keel test . --json`.
5. For semantic changes, use `keel test . --engine both --json` to compare native execution with the bounded reference evaluator. Review the diff and report remaining uncertainty.

Never weaken acceptance assertions, specification helpers, expected outcomes, or generator domains to obtain a pass. Manifest `tests` files are protected from structural edits, including helper functions. Filesystem access is not sandboxed by that policy. Holes, permission failures, timeouts, and incomplete evidence are not successful tests.

## Syntax

```text
pub fn square(value: Int) -> Int
    requires value >= -1000 && value <= 1000
    ensures result >= 0
{
    return value * value
}

test "square" { assert square(12) == 144 }
property "nonnegative" (n in gen.int(min: -1000, max: 1000)) {
    assert square(n) >= 0
}
```

Functions have explicitly typed parameters; an omitted return type means Unit. Blocks use braces. `let` is immutable; `var` allows assignment. `if`/`else`, `while`, `for value in list`, `return`, `assert`, and exhaustive `match` are supported. No shadowing. `result` is reserved for postconditions: use a name such as `output` for local variables and parameters, even in functions without contracts. Newlines are whitespace; optional semicolons can disambiguate. `//` comments are preserved by formatting and edits outside the replaced body.

Supported types: `Int` (signed 64-bit), `Bool`, `Text`, `List<Int>`, `Option<Int>`, `Result<Int, Text>`, and `Unit`. Lists/options/results are specific built-in types, not arbitrary generics. There are no user-defined records/unions, imports, closures, async functions, or automatic package installation yet. Project `keel.json` explicitly composes complete files into one namespace; it is not a module system.

Arithmetic overflow and invalid indexing trap consistently. Evaluation is left to right; `&&`/`||` short-circuit. No implicit numeric/text conversions. `text.len` counts UTF-8 bytes. Use `text.from_int`, `text.concat`, and explicit `text.clone`.

## Ownership and errors

Owned Text/List/Result parameters require `read`, `edit`, or `take`. `read` borrows for a call. `edit` borrows a mutable binding exclusively and must be explicit at the call site. `take` transfers ownership and invalidates the old binding. Returning an owned local transfers it. To retain two owners, explicitly clone. Borrowed values cannot escape or be returned as owned.

```text
fn append(values: edit List<Int>, value: Int) { list.push(edit values, value) }
fn length(values: read List<Int>) -> Int { return list.len(values) }
fn identity(values: take List<Int>) -> List<Int> { return values }

fn parsed_or_zero(raw: read Text) -> Int {
    match text.parse_int(raw) {
        Ok(value) => { return value }
        Err(message) => { assert text.len(message) > 0 return 0 }
    }
}
```

List literals are `[1, 2]` or `[]`. `list.at` traps on invalid indices; `list.get` returns `Option<Int>` and must be matched with `Some(value)` / `None`. Matches require both variants. Error Text payloads are borrowed inside their match arm; clone to retain them.

Read/edit argument loans last through later argument evaluation. Compute `let n = list.len(values)` before `list.push(edit values, n)`; nested reads during an exclusive argument borrow are rejected. A for-loop holds a read borrow on its input. Restore moved outer values on every loop iteration.

## Effects, contracts, evidence

Functions are externally pure unless declaring `effects { io.stdout }` or `effects { net.listen }`. Effects propagate through calls. Runtime authority is separate: use a launcher-controlled policy with `keel run . --policy keel.policy.json` or explicit CLI permission flags. Do not broaden policy without the user's authorization.

`requires` and `ensures` are pure Boolean runtime checks; `result` denotes the return value. They are ENFORCED when executed, never PROVEN. Specifications should be independent of the implementation being tested. Invalid external input should use typed errors where available.

`hole("name")` needs an expected type (return position, annotated binding, or typed argument). Inspect returns its expected type/bindings/effects. `check` returns INCOMPLETE; builds reject holes; reached holes return BLOCKED.

Tests return TESTED, FAILED, BLOCKED, or UNKNOWN. TESTED means recorded cases passed, not universal correctness. Reference/native parity is additional sampled evidence, not a soundness proof. Replay using the returned revision and `keel test . --filter 'exact property name' --value N --json`. Timeouts and resource limits must be reported, not hidden by retries.

## Structural edit request

```json
{
  "base_revision": "COPY_FROM_CONTEXT",
  "target": "fn:square",
  "operation": "replace_body",
  "source": "{ return value * value }",
  "run": "affected_checks_and_tests"
}
```

Edits preserve interfaces, contracts, and acceptance files. The run mode above checks the entire candidate and conservatively runs all tests. Multiple body replacements can be supplied in an `edits` array; all targets must share one physical file for an atomic transaction. Stale revisions are rejected; retrieve fresh context and reconsider the change instead of blindly retrying.

`keel serve` exposes a local JSON-lines protocol for repeated check/inspect/test/lint/format requests. It caches whole-source snapshots with an estimated budget, not declaration-level incrementality. See `keel agent spec protocol`. No installed tool reaches the internet to resolve an unknown API.
