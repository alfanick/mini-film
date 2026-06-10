use std::time::Duration;

use anyhow::{Result, anyhow};

use crate::updater::{LensfunUpdateReport, run_binary_update, run_lensfun_update};

pub(crate) fn run_update() -> Result<()> {
    let mut failures = Vec::new();
    let mut succeeded = false;

    eprintln!("checking mini-film release...");
    let binary_result = run_binary_update(Duration::from_secs(1));
    if let Ok(message) = &binary_result {
        succeeded = true;
        eprintln!("binary: {message}");
    } else if let Err(error) = binary_result {
        failures.push(format!("binary update failed: {error}"));
        eprintln!("binary: failed: {error}");
    }

    eprintln!("updating Lensfun database...");
    let lensfun_result = run_lensfun_update();
    match lensfun_result {
        Ok(report) => {
            succeeded = true;
            print_lensfun_report(&report)
        }
        Err(error) => {
            failures.push(format!("lensfun update failed: {error}"));
            eprintln!("lensfun: failed: {error}");
        }
    }

    if failures.is_empty() || (succeeded && !failures.is_empty()) {
        return Ok(());
    }

    Err(anyhow!(
        "both mini-film and lensfun updates failed: {}",
        failures.join("; ")
    ))
}

fn print_lensfun_report(report: &LensfunUpdateReport) {
    eprintln!(
        "lensfun: synced {} files from {} to {}",
        report.copied_files,
        report.cache_dir.display(),
        report.synced_dir.display()
    );
}
