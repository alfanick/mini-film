use std::{fs, path::Path, process::Command};

use anyhow::{Context, Result, bail};
use mini_film::SharpeningSettings;

use crate::app::util::cpu_thread_count;
use crate::cli::ExportOptions;

pub(crate) fn add_sharpening_args(command: &mut Command, sharpening: SharpeningSettings) {
    if !sharpening.is_enabled() {
        return;
    }

    let radius = sharpening.radius.clamp(0.1, 3.0);
    let sigma = (radius * (0.65 + sharpening.detail.clamp(0.0, 100.0) / 250.0)).clamp(0.1, 3.5);
    let amount = (sharpening.amount.clamp(0.0, 150.0) / 100.0).clamp(0.0, 1.5);
    let threshold = (sharpening.masking.clamp(0.0, 100.0) / 1000.0).clamp(0.0, 0.1);
    command
        .arg("-unsharp")
        .arg(format!("{radius:.2}x{sigma:.2}+{amount:.2}+{threshold:.3}"));
}

pub(crate) fn finalize_output(
    convert: &Path,
    input: &Path,
    output: &Path,
    export: &ExportOptions,
) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let mut command = Command::new(convert);
    add_convert_thread_limit(&mut command);
    command.arg(input);
    add_final_convert_args(&mut command, output, export)?;

    let status = command
        .status()
        .with_context(|| format!("running {}", convert.display()))?;

    if !status.success() {
        bail!("final export failed with status {status}");
    }

    Ok(())
}

pub(crate) fn add_final_convert_args(
    command: &mut Command,
    output: &Path,
    export: &ExportOptions,
) -> Result<()> {
    let ext = output_ext(output)?;
    add_resize_args(command, export);

    if ext == "jpg" || ext == "jpeg" {
        if export.strip_metadata {
            command.arg("-strip");
        }
        if export.progressive_jpeg {
            command.arg("-interlace").arg("Line");
        }
        command
            .arg("-depth")
            .arg("8")
            .arg("-sampling-factor")
            .arg(export.jpeg_subsampling.graphicsmagick_sampling_factor())
            .arg("-quality")
            .arg(export.jpg_quality.clamp(1, 100).to_string());
    } else if export.strip_metadata {
        command.arg("-strip");
    }

    command.arg(output);
    Ok(())
}

fn add_resize_args(command: &mut Command, export: &ExportOptions) {
    let geometry = if let Some(resize) = &export.resize {
        Some(resize.clone())
    } else if let Some(long_edge) = export.long_edge {
        Some(format!("{long_edge}x{long_edge}>"))
    } else {
        match (export.max_width, export.max_height) {
            (Some(width), Some(height)) => Some(format!("{width}x{height}>")),
            (Some(width), None) => Some(format!("{width}x>")),
            (None, Some(height)) => Some(format!("x{height}>")),
            (None, None) => None,
        }
    };

    if let Some(geometry) = geometry {
        command.arg("-resize").arg(geometry);
    }
}

pub(crate) fn add_convert_thread_limit(command: &mut Command) {
    command
        .arg("-limit")
        .arg("Threads")
        .arg(cpu_thread_count().to_string());
}

pub(crate) fn validate_export_options(export: &ExportOptions) -> Result<()> {
    if export.resize.is_some()
        && (export.long_edge.is_some() || export.max_width.is_some() || export.max_height.is_some())
    {
        bail!("use either --resize or --long-edge/--max-width/--max-height");
    }
    if export.long_edge.is_some() && (export.max_width.is_some() || export.max_height.is_some()) {
        bail!("use either --long-edge or --max-width/--max-height");
    }
    for (name, value) in [
        ("--long-edge", export.long_edge),
        ("--max-width", export.max_width),
        ("--max-height", export.max_height),
    ] {
        if value == Some(0) {
            bail!("{name} must be greater than zero");
        }
    }
    if export.resize.as_deref().is_some_and(str::is_empty) {
        bail!("--resize must not be empty");
    }
    Ok(())
}

pub(crate) fn validate_output_format(output: &Path) -> Result<()> {
    match output_ext(output)?.as_str() {
        "tif" | "tiff" | "jpg" | "jpeg" => Ok(()),
        ext => bail!("unsupported output extension .{ext}; use .tif/.tiff or .jpg/.jpeg"),
    }
}

pub(crate) fn output_ext(output: &Path) -> Result<String> {
    output
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .ok_or_else(|| anyhow::anyhow!("output path must have .tif/.tiff or .jpg/.jpeg extension"))
}
