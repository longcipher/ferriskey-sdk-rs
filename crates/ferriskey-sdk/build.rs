//! Build-time contract artifact generation for the FerrisKey SDK crate.

#[expect(
    unreachable_pub,
    reason = "contract.rs is shared with the library crate and its integration tests"
)]
#[path = "src/contract.rs"]
mod contract;

use std::{env, fs};

fn main() -> Result<(), contract::ContractError> {
    run()
}

fn run() -> Result<(), contract::ContractError> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .map_err(contract::ContractError::Env)?;
    let source_contract_path = contract::source_contract_path(&manifest_dir);
    let artifacts = contract::generate_artifacts(&manifest_dir)?;
    let normalized_contract_path = contract::normalized_contract_path(&manifest_dir);
    let generated_module_path =
        std::path::PathBuf::from(env::var("OUT_DIR").map_err(contract::ContractError::Env)?)
            .join("generated_contract.rs");

    println!("cargo:rerun-if-changed={}", source_contract_path.display());

    if let Some(parent) = normalized_contract_path.parent() {
        fs::create_dir_all(parent).map_err(contract::ContractError::Io)?;
    }

    fs::write(&normalized_contract_path, artifacts.normalized_json)
        .map_err(contract::ContractError::Io)?;
    fs::write(generated_module_path, contract::render_generated_module(&artifacts.registry))
        .map_err(contract::ContractError::Io)?;

    Ok(())
}
