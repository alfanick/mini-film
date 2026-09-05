//! Compile and embed the review frontend for every Cargo feature combination.
//! The frontend workspace stays in OUT_DIR so source checkouts remain untouched.

use std::{env, path::PathBuf, process::Command};

/// Build the browser bundle before Rust embeds it, then run optional Tauri setup.
fn main() {
    for path in [
        "frontend/review",
        "scripts/build-review.mjs",
        "tsconfig.review.json",
        "eslint.config.mjs",
        "package.json",
        "package-lock.json",
        "assets",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    for key in ["PATH", "NODE", "NPM_CONFIG_REGISTRY", "NPM_CONFIG_CACHE"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let node = env::var_os("NODE").unwrap_or_else(|| "node".into());
    let status = Command::new(node)
        .current_dir(&root)
        .arg(root.join("scripts/build-review.mjs"))
        .arg("--cargo-out-dir")
        .arg(output)
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
