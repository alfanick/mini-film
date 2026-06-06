#[cfg(feature = "github-update")]
use self_update::Status;

#[cfg(feature = "github-update")]
use self_update::cargo_crate_version;

/// Runs a GitHub release update check for official binaries.
///
/// Returns `Ok(())` regardless of whether an update is available; only
/// network/configuration errors are reported as failures.
#[cfg(feature = "github-update")]
pub(crate) fn run_auto_update_if_enabled() -> bool {
    if let Err(error) = check_for_update() {
        eprintln!("auto-update failed: {error}");
        return false;
    }
    true
}

#[cfg(feature = "github-update")]
fn check_for_update() -> anyhow::Result<()> {
    let status = self_update::backends::github::Update::configure()
        .repo_owner("alfanick")
        .repo_name("mini-film")
        .bin_name("mini-film")
        .target(asset_target())
        .current_version(cargo_crate_version!())
        .build()?
        .update()?;

    match status {
        Status::UpToDate(latest) => {
            println!("mini-film is up to date ({})", latest);
            Ok(())
        }
        Status::Updated(version) => {
            println!("mini-film updated to {version}");
            Ok(())
        }
    }
}

#[cfg(feature = "github-update")]
fn asset_target() -> &'static str {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else {
        self_update::get_target()
    }
}

#[cfg(not(feature = "github-update"))]
pub(crate) fn run_auto_update_if_enabled() -> bool {
    true
}
