<!-- keel:agent-guide:start -->
## Keel development

Before editing Keel code, run `keel agent context . --json`. For a focused change use
`keel agent context . --symbol FUNCTION --json`. The installed compiler provides
the supported-language reference: `keel agent spec language`,
`keel agent spec collections`, `keel agent spec stdlib`, `keel agent spec protocol`, and `keel api BUILTIN --json`.
Use `keel agent commands --json` to discover tools.

Use current revisions for `keel edit . --request edit.json --json`. Preserve
approved contracts, assertions, helper oracles, expected results, and generator domains.
Validate with `keel fmt . --check`, `keel check . --json`, `keel lint . --json`,
and `keel test . --engine both --json`. TESTED is sampled evidence; UNKNOWN,
BLOCKED, and INCOMPLETE are not success. Ask the user before expanding runtime
authority in keel.policy.json. Ordinary source edits and Git review still work.
<!-- keel:agent-guide:end -->
