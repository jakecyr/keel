# Static files and JSON endpoints in Keel

From the repository root:

```sh
cd examples/http_app
cargo run --locked --manifest-path ../../Cargo.toml -- test . --engine both
cargo run --locked --manifest-path ../../Cargo.toml -- run . --policy keel.policy.json
```

Open http://127.0.0.1:8090. HTML, CSS, JavaScript, and the JSON API all come from
the native Keel process. No Python, Node, or third-party package is required.

`http.serve_app(8090, "public", route)` calls
`route(method: read Text, path: read Text, body: read Text) -> Text` for each
request. A 404 response on GET/HEAD falls back to `public`, with directory
`index.html` and extension-based MIME types. Binary assets are sent directly by
the host and never converted into Keel Text. Other methods do not serve files.
Use `http.json_response` for JSON responses. An empty static root disables files.
Handler effects must also be declared by the function calling `http.serve_app`.

```sh
curl http://127.0.0.1:8090/api/health
curl -H 'Content-Type: application/json' -d '{"hello":"world"}' http://127.0.0.1:8090/api/echo
curl -I http://127.0.0.1:8090/style.css
```

The launcher grants the exact static root with `read: ["public"]` or
`--allow-read=public`, in addition to the listen permission. Paths are relative to
the process working directory. Only put public assets in that root. Traversal,
dot files, symlinks below the root, and nonregular files are rejected. The root
itself cannot be a symlink; its parent directories remain trusted.

This is an experimental sequential localhost host. Headers are limited to 16 KiB,
UTF-8 request bodies to 1 MiB, and static files to 8 MiB. Request reception and
response transmission each have a two-second I/O deadline. Handler execution is
not deadline-limited. Requests use Content-Length; chunked transfer, streaming,
multipart uploads, keep-alive, TLS, headers/query access, and WebSockets are not
implemented. The handler receives the path before `?`, without percent decoding.
The static host decodes percent escapes and rejects unsafe paths.
