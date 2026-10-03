.PHONY: build install test check verify bench demo clean

build:
	cargo build --release --locked

install:
	cargo install --path . --locked

test:
	cargo test --locked --all-targets
	python3 -m unittest discover -s benchmarks -p 'test_*.py'
	python3 -m unittest discover -s scripts/tests -p 'test_*.py'
	cargo run --locked -- test examples/web_server.keel --cases 1000 --seed 42
	cargo run --locked -- test examples/ownership.keel
	cargo run --locked -- test examples/web --engine both --cases 1000 --seed 42
	cargo run --locked -- test examples/collections.keel --engine both

check:
	cargo fmt --check
	cargo clippy --locked --all-targets -- -D warnings

verify: check test

bench: build
	python3 benchmarks/measure.py --output build/benchmarks/local.json

demo: build
	./target/release/keel run examples/web --policy examples/web/keel.policy.json

clean:
	cargo clean
