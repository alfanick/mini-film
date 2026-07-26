use std::{fs, path::Path};

use anyhow::{Context, Result, bail};
use tempfile::Builder;

pub(crate) fn ensure_file_symlink(
    source: &Path,
    destination: &Path,
    replace_regular_file: bool,
) -> Result<bool> {
    ensure_symlink(source, destination, LinkKind::File, replace_regular_file)
}

pub(crate) fn ensure_directory_symlink(source: &Path, destination: &Path) -> Result<bool> {
    ensure_symlink(source, destination, LinkKind::Directory, false)
}

#[derive(Clone, Copy)]
enum LinkKind {
    File,
    Directory,
}

fn ensure_symlink(
    source: &Path,
    destination: &Path,
    kind: LinkKind,
    replace_regular_file: bool,
) -> Result<bool> {
    if source == destination {
        return Ok(false);
    }
    let source = fs::canonicalize(source)
        .with_context(|| format!("canonicalizing symlink source {}", source.display()))?;
    match kind {
        LinkKind::File if !source.is_file() => {
            bail!("symlink source is not a file: {}", source.display())
        }
        LinkKind::Directory if !source.is_dir() => {
            bail!("symlink source is not a directory: {}", source.display())
        }
        _ => {}
    }

    let existing = match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if fs::canonicalize(destination).ok().as_deref() == Some(source.as_path()) {
                return Ok(false);
            }
            ExistingDestination::Symlink
        }
        Ok(metadata) if metadata.is_file() && replace_regular_file => {
            ExistingDestination::RegularFile
        }
        Ok(metadata) => {
            bail!(
                "refusing to replace non-symlink {} at {}",
                if metadata.is_dir() {
                    "directory"
                } else {
                    "path"
                },
                destination.display()
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ExistingDestination::Missing,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspecting symlink {}", destination.display()));
        }
    };

    let parent = destination.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "managed symlink has no parent directory: {}",
            destination.display()
        )
    })?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let placeholder = Builder::new()
        .prefix(".mini-film-link-")
        .tempfile_in(parent)
        .with_context(|| format!("reserving managed symlink in {}", parent.display()))?;
    let temporary_link = placeholder.path().to_path_buf();
    placeholder
        .close()
        .with_context(|| format!("preparing managed symlink {}", temporary_link.display()))?;
    create_symlink(&source, &temporary_link, kind).with_context(|| {
        format!(
            "creating managed symlink {} -> {}",
            temporary_link.display(),
            source.display()
        )
    })?;

    #[cfg(windows)]
    if !matches!(existing, ExistingDestination::Missing) {
        fs::remove_file(destination)
            .with_context(|| format!("removing stale symlink {}", destination.display()))?;
    }

    if let Err(error) = fs::rename(&temporary_link, destination) {
        fs::remove_file(&temporary_link).ok();
        return Err(error).with_context(|| {
            format!(
                "installing managed symlink {} -> {}",
                destination.display(),
                source.display()
            )
        });
    }
    if fs::canonicalize(destination).ok().as_deref() != Some(source.as_path()) {
        bail!(
            "managed symlink {} does not resolve to {}",
            destination.display(),
            source.display()
        );
    }
    let _ = existing;
    Ok(true)
}

enum ExistingDestination {
    Missing,
    Symlink,
    RegularFile,
}

#[cfg(unix)]
fn create_symlink(source: &Path, destination: &Path, _kind: LinkKind) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, destination)
}

#[cfg(windows)]
fn create_symlink(source: &Path, destination: &Path, kind: LinkKind) -> std::io::Result<()> {
    match kind {
        LinkKind::File => std::os::windows::fs::symlink_file(source, destination),
        LinkKind::Directory => std::os::windows::fs::symlink_dir(source, destination),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_link_replaces_generated_file_and_repairs_moved_source() {
        let temp = tempfile::tempdir().unwrap();
        let old_input = temp.path().join("old-input");
        let new_input = temp.path().join("new-input");
        let output = temp.path().join("output");
        fs::create_dir_all(&old_input).unwrap();
        fs::create_dir_all(&output).unwrap();
        let old_source = old_input.join("frame.jpg");
        let destination = output.join("frame.jpg");
        fs::write(&old_source, b"original").unwrap();
        fs::write(&destination, b"generated").unwrap();

        assert!(ensure_file_symlink(&old_source, &destination, true).unwrap());
        assert_eq!(
            fs::canonicalize(&destination).unwrap(),
            fs::canonicalize(&old_source).unwrap()
        );

        fs::rename(&old_input, &new_input).unwrap();
        let new_source = new_input.join("frame.jpg");
        assert!(ensure_file_symlink(&new_source, &destination, true).unwrap());
        assert_eq!(
            fs::canonicalize(&destination).unwrap(),
            fs::canonicalize(&new_source).unwrap()
        );
    }

    #[test]
    fn directory_link_never_replaces_a_real_directory() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("input");
        let destination = temp.path().join("output/originals");
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&destination).unwrap();

        assert!(ensure_directory_symlink(&input, &destination).is_err());
        assert!(destination.is_dir());
    }

    #[test]
    fn directory_link_repairs_after_source_moves() {
        let temp = tempfile::tempdir().unwrap();
        let old_input = temp.path().join("old-input");
        let new_input = temp.path().join("new-input");
        let destination = temp.path().join("output/originals");
        fs::create_dir_all(&old_input).unwrap();

        assert!(ensure_directory_symlink(&old_input, &destination).unwrap());
        fs::rename(&old_input, &new_input).unwrap();
        assert!(ensure_directory_symlink(&new_input, &destination).unwrap());
        assert_eq!(
            fs::canonicalize(&destination).unwrap(),
            fs::canonicalize(&new_input).unwrap()
        );
    }
}
