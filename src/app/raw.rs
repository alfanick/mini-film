use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};

use crate::app::export::add_convert_thread_limit;

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

/// Run convert for Hald application and optional depth.
///
/// This is the non-streaming convert pass used after RawTherapee or when grain
/// requires an intermediate image. It limits convert threads to CPU count,
/// applies `-hald-clut`, optionally forces depth for JPEG-bound 8-bit grain,
/// and writes the requested intermediate output.
pub(crate) fn run_convert_depth(
    convert: &Path,
    input_tiff: &Path,
    hald: &Path,
    output: &Path,
    depth: Option<u8>,
) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let mut command = Command::new(convert);
    add_convert_thread_limit(&mut command);
    command.arg(input_tiff).arg("-hald-clut").arg(hald);
    if let Some(depth) = depth {
        command.arg("-depth").arg(depth.to_string());
    }

    let status = command
        .arg(output)
        .status()
        .with_context(|| format!("running {}", convert.display()))?;

    if !status.success() {
        bail!("convert failed with status {status}");
    }

    Ok(())
}
