# FerrisKey SDK Workspace

[![DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/longcipher/ferriskey-sdk-rs)
[![Context7](https://img.shields.io/badge/Website-context7.com-blue)](https://context7.com/longcipher/ferriskey-sdk-rs)
[![crates.io](https://img.shields.io/crates/v/ferriskey-sdk.svg)](https://crates.io/crates/ferriskey-sdk)
[![docs.rs](https://docs.rs/ferriskey-sdk/badge.svg)](https://docs.rs/ferriskey-sdk)

![ferriskey-sdk](https://socialify.git.ci/longcipher/ferriskey-sdk-rs/image?font=Source+Code+Pro&language=1&name=1&owner=1&pattern=Circuit+Board&theme=Auto)

Rust workspace for the FerrisKey OpenAPI contract in [docs/openai.json](docs/openai.json). The repository ships two primary artifacts:

- `crates/ferriskey-sdk`: the shared Rust SDK generated and validated from the FerrisKey contract
- `bin/ferriskey-cli`: a descriptor-driven CLI that exercises the same SDK runtime path as library consumers

The workspace uses `cucumber-rs` for acceptance coverage, crate-local tests for the inner TDD loop, and Stoplight Prism as the contract mock for end-to-end verification.

## What Is Here

- Contract normalization and generated operation metadata derived from `docs/openai.json`
- Shared request encoding and response decoding for the entire SDK surface
- A dynamic CLI command tree grouped by API tag and operation
- Prism-backed SDK sweep coverage for all documented operations
- Prism-backed CLI smoke coverage and BDD acceptance scenarios
- Strict format, lint, test, and BDD verification through `just`

## Prerequisites

- Rust toolchain with nightly available for `cargo fmt` and clippy checks
- Node.js and npm for Stoplight Prism
- `just` for the workspace command surface

## Setup

```bash
just setup
just format
just lint
just test
just bdd
just test-all
```

`just setup` installs the Rust workspace tools and ensures `@stoplight/prism-cli` is available so the Prism-backed tests and examples can run locally.

## Workspace Layout

- `bin/ferriskey-cli`: CLI binary crate
- `crates/ferriskey-sdk`: reusable SDK crate
- `features/`: Gherkin acceptance scenarios executed by `cargo test -p ferriskey-sdk --test bdd`
- `docs/openai.json`: authoritative FerrisKey OpenAPI contract
- `target/prism/openai.prism.json`: normalized Prism-ready artifact generated from the contract

## Common Commands

```bash
just format
just lint
just test
just bdd
just test-all
just build
```

- `just test` runs the crate and integration test suites, including Prism-backed smoke tests.
- `just bdd` runs the FerrisKey acceptance scenarios from `features/`.
- `just test-all` runs both the TDD and BDD layers together.

## CLI Usage

Top-level commands are generated from API tags and operation descriptors. The CLI takes a required base URL, an optional bearer token, and emits structured JSON.

```bash
cargo run -p ferriskey-cli -- \
	--base-url http://127.0.0.1:4010 \
	auth get-openid-configuration \
	--realm-name test
```

Example with a request body:

```bash
cargo run -p ferriskey-cli -- \
	--base-url http://127.0.0.1:4010 \
	--bearer-token example-token \
	realm update-realm \
	--name test \
	--body '{"display_name":"example"}'
```

The `--body` argument accepts inline JSON or `@path/to/file.json`.

## Authentication

The CLI supports one-time authentication that saves credentials to `~/.ferriskey-cli/config.toml`. Once authenticated, subsequent commands automatically use the saved credentials without needing to specify `--base-url` or `--bearer-token`.

### Login

Authenticate with a FerrisKey server and save credentials:

```bash
cargo run -p ferriskey-cli -- login \
	--base-url http://127.0.0.1:8080 \
	--username admin \
	--password secret \
	--realm-name master
```

The `--realm-name` parameter is optional and defaults to `master`.

### Using Saved Credentials

After logging in, you can run commands without specifying credentials:

```bash
# Credentials are loaded automatically from ~/.ferriskey-cli/config.toml
cargo run -p ferriskey-cli -- realm get-realms --realm-name master
cargo run -p ferriskey-cli -- user get-users --realm-name master
```

You can still override saved credentials by providing `--base-url` or `--bearer-token` explicitly:

```bash
cargo run -p ferriskey-cli -- \
	--base-url http://different-server:8080 \
	realm get-realms --realm-name master
```

## Prism Verification

The acceptance and integration layers expect Prism to serve the normalized contract artifact. A direct local probe looks like this:

```bash
prism mock target/prism/openai.prism.json --host 127.0.0.1 --port 4010 --dynamic

cargo run -p ferriskey-cli -- \
	--base-url http://127.0.0.1:4010 \
	auth get-openid-configuration \
	--realm-name test
```

The SDK and CLI test suites start Prism automatically when needed, so the normal `just test`, `just bdd`, and `just test-all` flows already cover the contract-backed paths.

## Testing Model

- BDD via `features/*.feature` defines the acceptance contract.
- Example-based tests cover named SDK and CLI behaviors.
- `proptest` stays inside the ordinary `cargo test` loop for invariant-style checks.
- Prism-backed tests validate the SDK and CLI against the normalized contract, not a handwritten mock.

## License

Apache-2.0
