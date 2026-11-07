# Hello Keel

This starter was generated with `cargo run --locked -- init examples/hello`.
Its generated files are the project template; this README describes validation.

With Keel installed, from this directory run:

```sh
keel fmt --check
keel lint --deny-warnings
keel check
keel test --engine both
keel build -o build/hello
keel run --allow-stdout
```

The program prints `Hello from Keel!`. Edit `src/main.keel`; add independent tests
in `tests/acceptance.keel`. To generate your own project, run `keel init my-app`.
Runtime permissions are denied by default; printing requires `--allow-stdout`.
