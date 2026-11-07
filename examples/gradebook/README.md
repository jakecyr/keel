# Gradebook

Validates integer percentage scores and prints their truncated average and the number meeting a threshold. The average accepts 1–10,000 scores in 0–100.

From the repository root:

```sh
cargo run --locked -- test examples/gradebook --engine both
cargo run --locked -- run examples/gradebook --allow-stdout
```

Expected output:

```text
90
2
```
