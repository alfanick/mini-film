//! Compile and embed the review frontend for every Cargo feature combination.
//! The frontend workspace stays in OUT_DIR so source checkouts remain untouched.

use std::{env, path::PathBuf, process::Command};

#[path = "src/review_contract/mod.rs"]
pub mod review_contract;
#[path = "build-support/review_schema.rs"]
pub mod review_schema;

/// Build the browser bundle before Rust embeds it, then run optional Tauri setup.
fn main() {
    for path in [
        "frontend/review",
        "scripts/build-review.mjs",
        "scripts/review-contracts.mjs",
        "tsconfig.review.json",
        "eslint.config.mjs",
        "package.json",
        "package-lock.json",
        "assets",
        "src/review_contract",
        "build-support",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    for key in ["PATH", "NODE", "NPM_CONFIG_REGISTRY", "NPM_CONFIG_CACHE"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let contracts = output.join("contracts");
    review_schema::export(&contracts).expect("exporting review JSON contracts");
    let node = env::var_os("NODE").unwrap_or_else(|| "node".into());
    let status = Command::new(node)
        .current_dir(&root)
        .arg(root.join("scripts/build-review.mjs"))
        .arg("--cargo-out-dir")
        .arg(output)
        .arg("--contracts-dir")
        .arg(contracts)
        .arg("--profile")
        .arg(env::var("PROFILE").unwrap())
        .status()
        .expect("building the review UI requires Node.js 24 or newer and npm in PATH");
    assert!(
        status.success(),
        "review UI build failed; see diagnostics above"
    );

    #[cfg(feature = "desktop-app")]
    tauri_build::build();
}
