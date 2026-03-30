# Default recipe to display help
default:
  @just --list

# Format all code
format:
  rumdl fmt .
  taplo fmt
  cargo +nightly fmt --all

# Auto-fix linting issues
fix:
  rumdl check --fix .

# Run all lints
lint:
  typos
  rumdl check .
  taplo fmt --check
  cargo +nightly fmt --all -- --check
  cargo +nightly clippy --all -- -D warnings
  cargo machete

# Run tests
test:
  cargo test --all-features

# Run BDD scenarios
bdd:
  cargo test -p ferriskey-sdk --test bdd

# Run both TDD and BDD suites
test-all:
  cargo test --all-features
  cargo test -p ferriskey-sdk --test bdd

# Run tests with coverage
test-coverage:
  cargo tarpaulin --all-features --workspace --timeout 300

# Build entire workspace
build:
  cargo build --workspace

# Check all targets compile
check:
  cargo check --all-targets --all-features

# Publish all crates to crates.io (dry run)
publish-check:
  cargo publish --workspace --dry-run --allow-dirty

# Publish all crates to crates.io
publish:
  cargo publish --workspace

# Check for Chinese characters
check-cn:
  rg --line-number --column "\p{Han}"

# Full CI check
ci:
  @if ! command -v npx >/dev/null 2>&1; then echo "Error: npx not found. Please install Node.js."; exit 1; fi
  @if ! npx @stoplight/prism-cli --version >/dev/null 2>&1; then echo "Installing Prism..."; npm install --global @stoplight/prism-cli; fi
  just lint
  just build
  just test-all

# ============================================================
# Maintenance & Tools
# ============================================================

# Clean build artifacts
clean:
  cargo clean

# Install all required development tools
setup:
  cargo install cargo-machete
  cargo install taplo-cli
  cargo install typos-cli
  if ! command -v prism >/dev/null 2>&1; then npm install --global @stoplight/prism-cli; fi

# Generate documentation for the workspace
docs:
  cargo doc --no-deps --open
