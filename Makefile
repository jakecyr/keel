.PHONY: build test check demo clean

build:
	cargo build --release --locked

test:
	cargo test --locked
	cargo run --locked -- test examples/web_server.keel --cases 1000 --seed 42
	cargo run --locked -- test examples/ownership.keel

check:
	cargo fmt --check
	cargo clippy --locked --all-targets -- -D warnings

demo: build
	./target/release/keel run examples/web_server.keel --allow-net=127.0.0.1:8080

clean:
	cargo clean
