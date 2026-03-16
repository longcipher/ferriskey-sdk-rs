# FerrisKey SDK CLI — Implementation Tasks

| Metadata | Details |
| :--- | :--- |
| **Design Doc** | specs/2026-03-17-01-ferriskey-sdk-cli/design.md |
| **Owner** | pb-plan agent |
| **Start Date** | 2026-03-17 |
| **Target Date** | 2026-03-24 |
| **Status** | Planning |

## Summary & Phasing

This work should start by removing template identities and establishing a contract-derived foundation. Once the operation registry and normalized Prism artifact exist, the SDK runtime and CLI can be built against one shared descriptor layer. Prism-backed integration and BDD acceptance then become the contract safety net, followed by workspace command and documentation cleanup.

- **Property Testing Rule:** Add `proptest` coverage for descriptor normalization, request encoding, and CLI argument normalization because those behaviors span broad input combinations.
- **Fuzzing Rule:** `N/A` unless the implementation later introduces hostile-input parsing beyond the trusted repository-owned OpenAPI file.
- **Benchmark Rule:** `N/A` unless a later requirement introduces a specific performance target.
- **Identity Alignment Rule:** Rename `common` and `cli-app` before feature work so all later tasks target FerrisKey-specific crates.
- **Behavior Preservation Rule:** Replace the template demo behavior intentionally, then preserve FerrisKey contract behavior for all generated SDK and CLI surfaces.
- **Simplification Rule:** Use one shared operation registry for SDK, CLI, and Prism tests to avoid duplicate endpoint definitions.
- **Clarity Guardrail:** Prefer explicit modules and stable generated descriptors over macro-heavy handwritten endpoint glue.
- **Phase 1: Foundation & Scaffolding** — Identity cleanup, contract normalization, transport/auth seams
- **Phase 2: Core Logic** — SDK generation, typed execution, CLI command tree
- **Phase 3: Integration & Features** — Prism orchestration, full-surface contract coverage, BDD acceptance
- **Phase 4: Polish, QA & Docs** — Docs, commands, final verification

---

## Phase 1: Foundation & Scaffolding

### Task 1.1: Align Workspace Identity

> **Context:** The current workspace is still a template. Rename `crates/common` to `crates/ferriskey-sdk`, rename `bin/cli-app` to `bin/ferriskey-cli`, migrate the BDD runner to the SDK crate, and update placeholder metadata such as `repository = "TODO"`. This unlocks every later task by giving the SDK and CLI their final crate identities.
> **Verification:** Workspace manifests, `Justfile`, and docs all reference FerrisKey-specific crate names and still compile after the rename.

- **Priority:** P0
- **Scope:** Workspace layout and identity alignment
- **Scenario Coverage:** `prism-contract.feature: Normalized contract preserves the API surface`
- **Loop Type:** `TDD-only`
- **Behavioral Contract:** Replace template greeting/checkout behavior with FerrisKey SDK + CLI workspace structure.
- **Simplification Focus:** improve naming
- **Advanced Test Coverage:** Example-based only
- **Status:** 🟢 DONE
- [x] **Step 1:** Rename the template crates and update workspace manifests, paths, and `Justfile` recipes to target `ferriskey-sdk` and `ferriskey-cli`.
- [x] **Step 2:** Replace placeholder README and crate descriptions so subsequent tasks document FerrisKey-specific behavior.
- [x] **BDD Verification:** `N/A` because this task establishes crate identity rather than user-visible contract behavior.
- [x] **Verification:** `cargo check --workspace` succeeds after the rename.
- [x] **Advanced Test Verification:** `N/A` because no broad input-domain logic is introduced yet.
- [x] **Runtime Verification (if applicable):** `N/A` because this task does not start runtime services.

### Task 1.2: Build Contract Normalization And Registry Generation

> **Context:** `docs/openai.json` is the authoritative contract, but it lacks `servers`, has incomplete root tags, and references bearer auth without a declared `components.securitySchemes`. Introduce a normalization and generation step inside `crates/ferriskey-sdk` that emits a stable operation registry, tag inventory, model skeleton, and a Prism-compatible normalized contract artifact without mutating the source file.
> **Verification:** Generated metadata reports 72 paths, 107 operations, 12 tag groups, and 154 schemas, and the normalized contract remains diff-checkable against the source structure.

- **Priority:** P0
- **Scope:** Contract ingestion and code generation foundation
- **Scenario Coverage:** `prism-contract.feature: Normalized contract preserves the API surface`, `sdk-contract.feature: SDK exposes every documented operation`
- **Loop Type:** `BDD+TDD`
- **Behavioral Contract:** Preserve the documented FerrisKey contract while making it consumable by Prism and Rust code generation.
- **Simplification Focus:** consolidate related logic
- **Advanced Test Coverage:** Combination
- **Status:** 🟢 DONE
- [x] **Step 1:** Add a build-time normalization path that computes tag inventory from operations, injects missing test-only security metadata when referenced, and writes `target/prism/openai.prism.json`.
- [x] **Step 2:** Generate Rust descriptors and schema modules from the normalized contract so handwritten code never duplicates endpoint definitions.
- [x] **BDD Verification:** Add the `prism-contract.feature` scenario for contract preservation, confirm it fails before generation exists, then turns green after implementation.
- [x] **Verification:** `cargo test -p ferriskey-sdk normalized_contract_preserves_operation_counts -- --nocapture` passes and asserts the operation/tag/schema counts.
- [x] **Advanced Test Verification:** `cargo test -p ferriskey-sdk contract_registry_properties -- --nocapture` verifies deterministic descriptor generation and count preservation across normalization passes.
- [x] **Runtime Verification (if applicable):** `N/A` because this task produces artifacts but does not run a long-lived service.

### Task 1.3: Implement Transport, Auth, And Error Seams

> **Context:** The SDK must respect the repository's dependency policy: `hpx` for HTTP, `thiserror` in library code, and explicit auth injection through interfaces. Introduce the handwritten runtime core now so generated methods and Prism tests have one execution path.
> **Verification:** A `FerriskeySdk<T: Transport>` entrypoint can build requests, apply optional bearer auth, and return typed transport or decode errors without direct dependency on a concrete HTTP client in business-facing modules.

- **Priority:** P0
- **Scope:** Runtime execution foundation
- **Scenario Coverage:** `sdk-contract.feature: Secured operations apply bearer auth and decode structured responses`
- **Loop Type:** `TDD-only`
- **Behavioral Contract:** Preserve contract semantics while making transport and auth injectable.
- **Simplification Focus:** reduce nesting
- **Advanced Test Coverage:** Example-based only
- **Status:** 🟢 DONE
- [x] **Step 1:** Add `Transport`, `SdkConfig`, `AuthStrategy`, `SdkRequest`, `SdkResponse`, and `SdkError` abstractions with `HpxTransport` as the primary adapter.
- [x] **Step 2:** Add unit tests for bearer-header injection, missing-auth failures, and status/body mismatch handling.
- [x] **BDD Verification:** `N/A` because secured behavior is accepted later through Prism-backed scenarios after endpoint descriptors exist.
- [x] **Verification:** `cargo test -p ferriskey-sdk transport_and_auth_core -- --nocapture` passes.
- [x] **Advanced Test Verification:** `N/A` because this task's logic is narrow enough for example-based coverage.
- [x] **Runtime Verification (if applicable):** `N/A` because no external server is required yet.

---

## Phase 2: Core Logic

### Task 2.1: Generate Typed SDK Models And Operation Facades

> **Context:** This task turns the normalized contract into usable Rust APIs. Reuse the generated descriptor pipeline from Task 1.2 and the transport/auth seams from Task 1.3 to expose one typed façade per tag (`realm`, `user`, `auth`, and so on) plus a generic `execute` path for exhaustive contract sweeps.
> **Verification:** Every `operationId` in the contract maps to one descriptor and one callable SDK method or generic executor entry.

- **Priority:** P0
- **Scope:** Generated models and SDK surface
- **Scenario Coverage:** `sdk-contract.feature: SDK exposes every documented operation`
- **Loop Type:** `BDD+TDD`
- **Behavioral Contract:** Implement the documented FerrisKey API surface exactly once through the SDK.
- **Simplification Focus:** remove redundancy
- **Advanced Test Coverage:** Combination
- **Status:** 🟢 DONE
- [x] **Step 1:** Generate or materialize typed schema models and operation descriptors in `crates/ferriskey-sdk/src/generated/` with deterministic module names grouped by tag.
- [x] **Step 2:** Expose tag clients plus a generic executor on the public SDK, ensuring every descriptor can round-trip from method call to request construction.
- [x] **BDD Verification:** Add the `sdk-contract.feature` scenario for full operation exposure, make it fail while some operations are missing, then make it pass when the descriptor and facade inventory reaches parity.
- [x] **Verification:** `cargo test -p ferriskey-sdk sdk_exposes_all_operations -- --nocapture` passes and asserts parity with the contract registry.
- [x] **Advanced Test Verification:** `cargo test -p ferriskey-sdk response_mapping_properties -- --nocapture` checks deterministic status-to-decoder mapping for generated operations.
- [x] **Runtime Verification (if applicable):** `N/A` because this task can be verified with unit and generated-surface tests before Prism is introduced.

### Task 2.2: Implement Request Encoding And Response Decoding Rules

> **Context:** Once all descriptors exist, the SDK still needs a single canonical encoder/decoder path that handles path parameters, repeated query parameters, nullable request bodies, and mixed success/error payloads. This is broad input-domain logic and should carry property coverage.
> **Verification:** Generated operations can serialize requests and decode Prism or server responses according to their descriptor metadata without handwritten per-endpoint encoding code.

- **Priority:** P0
- **Scope:** Request/response pipeline
- **Scenario Coverage:** `sdk-contract.feature: SDK exposes every documented operation`, `sdk-contract.feature: Secured operations apply bearer auth and decode structured responses`
- **Loop Type:** `BDD+TDD`
- **Behavioral Contract:** Preserve the request and response semantics defined by the FerrisKey contract.
- **Simplification Focus:** consolidate related logic
- **Advanced Test Coverage:** Property
- **Status:** 🟢 DONE
- [x] **Step 1:** Implement generic path/query/header/body encoders driven by `OperationDescriptor` metadata.
- [x] **Step 2:** Implement response matching and decoding for documented status codes, including typed API error payloads where present.
- [x] **BDD Verification:** Re-run the SDK contract scenario and keep it red until at least one representative operation per tag can execute through the generic pipeline.
- [x] **Verification:** `cargo test -p ferriskey-sdk request_response_pipeline -- --nocapture` passes for representative operations across all tag groups.
- [x] **Advanced Test Verification:** `cargo test -p ferriskey-sdk parameter_encoding_properties -- --nocapture` verifies path/query serialization and deterministic omission rules.
- [x] **Runtime Verification (if applicable):** `N/A` until Prism-backed integration begins in Phase 3.

### Task 2.3: Implement Dynamic CLI Command Generation

> **Context:** The CLI must expose every documented operation without manually maintaining 107 subcommands. Reuse the operation registry to build a Clap command tree grouped by tag, map flags to path/query/body arguments, and call the SDK rather than bypassing it.
> **Verification:** Operators can discover API domains from the CLI, pass required arguments in a predictable format, and receive structured output from the same execution path as SDK consumers.

- **Priority:** P0
- **Scope:** CLI surface and argument normalization
- **Scenario Coverage:** `cli-invocation.feature: CLI command groups mirror API tags`, `cli-invocation.feature: CLI flags and JSON body arguments invoke the same contract as the SDK`
- **Loop Type:** `BDD+TDD`
- **Behavioral Contract:** Introduce a full FerrisKey CLI whose behavior mirrors the documented API contract.
- **Simplification Focus:** remove redundancy
- **Advanced Test Coverage:** Combination
- **Status:** 🟢 DONE
- [x] **Step 1:** Build `bin/ferriskey-cli` with a descriptor-driven command tree that groups commands by tag and supports path/query/body arguments plus auth/base-url configuration.
- [x] **Step 2:** Reuse SDK execution and output formatting helpers so the CLI stays a thin adapter rather than a second HTTP client.
- [x] **BDD Verification:** Add the CLI feature scenarios, confirm command discovery and invocation fail before the CLI exists, then make them pass through the shared SDK runtime.
- [x] **Verification:** `cargo test -p ferriskey-sdk --test cli_smoke cli_lists_tag_groups cli_invokes_operation_with_arguments` passes.
- [x] **Advanced Test Verification:** `cargo test -p ferriskey-sdk cli_argument_properties -- --nocapture` verifies descriptor-driven argument normalization.
- [x] **Runtime Verification (if applicable):** `N/A` until Prism-backed integration begins in Phase 3.

---

## Phase 3: Integration & Features

### Task 3.1: Add Prism Process Orchestration And Full Contract Sweep

> **Context:** The user explicitly requires Stoplight Prism as the mock server for integration testing. Add reusable test support that launches Prism from the normalized contract artifact, records logs, waits for readiness, and then sweeps every SDK operation against the mock. This task depends on the transport seam from Task 1.3 and the full registry from Task 2.1.
> **Verification:** Prism boots from the normalized contract, the SDK can call every documented operation through the registry, and the test suite reports zero uncovered operations.

- **Priority:** P0
- **Scope:** External mock integration and contract sweep
- **Scenario Coverage:** `prism-contract.feature: Prism sweep validates every documented operation`, `sdk-contract.feature: Secured operations apply bearer auth and decode structured responses`
- **Loop Type:** `BDD+TDD`
- **Behavioral Contract:** Validate the implemented SDK against the FerrisKey contract via Prism rather than only unit tests.
- **Simplification Focus:** consolidate related logic
- **Advanced Test Coverage:** Example-based only
- **Status:** 🟢 DONE
- [x] **Step 1:** Add a Prism launcher helper that starts `@stoplight/prism-cli` against `target/prism/openai.prism.json`, captures `target/prism/prism.log`, and blocks until a probe endpoint responds.
- [x] **Step 2:** Implement `prism_contract.rs` to iterate over the full registry, synthesize representative inputs for each operation, and assert coverage plus typed decoding.
- [x] **BDD Verification:** Make `prism-contract.feature` fail before the sweep exists, then pass once the registry and Prism harness validate all operations.
- [x] **Verification:** `cargo test -p ferriskey-sdk --test prism_contract` passes and reports 107 operations covered.
- [x] **Advanced Test Verification:** `N/A` because the contract sweep itself is the primary advanced integration verification here.
- [x] **Runtime Verification (if applicable):** Capture `tail -n 50 target/prism/prism.log` and `curl -sSf http://127.0.0.1:${PRISM_PORT:-4010}/realms/test/.well-known/openid-configuration` during the integration run.

### Task 3.2: Wire CLI Acceptance Scenarios And End-To-End Smoke Tests

> **Context:** The CLI must be proven through the same Prism harness, not only by inspecting the command tree. Reuse the Prism launcher from Task 3.1 and keep the `cucumber-rs` world thin by delegating real work to SDK and CLI test helpers.
> **Verification:** BDD scenarios cover tag discovery, operation invocation, and SDK parity while Prism is running, and CLI smoke tests confirm human-facing behavior.

- **Priority:** P1
- **Scope:** Acceptance coverage for CLI and SDK
- **Scenario Coverage:** `cli-invocation.feature: CLI command groups mirror API tags`, `cli-invocation.feature: CLI flags and JSON body arguments invoke the same contract as the SDK`, `sdk-contract.feature: SDK exposes every documented operation`
- **Loop Type:** `BDD+TDD`
- **Behavioral Contract:** Provide a CLI and SDK experience that matches the documented API through end-to-end tests.
- **Simplification Focus:** reduce nesting
- **Advanced Test Coverage:** Example-based only
- **Status:** 🟢 DONE
- [x] **Step 1:** Add `crates/ferriskey-sdk/tests/bdd.rs` and step helpers that exercise the SDK and CLI through Prism-backed test fixtures.
- [x] **Step 2:** Add `cli_smoke.rs` to validate representative command invocations and output formatting against Prism responses.
- [x] **BDD Verification:** `cargo test -p ferriskey-sdk --test bdd` passes all FerrisKey feature scenarios.
- [x] **Verification:** `cargo test -p ferriskey-sdk --test cli_smoke` passes for representative commands from multiple tag groups.
- [x] **Advanced Test Verification:** `N/A` because this task's value is end-to-end acceptance rather than additional generator invariants.
- [x] **Runtime Verification (if applicable):** Capture `tail -n 50 target/prism/prism.log` and a CLI probe such as `cargo run -p ferriskey-cli -- --base-url http://127.0.0.1:${PRISM_PORT:-4010} auth get-openid-configuration --realm-name test`.

---

## Phase 4: Polish, QA & Docs

### Task 4.1: Finalize Repository Commands, Documentation, And Verification Wiring

> **Context:** After SDK, CLI, and Prism tests exist, the repository still needs operator-facing documentation and stable commands. Update `README.md`, extend `Justfile` recipes where needed, and ensure the workspace verification flow remains aligned with AGENTS.md.
> **Verification:** A maintainer can follow the documented steps to run the SDK tests, Prism-backed contract suite, and CLI examples without tribal knowledge.

- **Priority:** P2
- **Scope:** Docs, command surface, and final QA wiring
- **Scenario Coverage:** `prism-contract.feature: Prism sweep validates every documented operation`, `cli-invocation.feature: CLI flags and JSON body arguments invoke the same contract as the SDK`
- **Loop Type:** `TDD-only`
- **Behavioral Contract:** Preserve the existing workspace verification cadence while adding FerrisKey-specific docs and commands.
- **Simplification Focus:** improve naming
- **Advanced Test Coverage:** Example-based only
- **Status:** 🟢 DONE
- [x] **Step 1:** Rewrite README usage, CLI examples, and setup notes around FerrisKey SDK + CLI + Prism prerequisites.
- [x] **Step 2:** Update `Justfile` so `just bdd` and `just test-all` target the FerrisKey crate and cover Prism-backed tests or document the exact follow-up command if Prism remains opt-in.
- [x] **BDD Verification:** `cargo test -p ferriskey-sdk --test bdd` still passes after command and doc updates.
- [x] **Verification:** `just format && just lint && just test && just bdd && just test-all` completes successfully.
- [x] **Advanced Test Verification:** `N/A` because this task wires existing test layers rather than adding new invariants.
- [x] **Runtime Verification (if applicable):** Capture `tail -n 50 target/prism/prism.log` and `curl -sSf http://127.0.0.1:${PRISM_PORT:-4010}/realms/test/.well-known/openid-configuration` during the final verification run.

---

## Summary & Timeline

| Phase | Tasks | Target Date |
| :--- | :---: | :--- |
| **1. Foundation** | 3 | 03-18 |
| **2. Core Logic** | 3 | 03-20 |
| **3. Integration** | 2 | 03-22 |
| **4. Polish** | 1 | 03-24 |
| **Total** | **9** | |

## Definition of Done

1. [ ] **Linted:** No lint errors.
2. [ ] **Tested:** Unit tests covering added logic.
3. [ ] **Formatted:** Code formatter applied.
4. [ ] **Verified:** Each task's specific verification criterion met.
5. [ ] **Advanced-Tested (when applicable):** Property-test verification captured, or `N/A` is explicitly justified.
6. [ ] **Runtime-Evidenced (when applicable):** Prism logs and probe results are captured, or `N/A` is explicitly justified.
7. [ ] **Behavior-Preserved or Documented:** Intentional replacement of template behavior is documented, and FerrisKey contract behavior is preserved thereafter.
8. [ ] **Simplified Responsibly:** Shared descriptors prevent duplicated endpoint definitions and cleanup stays within the planned scope.
