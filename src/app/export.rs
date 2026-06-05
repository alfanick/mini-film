use std::{fs, path::Path, process::Command};

use anyhow::{Context, Result, bail};

use crate::app::util::cpu_thread_count;
use crate::cli::ExportOptions;

/// Run the final convert invocation that writes the user-facing output.
///
/// Earlier stages may produce a TIFF, PPM, or grained temporary file. This
/// function creates the destination directory, applies thread limits, appends the
/// shared final export options, and checks convert's exit status so failed
/// encodes do not look like successful pipeline completion.
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

/// Append final output arguments for JPEG or TIFF export.
///
/// Resize options are added first so the encoder sees the final image geometry.
/// JPEG outputs are forced to 8-bit, then receive metadata stripping,
/// progressive/interlace mode, chroma subsampling, and quality settings. TIFF
/// outputs keep the upstream 16-bit depth unless the caller explicitly selected
/// metadata stripping.
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

/// Append one convert resize geometry derived from export options.
///
/// The CLI accepts raw GraphicsMagick geometry, longest-edge limiting, or
/// separate max width/height controls. This helper chooses exactly one geometry
/// string and uses the `>` suffix for bounding modes so convert only downsizes
/// images that exceed the requested limit.
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

/// Limit convert's worker threads to the available CPU count.
///
/// ImageMagick/GraphicsMagick can use multiple threads internally. Passing the
/// explicit limit prevents accidental oversubscription when Rayon or batch
/// progress work is also active, while still letting convert use all logical
/// CPUs available on the machine.
pub(crate) fn add_convert_thread_limit(command: &mut Command) {
    command
        .arg("-limit")
        .arg("Threads")
        .arg(cpu_thread_count().to_string());
}

/// Validate mutually exclusive export sizing options.
///
/// Raw `--resize` geometry and structured max-size flags express the same
/// operation in different forms, so mixing them would create surprising convert
/// argument order. This check rejects ambiguous combinations, zero dimensions,
/// and empty geometry before any RAW work starts.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::JpegSubsampling;

    fn export_options() -> ExportOptions {
        ExportOptions {
            jpg_quality: 91,
            resize: None,
            long_edge: None,
            max_width: None,
            max_height: None,
            jpeg_subsampling: JpegSubsampling::S422,
            strip_metadata: false,
            progressive_jpeg: false,
        }
    }

    fn command_args(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect()
    }

    #[test]
    fn jpeg_export_args_force_8bit_quality_and_sampling() {
        let mut command = Command::new("convert");
        let mut export = export_options();
        export.long_edge = Some(2048);
        export.strip_metadata = true;
        export.progressive_jpeg = true;

        add_final_convert_args(&mut command, Path::new("out.jpg"), &export).unwrap();

        let args = command_args(&command);
        assert_eq!(
            args,
            [
                "-resize",
                "2048x2048>",
                "-strip",
                "-interlace",
                "Line",
                "-depth",
                "8",
                "-sampling-factor",
                "2x1,1x1,1x1",
                "-quality",
                "91",
                "out.jpg",
            ]
        );
    }

    #[test]
    fn tiff_export_keeps_depth_and_accepts_structured_resize() {
        let mut command = Command::new("convert");
        let mut export = export_options();
        export.max_width = Some(3000);
        export.max_height = Some(2000);
        export.strip_metadata = true;

        add_final_convert_args(&mut command, Path::new("out.tif"), &export).unwrap();

        assert_eq!(
            command_args(&command),
            ["-resize", "3000x2000>", "-strip", "out.tif"]
        );
    }

    #[test]
    fn validate_export_options_rejects_ambiguous_or_zero_resize() {
        let mut export = export_options();
        export.resize = Some("3000x3000>".to_string());
        export.long_edge = Some(3000);
        assert!(validate_export_options(&export).is_err());

        let mut export = export_options();
        export.max_width = Some(0);
        assert!(validate_export_options(&export).is_err());

        let mut export = export_options();
        export.resize = Some(String::new());
        assert!(validate_export_options(&export).is_err());
    }

    #[test]
    fn output_extension_validation_is_case_insensitive() {
        assert_eq!(output_ext(Path::new("x.JPG")).unwrap(), "jpg");
        assert!(validate_output_format(Path::new("x.TIFF")).is_ok());
        assert!(validate_output_format(Path::new("x.png")).is_err());
        assert!(output_ext(Path::new("no-extension")).is_err());
    }
}
