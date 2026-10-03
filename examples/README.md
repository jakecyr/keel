# Keel examples

These examples exercise the experimental native compiler and trusted runtime.
Start with the README in each directory for build commands and scoped permissions.

| Example | What it demonstrates |
| --- | --- |
| [Catalog API](catalog_api/README.md) | Request headers/query validation, bounded JSON arrays, checked integer discounts, and JSON replacement. |
| [HTTP app](http_app/README.md) | A native Keel server serving HTML, CSS, JavaScript, binary assets, and JSON API endpoints on one origin. |
| [Evolving arena](evolving_arena/README.md) | Two players propose new Keel ability code, validate it, and activate it between rounds; native game/server/worker orchestration with offline and OpenAI modes. |
| [Jev Pong](jev_pong/README.md) | Native Pong physics, HTTP hosting, and two independently stateful Jev decision workers; an offline mode needs no credentials. |
| [Jev request](jev/README.md) | A single structured decision request using Keel's JSON, configuration, and HTTP APIs. |
| [Web routes](web/) | The original pure GET/path-only `http.serve` example. |
| [Standard library](stdlib.keel) | Offline parsing, collections, and scoped parallel-map examples. |

Game browser code renders state and sends controls; the application servers and
workers are Keel. Shell launchers build and start executables. Python game tests
are offline fixtures, not runtime servers. Live model modes consume API usage
and are explicitly selected; no CI test makes a live inference call.

Native/reference test agreement is sampled evidence, not formal proof or an
agent-efficiency benchmark. See [release gaps](../docs/release-gaps.md).

The public CLI regression `all_application_examples_check_test_both_engines_and_build_offline`
checks/builds every application project and worker manifest and tests application
logic in both engines. `holes.keel` intentionally reports BLOCKED and
`counterexample.keel` intentionally reports FAILED; they teach diagnostics and
must not be changed to pass. Long-running hosts and live integrations have separate
offline socket/process fixtures; building them does not run a paid service.
