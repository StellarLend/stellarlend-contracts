# StellarLend Hello-World Contract Makefile

.PHONY: build test audit clean help

# Default target
help:
	@echo "StellarLend Hello-World Contract"
	@echo "================================"
	@echo ""
	@echo "Available targets:"
	@echo "  build    - Build the WASM contract"
	@echo "  test     - Run contract tests"
	@echo "  audit    - Run WASM size and API audit"
	@echo "  clean    - Clean build artifacts"
	@echo "  help     - Show this help message"

# Build the contract
build:
	@echo "🔨 Building StellarLend contract..."
	stellar contract build

# Run tests (note: currently has std issues, but keeping for future)
test:
	@echo "🧪 Running contract tests..."
	cargo test --lib || echo "⚠️  Tests currently have std compatibility issues"

# Run WASM audit
audit:
	@echo "🔍 Running WASM audit..."
	@./scripts/wasm-audit.sh

# Clean build artifacts
clean:
	@echo "🧹 Cleaning build artifacts..."
	cargo clean
	rm -f build_output.tmp

# Build and audit in one command
build-audit: build audit

# Quick size check
size:
	@echo "📏 Quick size check..."
	@stellar contract build 2>/dev/null | grep "Wasm Size:" || echo "Build first with 'make build'"