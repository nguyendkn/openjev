.PHONY: build test bench serve fmt clippy ci server-deploy

build:
	cargo build --workspace

test:
	cargo test --workspace

bench:
	cargo run -p cli --

serve:
	cargo run -p server --

fmt:
	cargo fmt --all

clippy:
	cargo clippy --workspace -- -D warnings

ci: fmt-check clippy test

fmt-check:
	cargo fmt --all -- --check

server-deploy:
	@echo "Phase 7 deployment - to be implemented"
