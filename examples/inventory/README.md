# Inventory

Uses product IDs as list indices, records sales and restocks, and rejects invalid transactions without mutation. Restocks cap each quantity at 1,000,000.

From the repository root:

```sh
cargo run --locked -- test examples/inventory --engine both
cargo run --locked -- run examples/inventory --allow-stdout
```

Expected output:

```text
Sale recorded
Stock received
3
2
10
```
