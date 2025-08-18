.PHONY: help build test test-coverage clean clippy format check doc package publish install-tools ci pre-commit example bench ci-install test-coverage-lcov publish-ci ci-quick

# Default target
help:	## Show this help message
	@echo "Available targets:"
	@grep -E "^[a-zA-Z_-]+:.*?## .*" $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'

# Build targets
build:	## Build the project
	cargo build

build-release:	## Build the project in release mode
	cargo build --release

# Test targets
test:	## Run all tests
	cargo test

test-verbose:	## Run tests with verbose output
	cargo test -- --nocapture

test-coverage:	install-tools	## Generate test coverage report
	cargo tarpaulin --out Html --output-dir coverage

test-coverage-ci:	install-tools	## Generate test coverage for CI (lcov format)
	cargo tarpaulin --out Lcov --output-dir coverage

test-coverage-lcov: install-tools ## Generate test coverage in LCOV format for CI
	cargo tarpaulin --out Lcov --output-dir coverage

# Code quality targets
check:	## Check code without building
	cargo check

clippy:	## Run clippy linter
	cargo clippy -- -D warnings

clippy-fix:	## Run clippy with automatic fixes
	cargo clippy --fix --allow-dirty --allow-staged

format:	## Format code
	cargo fmt

format-check:	## Check if code is formatted
	cargo fmt -- --check

# Documentation targets
doc:	## Generate documentation
	cargo doc --no-deps

doc-open:	## Generate and open documentation
	cargo doc --no-deps --open

doc-all:	## Generate documentation with dependencies
	cargo doc

# Package and publish targets
package:	## Create a package
	cargo package

package-list:	## List files that would be included in package
	cargo package --list

publish-dry-run:	## Dry run of publishing to crates.io
	cargo publish --dry-run

publish:	## Publish to crates.io
	cargo publish

publish-ci: ## Publish to crates.io using CARGO_REGISTRY_TOKEN
	cargo publish --token ${CARGO_REGISTRY_TOKEN}

# Example and benchmark targets
example:	## Run the basic usage example
	cargo run --example basic_usage

example-with-logs:	## Run example with logging enabled
	RUST_LOG=debug cargo run --example basic_usage

bench:	## Run benchmarks (if any)
	cargo bench

# Development tools
install-tools:	## Install development tools
	cargo install cargo-tarpaulin || true
	cargo install cargo-audit || true
	cargo install cargo-outdated || true
	cargo install cargo-edit || true
	rustup component add clippy || true
	rustup component add rustfmt || true

ci-install: install-tools ## Install tools for CI environment
	@echo "CI tools installed!"

audit:	install-tools	## Audit dependencies for security vulnerabilities
	cargo audit

outdated:	install-tools	## Check for outdated dependencies
	cargo outdated

update:	## Update dependencies
	cargo update

# Cleaning targets
clean:	## Clean build artifacts
	cargo clean

clean-coverage:	## Clean coverage reports
	rm -rf coverage/

clean-all:	clean clean-coverage	## Clean everything

# CI/CD targets
ci:	format-check clippy test doc package	## Run all CI checks
	@echo "All CI checks passed!"

ci-quick: format-check clippy test doc ## Quick CI checks without coverage
	@echo "Quick CI checks passed!"

pre-commit:	format clippy test	## Run pre-commit checks
	@echo "Pre-commit checks passed!"

# Development workflow
dev-setup:	install-tools	## Set up development environment
	@echo "Development environment setup complete!"

quick-check:	format-check clippy check	## Quick development checks
	@echo "Quick checks passed!"

# Release workflow
pre-release:	ci test-coverage	## Prepare for release
	@echo "Pre-release checks complete!"
	@echo "Ready to tag and release!"

# Watch targets (requires cargo-watch)
watch-test:	## Watch for changes and run tests
	cargo watch -x test

watch-check:	## Watch for changes and run check
	cargo watch -x check

install-watch:	## Install cargo-watch
	cargo install cargo-watch

# Performance targets
profile:	## Build with profiling enabled
	cargo build --release --features profiling

size-analysis:	## Analyze binary size
	cargo build --release
	ls -la target/release/

# Security and maintenance
security-check:	audit	## Run security checks
	@echo "Security check complete!"

maintenance:	outdated audit	## Run maintenance checks
	@echo "Maintenance check complete!"

# Help with common workflows
workflow-help:	## Show common development workflows
	@echo "Common development workflows:"
	@echo "  1. Initial setup:     make dev-setup"
	@echo "  2. Before commit:     make pre-commit"
	@echo "  3. Full CI check:     make ci"
	@echo "  4. Test with coverage: make test-coverage"
	@echo "  5. Prepare release:   make pre-release"
	@echo "  6. Publish:           make publish-dry-run && make publish"
	@echo "  7. Run example:       make example-with-logs"
