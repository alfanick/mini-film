use std::{
    fs::{self, File},
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};
use mini_film::SharpeningSettings;

use crate::app::export::{add_convert_thread_limit, add_final_convert_args, add_sharpening_args};
use crate::cli::{ExportOptions, RawEngine};

pub(crate) fn raw_engine_step(engine: RawEngine) -> &'static str {
    match engine {
        RawEngine::Auto => "rawtherapee",
        RawEngine::Rawtherapee => "rawtherapee",
        RawEngine::Dcraw => "dcraw",
    }
}

pub(crate) fn run_raw_develop(
    engine: RawEngine,
    rawtherapee: &Path,
    dcraw: &Path,
    dcraw_args: &[String],
    camera_profile: Option<&str>,
    raw: &Path,
    output_tiff: &Path,
    quiet: bool,
) -> Result<()> {
    match engine {
        RawEngine::Rawtherapee => run_rawtherapee(rawtherapee, raw, output_tiff, quiet),
        RawEngine::Dcraw => run_dcraw(dcraw, dcraw_args, camera_profile, raw, output_tiff, quiet),
        RawEngine::Auto => match run_rawtherapee(rawtherapee, raw, output_tiff, quiet) {
            Ok(()) => Ok(()),
            Err(rt_err) => {
                if !quiet {
                    eprintln!(
                        "rawtherapee failed for {}, falling back to dcraw: {rt_err:#}",
                        raw.display()
                    );
                }
                run_dcraw(dcraw, dcraw_args, camera_profile, raw, output_tiff, quiet)
            }
        },
    }
}

fn run_rawtherapee(rawtherapee: &Path, raw: &Path, output_tiff: &Path, quiet: bool) -> Result<()> {
    if let Some(parent) = output_tiff.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let mut command = Command::new(rawtherapee);
    command
        .arg("-q")
        .arg("-Y")
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

fn run_dcraw(
    dcraw: &Path,
    dcraw_args: &[String],
    camera_profile: Option<&str>,
    raw: &Path,
    output_tiff: &Path,
    quiet: bool,
) -> Result<()> {
    if let Some(parent) = output_tiff.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let output = File::create(output_tiff)
        .with_context(|| format!("creating intermediate {}", output_tiff.display()))?;

    let mut command = Command::new(dcraw);
    command.args(dcraw_args);
    if let Some(profile) = camera_profile {
        command.arg("-p").arg(profile);
    }
    if quiet {
        command.stderr(Stdio::null());
    }
    let status = command
        .arg("-c")
        .arg(raw)
        .stdout(Stdio::from(output))
        .status()
        .with_context(|| format!("running {}", dcraw.display()))?;

    if !status.success() {
        bail!("dcraw failed with status {status}");
    }

    Ok(())
}

pub(crate) fn run_dcraw_convert_final(
    dcraw: &Path,
    dcraw_args: &[String],
    camera_profile: Option<&str>,
    raw: &Path,
    convert: &Path,
    hald: &Path,
    sharpening: SharpeningSettings,
    output: &Path,
    export: &ExportOptions,
) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let mut dcraw_command = Command::new(dcraw);
    dcraw_command.args(dcraw_args);
    if let Some(profile) = camera_profile {
        dcraw_command.arg("-p").arg(profile);
    }
    let mut dcraw_child = dcraw_command
        .arg("-c")
        .arg(raw)
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("running {}", dcraw.display()))?;

    let dcraw_stdout = dcraw_child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to capture dcraw stdout"))?;

    let mut convert_command = Command::new(convert);
    add_convert_thread_limit(&mut convert_command);
    convert_command.arg("tiff:-").arg("-hald-clut").arg(hald);
    add_sharpening_args(&mut convert_command, sharpening);
    add_final_convert_args(&mut convert_command, output, export)?;
    let mut convert_child = convert_command
        .stdin(Stdio::from(dcraw_stdout))
        .spawn()
        .with_context(|| format!("running {}", convert.display()))?;

    let convert_status = convert_child.wait()?;
    let dcraw_status = dcraw_child.wait()?;

    if !dcraw_status.success() {
        bail!("dcraw failed with status {dcraw_status}");
    }
    if !convert_status.success() {
        bail!("convert failed with status {convert_status}");
    }

    Ok(())
}

pub(crate) fn run_convert_depth(
    convert: &Path,
    input_tiff: &Path,
    hald: &Path,
    sharpening: SharpeningSettings,
    output: &Path,
    depth: Option<u8>,
) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let mut command = Command::new(convert);
    add_convert_thread_limit(&mut command);
    command.arg(input_tiff).arg("-hald-clut").arg(hald);
    add_sharpening_args(&mut command, sharpening);
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
