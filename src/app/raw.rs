use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};

/// Develop one RAW file with RawTherapee.
///
/// RawTherapee is the only RAW engine. The apply pipeline treats its output as
/// a 16-bit TIFF intermediate, and batch/sampler can suppress RawTherapee's
/// stdout/stderr to keep progress output readable.
pub(crate) fn run_raw_develop(
    rawtherapee: &Path,
    profiles: &[PathBuf],
    raw: &Path,
    output_tiff: &Path,
    quiet: bool,
) -> Result<()> {
    run_rawtherapee(rawtherapee, profiles, raw, output_tiff, quiet)
}

/// Invoke RawTherapee CLI and require it to create the requested TIFF.
///
/// RawTherapee can print warnings or fail in ways that are not useful for later
/// pipeline steps, so this wrapper creates the destination directory, requests
/// overwrite, TIFF output, and 16-bit depth, optionally silences logs for batch,
/// then checks both process status and actual output-file existence.
fn run_rawtherapee(
    rawtherapee: &Path,
    profiles: &[PathBuf],
    raw: &Path,
    output_tiff: &Path,
    quiet: bool,
) -> Result<()> {
    if let Some(parent) = output_tiff.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let mut command = Command::new(rawtherapee);
    command.arg("-q").arg("-Y");
    for profile in profiles {
        command.arg("-p").arg(profile);
    }
    command
        .arg("-o")
        .arg(output_tiff)
        .arg("-t")
        .arg("-b16")
        .arg("-c")
        .arg(raw);
    if quiet {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let status = command
        .status()
        .with_context(|| format!("running {}", rawtherapee.display()))?;

    if !status.success() {
        bail!("rawtherapee failed with status {status}");
    }
    if !output_tiff.exists() {
        bail!(
            "rawtherapee finished without creating {}",
            output_tiff.display()
        );
    }

    Ok(())
}
