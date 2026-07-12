use super::prelude::*;
use crate::app::export::{add_convert_thread_limit, add_convert_thread_limit_with_count};
use crate::app::util::{half_cpu_thread_count, is_jpeg_input_file};

pub(super) const COMPRESSED_REVIEW_CACHE_VERSION: &str = "compressed-v1";
pub(super) const COMPRESSED_REVIEW_THUMBNAIL_LONG_EDGE: u32 = 512;
pub(super) const COMPRESSED_REVIEW_THUMBNAIL_QUALITY: u8 = 55;
pub(super) const COMPRESSED_REVIEW_PREVIEW_LONG_EDGE: u32 = 2048;
pub(super) const COMPRESSED_REVIEW_PREVIEW_QUALITY: u8 = 82;

pub(super) fn extract_embedded_preview(raw: &Path, output: &Path, convert: &Path) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    if is_jpeg_input_file(raw) {
        return auto_orient_preview(convert, raw, output);
    }

    for tag in ["PreviewImage", "JpgFromRaw", "OtherImage", "ThumbnailImage"] {
        let result = Command::new("exiftool")
            .arg("-b")
            .arg(format!("-{tag}"))
            .arg(raw)
            .output()
            .with_context(|| format!("extracting {tag} from {}", raw.display()))?;
        if !result.status.success() || !looks_like_jpeg(&result.stdout) {
            continue;
        }

        let temp = output.with_extension("jpg.tmp");
        fs::write(&temp, &result.stdout).with_context(|| format!("writing {}", temp.display()))?;
        copy_raw_orientation(raw, &temp);
        auto_orient_preview(convert, &temp, output)?;
        let _ = fs::remove_file(&temp);
        return Ok(());
    }

    bail!("no embedded JPEG preview found in {}", raw.display())
}

fn copy_raw_orientation(raw: &Path, preview: &Path) {
    let _ = Command::new("exiftool")
        .args(["-q", "-q", "-overwrite_original", "-TagsFromFile"])
        .arg(raw)
        .arg("-Orientation")
        .arg(preview)
        .status();
}

fn auto_orient_preview(convert: &Path, input: &Path, output: &Path) -> Result<()> {
    let mut command = Command::new(convert);
    add_convert_thread_limit(&mut command, convert);
    let result = command
        .arg(input)
        .arg("-auto-orient")
        .arg(output)
        .output()
        .with_context(|| format!("auto-orienting preview with {}", convert.display()))?;
    if !result.status.success() {
        bail!(
            "preview auto-orient failed with status {}\nstderr:\n{}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        );
    }
    Ok(())
}

pub(super) fn ensure_compressed_review_thumbnail(
    source: &Path,
    output: &Path,
    convert: &Path,
) -> Result<()> {
    ensure_compressed_review_derivative(
        source,
        output,
        convert,
        COMPRESSED_REVIEW_THUMBNAIL_LONG_EDGE,
        COMPRESSED_REVIEW_THUMBNAIL_QUALITY,
    )
}

pub(super) fn ensure_compressed_review_preview(
    source: &Path,
    output: &Path,
    convert: &Path,
) -> Result<()> {
    ensure_compressed_review_derivative(
        source,
        output,
        convert,
        COMPRESSED_REVIEW_PREVIEW_LONG_EDGE,
        COMPRESSED_REVIEW_PREVIEW_QUALITY,
    )
}

fn ensure_compressed_review_derivative(
    source: &Path,
    output: &Path,
    convert: &Path,
    long_edge: u32,
    quality: u8,
) -> Result<()> {
    if output.is_file() {
        return Ok(());
    }
    write_compressed_review_derivative(convert, source, output, long_edge, quality)
}

fn write_compressed_review_derivative(
    convert: &Path,
    source: &Path,
    output: &Path,
    long_edge: u32,
    quality: u8,
) -> Result<()> {
    let parent = output
        .parent()
        .ok_or_else(|| anyhow!("review derivative has no parent: {}", output.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let temp = Builder::new()
        .prefix(".mini-film-review-")
        .suffix(".jpg")
        .tempfile_in(parent)
        .with_context(|| format!("creating temporary review image in {}", parent.display()))?;
    let temp = temp.into_temp_path();

    let mut command = Command::new(convert);
    add_convert_thread_limit_with_count(&mut command, convert, half_cpu_thread_count());
    add_compressed_review_derivative_args(&mut command, source, temp.as_ref(), long_edge, quality);
    let result = command
        .output()
        .with_context(|| format!("creating review image with {}", convert.display()))?;
    if !result.status.success() {
        bail!(
            "review image conversion failed with status {}\nstderr:\n{}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let temp_path: &Path = temp.as_ref();
    fs::rename(temp_path, output)
        .with_context(|| format!("moving review image to {}", output.display()))?;
    Ok(())
}

fn add_compressed_review_derivative_args(
    command: &mut Command,
    source: &Path,
    output: &Path,
    long_edge: u32,
    quality: u8,
) {
    command
        .arg("-define")
        .arg(format!("jpeg:size={long_edge}x{long_edge}"))
        .arg(source)
        .arg("-auto-orient")
        .arg("-filter")
        .arg("Triangle")
        .arg("-resize")
        .arg(format!("{long_edge}x{long_edge}>"))
        .arg("-strip")
        .arg("-interlace")
        .arg("Line")
        .arg("-depth")
        .arg("8")
        .arg("-sampling-factor")
        .arg("2x2,1x1,1x1")
        .arg("-quality")
        .arg(quality.clamp(1, 100).to_string())
        .arg(output);
}

pub(super) fn compressed_review_cache_file_name(source: &Path, image_id: u64) -> String {
    let mut hasher = Sha1::new();
    hasher.update(source.to_string_lossy().as_bytes());
    if let Ok(metadata) = fs::metadata(source) {
        hasher.update(metadata.len().to_le_bytes());
        if let Ok(modified) = metadata.modified()
            && let Ok(elapsed) = modified.duration_since(std::time::UNIX_EPOCH)
        {
            hasher.update(elapsed.as_nanos().to_le_bytes());
        }
    }
    let digest = hasher.finalize();
    let fingerprint = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{image_id:08}-{fingerprint}.jpg")
}

pub(super) fn looks_like_jpeg(bytes: &[u8]) -> bool {
    bytes.len() > 3 && bytes[0] == 0xff && bytes[1] == 0xd8 && bytes[2] == 0xff
}

pub(super) fn short_path_sha1(path: &Path) -> String {
    let mut hasher = Sha1::new();
    hasher.update(path.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    fn command_args(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn compressed_review_derivative_is_progressive_bounded_and_stripped() {
        let mut command = Command::new("convert");
        add_compressed_review_derivative_args(
            &mut command,
            Path::new("input.jpg"),
            Path::new("preview.jpg"),
            COMPRESSED_REVIEW_PREVIEW_LONG_EDGE,
            COMPRESSED_REVIEW_PREVIEW_QUALITY,
        );

        assert_eq!(
            command_args(&command),
            [
                "-define",
                "jpeg:size=2048x2048",
                "input.jpg",
                "-auto-orient",
                "-filter",
                "Triangle",
                "-resize",
                "2048x2048>",
                "-strip",
                "-interlace",
                "Line",
                "-depth",
                "8",
                "-sampling-factor",
                "2x2,1x1,1x1",
                "-quality",
                "82",
                "preview.jpg",
            ]
        );
    }

    #[test]
    fn compressed_review_cache_name_changes_with_source_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("frame.jpg");
        fs::write(&source, b"one").unwrap();
        let first = compressed_review_cache_file_name(&source, 7);
        fs::write(&source, b"longer source").unwrap();
        let second = compressed_review_cache_file_name(&source, 7);

        assert!(first.starts_with("00000007-"));
        assert!(first.ends_with(".jpg"));
        assert_ne!(first, second);
    }
}
