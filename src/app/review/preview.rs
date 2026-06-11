use super::prelude::*;
use crate::app::export::add_convert_thread_limit;

pub(super) fn extract_embedded_preview(raw: &Path, output: &Path, convert: &Path) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
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
