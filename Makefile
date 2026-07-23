# Developer convenience targets. `make setup` is the one-time step that activates
# the git hooks; the rest mirror the CI jobs in .github/workflows/ci.yml so you
# can reproduce any CI failure locally. Run `make` (or `make help`) for the list.

.DEFAULT_GOAL := help
.PHONY: help setup fmt fmt-check lint test bench ci py-dev

help: ## Show this help
	@grep -hE '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-10s\033[0m %s\n", $$1, $$2}'

setup: ## One-time: point git at the tracked hooks in .githooks/
	git config core.hooksPath .githooks

fmt: ## Format the workspace in place
	cargo fmt --all

fmt-check: ## Check formatting without writing (CI fmt job)
	cargo fmt --all --check

lint: ## Clippy across all targets, warnings denied (CI clippy job)
	cargo clippy --workspace --all-targets -- -D warnings

test: ## Run the test suite with nextest (CI test job)
	cargo nextest run --workspace

bench: ## Build the benchmarks without running them (CI bench step)
	cargo bench --workspace --no-run

py-dev: ## Build the Python wheel and install it + recipe deps into .venv (editor completions)
	uv venv --allow-existing
	uvx maturin build --release -m crates/htmlarc-py/Cargo.toml
	uv pip install --reinstall-package htmlarc target/wheels/htmlarc-*-abi3-*.whl
	uv pip install requests polars warcio trafilatura libzim

ci: fmt-check lint test bench ## Run every CI check locally
