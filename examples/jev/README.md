# Query Jev from Keel

This example sends one request containing Choice, Noul, and Score questions,
logs selected response fields, and branches on the decoded Choice string.
It uses the [TypeSafe HTTP API](https://docs.typesafe.ai/api):
`POST https://api.typesafe.ai/v1/systemone`, bearer authentication, `jev-latest`,
and answers keyed by the question names. The input is synthetic support text.

From this directory, with the repository development compiler:

```sh
# .env already exists locally; otherwise copy .env.example and fill it in.
cargo run --locked --manifest-path ../../Cargo.toml -- test . --engine both
cargo run --locked --manifest-path ../../Cargo.toml -- run . --policy keel.policy.json -o ../../build/jev
```

With an installed compiler: `keel test . --engine both`, then
`keel run . --policy keel.policy.json`. HTTP support needs libcurl and
`pkg-config`; see [the standard-library guide](../../docs/stdlib.md).
Run from this directory because file permissions and reads use relative paths.
The policy grants only stdout, `.env`, `request.json`, and the TypeSafe origin.
The live run consumes API usage; offline tests never read credentials or call Jev.

Put `JEV_API_KEY=...` in `.env`. The parser reads literal assignments without
executing shell commands. The key stays out of source, command arguments, and
logs. `.env`, builds, and local logs are excluded from Git/Cargo packaging.
Do not place credentials in `request.json` or the checked-in sample output.

The central operations are:

```text
http.post_json(url, request, key)
json.text(body, "/answers/department/choice")
json.get(body, "/answers/refund_requested/noul")
```

Each returns `Result<Text, Text>` and requires both match arms. `json.text`
decodes strings; `json.get` preserves JSON numbers as text. Keel currently lacks
floating-point arithmetic, so this example logs probabilities rather than
applying numeric thresholds. Non-200 responses and malformed/missing fields are
handled without blindly indexing or printing arbitrary error response bodies.
There are no automatic retries. Review the provider's rate-limit guidance before
adding retries to an application.

`sample-output.txt` records the successful development smoke call, not an expected
model-answer assertion. `tests.keel` uses independent fixed response fixtures;
model output can change. This example is not an agent benchmark.
