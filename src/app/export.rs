use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
};

use anyhow::{Context, Result, bail};

use crate::app::retouch::{RetouchSettings, normalize_rotation};
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
    finalize_output_with_retouch(convert, input, output, export, None)
}

pub(crate) fn finalize_output_with_retouch(
    convert: &Path,
    input: &Path,
    output: &Path,
    export: &ExportOptions,
    retouch: Option<&RetouchSettings>,
) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let mut command = Command::new(convert);
    add_convert_thread_limit(&mut command, convert);
    command.arg(input);
    add_retouch_geometry_args(&mut command, input, retouch)?;
    add_final_convert_args(&mut command, output, export)?;

    let status = command
        .status()
        .with_context(|| format!("running {}", convert.display()))?;

    if !status.success() {
        bail!("final export failed with status {status}");
    }

    Ok(())
}

fn add_retouch_geometry_args(
    command: &mut Command,
    input: &Path,
    retouch: Option<&RetouchSettings>,
) -> Result<()> {
    let Some(retouch) = retouch else {
        return Ok(());
    };
    let retouch = retouch.clone().normalized();
    let rotation = normalize_rotation(retouch.rotation_degrees);
    let (mut width, mut height) =
        image::image_dimensions(input).with_context(|| format!("reading {}", input.display()))?;
    if rotation != 0.0 {
        command.arg("-background").arg("black");
        command.arg("-rotate").arg(format_geometry_float(rotation));
        if is_quarter_turn(rotation) {
            std::mem::swap(&mut width, &mut height);
        } else if !is_half_turn(rotation) {
            let (auto_width, auto_height) = rotated_auto_crop_dimensions(width, height, rotation);
            command
                .arg("-gravity")
                .arg("Center")
                .arg("-crop")
                .arg(format!("{auto_width}x{auto_height}+0+0"))
                .arg("+repage")
                .arg("-gravity")
                .arg("NorthWest");
            width = auto_width;
            height = auto_height;
        }
    }
    if let Some(crop) = retouch.crop {
        let crop_width = ((crop.width * width as f32).round() as u32).clamp(1, width.max(1));
        let crop_height = ((crop.height * height as f32).round() as u32).clamp(1, height.max(1));
        let max_x = width.saturating_sub(crop_width);
        let max_y = height.saturating_sub(crop_height);
        let x = ((crop.x * width as f32).round() as u32).min(max_x);
        let y = ((crop.y * height as f32).round() as u32).min(max_y);
        command
            .arg("-crop")
            .arg(format!("{crop_width}x{crop_height}+{x}+{y}"))
            .arg("+repage");
    }
    Ok(())
}

fn is_quarter_turn(rotation: f32) -> bool {
    let normalized = normalize_rotation(rotation).abs();
    (normalized - 90.0).abs() < 0.001
}

fn is_half_turn(rotation: f32) -> bool {
    (normalize_rotation(rotation).abs() - 180.0).abs() < 0.001
}

fn rotated_auto_crop_dimensions(width: u32, height: u32, rotation: f32) -> (u32, u32) {
    let width = width.max(1) as f64;
    let height = height.max(1) as f64;
    let radians = (normalize_rotation(rotation).abs() as f64).to_radians();
    let sin_a = radians.sin().abs();
    let cos_a = radians.cos().abs();
    if sin_a <= f64::EPSILON || cos_a <= f64::EPSILON {
        return (width.round() as u32, height.round() as u32);
    }

    let (long_side, short_side) = if width >= height {
        (width, height)
    } else {
        (height, width)
    };
    let (auto_width, auto_height) =
        if short_side <= 2.0 * sin_a * cos_a * long_side || (sin_a - cos_a).abs() < f64::EPSILON {
            let side = 0.5 * short_side;
            if width >= height {
                (side / sin_a, side / cos_a)
            } else {
                (side / cos_a, side / sin_a)
            }
        } else {
            let cos_2a = cos_a * cos_a - sin_a * sin_a;
            (
                (width * cos_a - height * sin_a) / cos_2a,
                (height * cos_a - width * sin_a) / cos_2a,
            )
        };

    (
        auto_width.floor().max(1.0).min(width).round() as u32,
        auto_height.floor().max(1.0).min(height).round() as u32,
    )
}

fn format_geometry_float(value: f32) -> String {
    format!("{value:.3}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

/// Append final output arguments for JPEG or TIFF export.
///
/// Resize options are added first so the encoder sees the final image geometry.
/// JPEG outputs are forced to 8-bit, then receive metadata stripping,
/// progressive/interlace mode, chroma subsampling, and quality settings. TIFF
/// outputs keep the upstream 16-bit depth, use TIFF Zip/Deflate compression,
/// and only strip metadata when explicitly requested.
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
            .arg("-colorspace")
            .arg("sRGB")
            .arg("-depth")
            .arg("8")
            .arg("-sampling-factor")
            .arg(export.jpeg_subsampling.graphicsmagick_sampling_factor())
            .arg("-quality")
            .arg(export.jpg_quality.clamp(1, 100).to_string());
    } else {
        if export.strip_metadata {
            command.arg("-strip");
        }
        command.arg("-compress").arg("Zip");
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
pub(crate) fn add_convert_thread_limit(command: &mut Command, convert: &Path) {
    if convert_supports_threads_limit(convert) {
        command
            .arg("-limit")
            .arg("Threads")
            .arg(cpu_thread_count().to_string());
    }
}

fn thread_support_cache() -> &'static Mutex<HashMap<PathBuf, bool>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, bool>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn convert_supports_threads_limit(convert: &Path) -> bool {
    let cache = thread_support_cache();

    if let Ok(map) = cache.lock()
        && let Some(supported) = map.get(convert).copied()
    {
        return supported;
    }

    let supported = detect_convert_threads_limit(convert);
    if let Ok(mut map) = cache.lock() {
        map.insert(convert.to_path_buf(), supported);
    }
    supported
}

fn detect_convert_threads_limit(convert: &Path) -> bool {
    let output = Command::new(convert)
        .arg("-list")
        .arg("resource")
        .output()
        .ok();

    let Some(output) = output else {
        return false;
    };

    let listing = String::from_utf8_lossy(&output.stdout);
    let errors = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{listing}\n{errors}");

    is_threads_present_in_resource_output(&combined)
}

fn is_threads_present_in_resource_output(text: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim_start();
        let lower = trimmed.to_ascii_lowercase();
        lower.starts_with("threads")
            || trimmed
                .split_once(':')
                .is_some_and(|(key, _)| key.trim().eq_ignore_ascii_case("threads"))
    })
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
    use crate::app::retouch::RetouchSettings;
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
    fn arbitrary_rotation_auto_crops_before_user_crop() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("input.png");
        image::RgbImage::new(400, 300).save(&input).unwrap();
        let mut command = Command::new("convert");
        let retouch = RetouchSettings {
            rotation_degrees: 5.0,
            ..Default::default()
        };

        add_retouch_geometry_args(&mut command, &input, Some(&retouch)).unwrap();

        let args = command_args(&command);
        assert_eq!(
            &args[..8],
            [
                "-background",
                "black",
                "-rotate",
                "5",
                "-gravity",
                "Center",
                "-crop",
                "378x268+0+0",
            ]
        );
        assert_eq!(&args[8..], ["+repage", "-gravity", "NorthWest"]);
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
    fn tiff_export_keeps_depth_compresses_and_accepts_structured_resize() {
        let mut command = Command::new("convert");
        let mut export = export_options();
        export.max_width = Some(3000);
        export.max_height = Some(2000);
        export.strip_metadata = true;

        add_final_convert_args(&mut command, Path::new("out.tif"), &export).unwrap();

        assert_eq!(
            command_args(&command),
            [
                "-resize",
                "3000x2000>",
                "-strip",
                "-compress",
                "Zip",
                "out.tif"
            ]
        );
    }

    #[test]
    fn detects_threads_resource_from_convert_listing_output() {
        let listing = r#"
Resource limits:
  Width: 4096
  Height: 4096
  Area: 512M
  Threads: 16
  Time: 2.0
"#;
        assert!(is_threads_present_in_resource_output(listing));
    }

    #[test]
    fn ignores_threads_resource_if_not_present() {
        let listing = r#"
Resource limits:
  Width: 4096
  Height: 4096
  Area: 512M
"#;
        assert!(!is_threads_present_in_resource_output(listing));
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
