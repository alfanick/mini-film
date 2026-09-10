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
        "scripts",
        "tsconfig.review.json",
        "tsconfig.tooling.json",
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
    let version = Command::new(&node)
        .arg("--version")
        .output()
        .expect("building the review UI requires Node.js 24.12 or newer and npm in PATH");
    let version_text = String::from_utf8_lossy(&version.stdout);
    let mut version_parts = version_text.trim().trim_start_matches('v').split('.');
    let major = version_parts
        .next()
        .and_then(|part| part.parse::<u32>().ok());
    let minor = version_parts
        .next()
        .and_then(|part| part.parse::<u32>().ok());
    assert!(
        version.status.success()
            && matches!((major, minor), (Some(major), Some(minor)) if major > 24 || major == 24 && minor >= 12),
        "building the review UI requires Node.js 24.12 or newer; found {}",
        version_text.trim()
    );
    let status = Command::new(node)
        .current_dir(&root)
        .arg(root.join("scripts/build-review.mts"))
        .arg("--cargo-out-dir")
        .arg(output)
        .arg("--contracts-dir")
        .arg(contracts)
        .arg("--profile")
        .arg(env::var("PROFILE").unwrap())
        .status()
        .expect("building the review UI requires Node.js 24.12 or newer and npm in PATH");
    assert!(
        status.success(),
        "review UI build failed; see diagnostics above"
    );

    #[cfg(feature = "desktop-app")]
    tauri_build::build();
}
