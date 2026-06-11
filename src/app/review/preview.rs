use super::prelude::*;

pub(super) fn extract_embedded_preview(raw: &Path, output: &Path) -> Result<()> {
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
        fs::rename(&temp, output)
            .with_context(|| format!("renaming {} to {}", temp.display(), output.display()))?;
        return Ok(());
    }

    bail!("no embedded JPEG preview found in {}", raw.display())
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
