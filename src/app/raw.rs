use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};

use crate::cli::JpegSubsampling;

/// Develop one RAW file with RawTherapee.
///
/// RawTherapee is the only RAW engine. TIFF-bound work uses a 16-bit TIFF
/// intermediate, while JPEG-bound work can use an 8-bit JPEG intermediate, and
/// batch/sampler can suppress RawTherapee's stdout/stderr to keep progress
/// output readable.
pub(crate) fn run_raw_develop(
    rawtherapee: &Path,
    profiles: &[PathBuf],
    raw: &Path,
    output_tiff: &Path,
    quiet: bool,
) -> Result<()> {
    run_rawtherapee(
        rawtherapee,
        profiles,
        raw,
        output_tiff,
        RawOutput::Tiff16,
        quiet,
    )
}

pub(crate) fn run_raw_develop_jpeg(
    rawtherapee: &Path,
    profiles: &[PathBuf],
    raw: &Path,
    output_jpeg: &Path,
    quality: u8,
    subsampling: JpegSubsampling,
    quiet: bool,
) -> Result<()> {
    run_rawtherapee(
        rawtherapee,
        profiles,
        raw,
        output_jpeg,
        RawOutput::Jpeg8 {
            quality,
            subsampling,
        },
        quiet,
    )
}

enum RawOutput {
    Tiff16,
    Jpeg8 {
        quality: u8,
        subsampling: JpegSubsampling,
    },
}

/// Invoke RawTherapee CLI and require it to create the requested intermediate.
///
/// RawTherapee can print warnings or fail in ways that are not useful for later
/// pipeline steps, so this wrapper creates the destination directory, requests
/// overwrite, selects either 16-bit TIFF or 8-bit JPEG output, optionally
/// silences logs for batch, then checks both process status and actual
/// output-file existence.
fn run_rawtherapee(
    rawtherapee: &Path,
    profiles: &[PathBuf],
    raw: &Path,
    output: &Path,
    output_format: RawOutput,
    quiet: bool,
) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let mut command = Command::new(rawtherapee);
    command.arg("-q").arg("-Y");
    for profile in profiles {
        command.arg("-p").arg(profile);
    }
    command.arg("-o").arg(output);
    match output_format {
        RawOutput::Tiff16 => {
            command.arg("-t").arg("-b16");
        }
        RawOutput::Jpeg8 {
            quality,
            subsampling,
        } => {
            command
                .arg(format!("-j{}", quality.clamp(1, 100)))
                .arg(format!("-js{}", rawtherapee_subsampling(subsampling)));
        }
    }
    command.arg("-c").arg(raw);
    if quiet {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let status = command
        .status()
        .with_context(|| format!("running {}", rawtherapee.display()))?;

    if !status.success() {
        bail!("rawtherapee failed with status {status}");
    }
    if !output.exists() {
        bail!("rawtherapee finished without creating {}", output.display());
    }

    Ok(())
}

fn rawtherapee_subsampling(subsampling: JpegSubsampling) -> u8 {
    match subsampling {
        JpegSubsampling::S420 => 1,
        JpegSubsampling::S422 => 2,
        JpegSubsampling::S444 => 3,
    }
}
