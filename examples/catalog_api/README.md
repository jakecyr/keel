# Catalog transformation API

An offline localhost API in Keel: validate a JSON array of products, apply an
integer percentage discount, and preserve unrelated fields and number spelling.
It demonstrates borrowed request metadata, recoverable validation, array traversal,
and JSON Pointer replacement without imports or new language syntax.

From the repository root:

```sh
cargo run --locked -- test examples/catalog_api --engine both
cargo run --locked -- build examples/catalog_api -o build/catalog-api
./build/catalog-api --allow-net=127.0.0.1:8092
```

In another terminal:

```sh
curl --fail-with-body http://127.0.0.1:8092/api/health
curl --fail-with-body 'http://127.0.0.1:8092/api/discount?percent=10' \
  -H 'Content-Type: application/json' \
  --data '[{"name":"tea","price_cents":199,"score":0.9900}]'
```

The discount response is
`[{"name":"tea","price_cents":180,"score":0.9900}]`.
The discount amount rounds down to integer cents. Percent must be 0–100; each
price must be an integer from 0 to 1,000,000,000 cents; a batch has at most 100
products. The price bound prevents intermediate integer overflow. Missing fields,
fractional prices, malformed JSON, and invalid percentages return 422. Missing,
malformed, or repeated percent parameters return 400. The example requires exactly
one Content-Type with value `application/json`; it does not infer media types or
accept parameters. Unknown routes return 404 and wrong methods return 405.

`http.serve_api` supplies `(method, target, headers, body)` as nonescaping read
borrows. `http.path` separates routing from the query; `http.query` decodes form
values and rejects duplicate matches. `http.header` performs ASCII case-insensitive
lookup and rejects repeated matching headers. `json.array_len` and `json.set`
return typed errors; `json.set` only replaces an existing value. JSON strings
used as replacement values must first be escaped with `json.quote`.

This example has no database, authentication, persistent state, or live API calls.
It uses the sequential localhost host with its documented framing/size/deadline
limits. It is not a production commerce service. See the
[server design review](../../docs/server-readiness.md) and
[standard library](../../docs/stdlib.md). The Rust suite runs the actual routes
over fragmented TCP requests under address/undefined-behavior sanitizers.
