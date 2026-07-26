use super::*;
use crate::app::managed_symlink::ensure_file_symlink;

const SQLITE_COMPANION_SUFFIXES: [&str; 3] = ["-journal", "-wal", "-shm"];

#[derive(Debug)]
pub(super) struct ReviewCatalogLocation {
    catalog_path: PathBuf,
    output_link_path: PathBuf,
}

impl ReviewCatalogLocation {
    pub(super) fn prepare(input_root: &Path, output_root: &Path) -> Result<Self> {
        let location = Self {
            catalog_path: input_root.join(SQLITE_STATE_FILE),
            output_link_path: output_root.join(SQLITE_STATE_FILE),
        };
        if location.catalog_path == location.output_link_path {
            regular_file_exists(&location.catalog_path)?;
            return Ok(location);
        }

        let catalog_exists = regular_file_exists(&location.catalog_path)?;
        if !catalog_exists && has_sqlite_companions(&location.catalog_path)? {
            bail!(
                "review catalog is missing but SQLite companion files remain beside {}",
                location.catalog_path.display()
            );
        }

        match catalog_entry(&location.output_link_path)? {
            CatalogEntry::RegularFile => {
                migrate_catalog(&location.output_link_path, &location.catalog_path)?;
            }
            CatalogEntry::Symlink if !catalog_exists => {
                let target = fs::read_link(&location.output_link_path).with_context(|| {
                    format!(
                        "reading review catalog symlink {}",
                        location.output_link_path.display()
                    )
                })?;
                bail!(
                    "review catalog is missing from input folder {}; output symlink {} still points to {}",
                    input_root.display(),
                    location.output_link_path.display(),
                    target.display()
                );
            }
            CatalogEntry::Missing if !catalog_exists => {
                if has_sqlite_companions(&location.output_link_path)? {
                    bail!(
                        "review catalog is missing but SQLite companion files remain beside {}",
                        location.output_link_path.display()
                    );
                }
            }
            CatalogEntry::Missing | CatalogEntry::Symlink => {}
        }

        if regular_file_exists(&location.catalog_path)? {
            migrate_sqlite_companions(&location.output_link_path, &location.catalog_path)?;
        }
        Ok(location)
    }

    pub(super) fn catalog_path(&self) -> &Path {
        &self.catalog_path
    }

    pub(super) fn ensure_output_link(&self) -> Result<()> {
        if self.catalog_path == self.output_link_path {
            return Ok(());
        }
        if !regular_file_exists(&self.catalog_path)? {
            bail!(
                "review catalog was not created at {}",
                self.catalog_path.display()
            );
        }
        ensure_file_symlink(&self.catalog_path, &self.output_link_path, false)?;
        Ok(())
    }
}

enum CatalogEntry {
    Missing,
    RegularFile,
    Symlink,
}

fn catalog_entry(path: &Path) -> Result<CatalogEntry> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Ok(CatalogEntry::Symlink),
        Ok(metadata) if metadata.is_file() => Ok(CatalogEntry::RegularFile),
        Ok(_) => bail!("review catalog path is not a file: {}", path.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(CatalogEntry::Missing),
        Err(error) => {
            Err(error).with_context(|| format!("inspecting review catalog {}", path.display()))
        }
    }
}

fn regular_file_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(true),
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "input review catalog must be a regular file, not a symlink: {}",
                path.display()
            )
        }
        Ok(_) => bail!("review catalog path is not a file: {}", path.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => {
            Err(error).with_context(|| format!("inspecting review catalog {}", path.display()))
        }
    }
}

fn migrate_catalog(source: &Path, destination: &Path) -> Result<()> {
    if source == destination {
        return Ok(());
    }
    if !regular_file_exists(source)? {
        bail!("review catalog is missing: {}", source.display());
    }
    let destination_exists = regular_file_exists(destination)?;
    if destination_exists && !files_equal(source, destination)? {
        bail!(
            "refusing to overwrite conflicting review catalog {}; existing output catalog remains at {}",
            destination.display(),
            source.display()
        );
    }
    verify_companion_conflicts(source, destination, destination_exists)?;

    if !destination_exists {
        move_file(source, destination).with_context(|| {
            format!(
                "moving review catalog {} to {}",
                source.display(),
                destination.display()
            )
        })?;
    }
    migrate_sqlite_companions(source, destination)?;
    if regular_file_exists(source)? {
        fs::remove_file(source)
            .with_context(|| format!("removing migrated review catalog {}", source.display()))?;
    }
    Ok(())
}

fn verify_companion_conflicts(
    source: &Path,
    destination: &Path,
    destination_catalog_exists: bool,
) -> Result<()> {
    for suffix in SQLITE_COMPANION_SUFFIXES {
        let source_companion = sqlite_companion_path(source, suffix);
        let destination_companion = sqlite_companion_path(destination, suffix);
        let source_exists = regular_file_exists(&source_companion)?;
        let destination_exists = regular_file_exists(&destination_companion)?;
        if source_exists
            && destination_exists
            && !files_equal(&source_companion, &destination_companion)?
        {
            bail!(
                "refusing to overwrite conflicting SQLite companion {}",
                destination_companion.display()
            );
        }
        if !destination_catalog_exists && !source_exists && destination_exists {
            bail!(
                "refusing to combine review catalog with orphan SQLite companion {}",
                destination_companion.display()
            );
        }
    }
    Ok(())
}

fn migrate_sqlite_companions(source: &Path, destination: &Path) -> Result<()> {
    for suffix in SQLITE_COMPANION_SUFFIXES {
        let source_companion = sqlite_companion_path(source, suffix);
        if !regular_file_exists(&source_companion)? {
            continue;
        }
        let destination_companion = sqlite_companion_path(destination, suffix);
        if regular_file_exists(&destination_companion)? {
            if !files_equal(&source_companion, &destination_companion)? {
                bail!(
                    "refusing to overwrite conflicting SQLite companion {}",
                    destination_companion.display()
                );
            }
            fs::remove_file(&source_companion).with_context(|| {
                format!(
                    "removing duplicate SQLite companion {}",
                    source_companion.display()
                )
            })?;
        } else {
            move_file(&source_companion, &destination_companion).with_context(|| {
                format!(
                    "moving SQLite companion {} to {}",
                    source_companion.display(),
                    destination_companion.display()
                )
            })?;
        }
    }
    Ok(())
}

fn has_sqlite_companions(database: &Path) -> Result<bool> {
    for suffix in SQLITE_COMPANION_SUFFIXES {
        if regular_file_exists(&sqlite_companion_path(database, suffix))? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn sqlite_companion_path(database: &Path, suffix: &str) -> PathBuf {
    let mut path = database.as_os_str().to_os_string();
    path.push(suffix);
    PathBuf::from(path)
}

fn files_equal(left: &Path, right: &Path) -> Result<bool> {
    if fs::metadata(left)?.len() != fs::metadata(right)?.len() {
        return Ok(false);
    }
    let mut left = BufReader::new(
        fs::File::open(left).with_context(|| format!("opening {}", left.display()))?,
    );
    let mut right = BufReader::new(
        fs::File::open(right).with_context(|| format!("opening {}", right.display()))?,
    );
    let mut left_buffer = [0u8; 64 * 1024];
    let mut right_buffer = [0u8; 64 * 1024];
    loop {
        let left_read = left.read(&mut left_buffer)?;
        let right_read = right.read(&mut right_buffer)?;
        if left_read != right_read || left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}

fn move_file(source: &Path, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    match fs::rename(source, destination) {
        Ok(()) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {}
        Err(error) => return Err(error.into()),
    }

    let parent = destination.parent().ok_or_else(|| {
        anyhow!(
            "review catalog destination has no parent: {}",
            destination.display()
        )
    })?;
    let mut source_file =
        fs::File::open(source).with_context(|| format!("opening {}", source.display()))?;
    let source_permissions = source_file.metadata()?.permissions();
    let mut temporary = Builder::new()
        .prefix(".mini-film-catalog-migration-")
        .tempfile_in(parent)
        .with_context(|| format!("creating catalog migration file in {}", parent.display()))?;
    std::io::copy(&mut source_file, temporary.as_file_mut())
        .with_context(|| format!("copying {}", source.display()))?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("syncing migrated catalog {}", destination.display()))?;
    fs::set_permissions(temporary.path(), source_permissions)
        .with_context(|| format!("preserving permissions for {}", destination.display()))?;
    temporary
        .persist(destination)
        .map_err(|error| error.error)
        .with_context(|| format!("installing migrated catalog {}", destination.display()))?;
    fs::remove_file(source)
        .with_context(|| format!("removing migrated source {}", source.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_catalog_and_companions_move_to_input() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("input");
        let output = temp.path().join("output");
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&output).unwrap();
        let old_catalog = output.join(SQLITE_STATE_FILE);
        let old_wal = sqlite_companion_path(&old_catalog, "-wal");
        fs::write(&old_catalog, b"catalog").unwrap();
        fs::write(&old_wal, b"wal").unwrap();

        let location = ReviewCatalogLocation::prepare(&input, &output).unwrap();
        let new_catalog = input.join(SQLITE_STATE_FILE);
        let new_wal = sqlite_companion_path(&new_catalog, "-wal");
        assert_eq!(fs::read(&new_catalog).unwrap(), b"catalog");
        assert_eq!(fs::read(&new_wal).unwrap(), b"wal");
        assert!(!old_catalog.exists());
        assert!(!old_wal.exists());

        location.ensure_output_link().unwrap();
        assert!(
            fs::symlink_metadata(&old_catalog)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::canonicalize(&old_catalog).unwrap(),
            fs::canonicalize(&new_catalog).unwrap()
        );
    }

    #[test]
    fn conflicting_catalogs_are_preserved_and_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("input");
        let output = temp.path().join("output");
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&output).unwrap();
        let input_catalog = input.join(SQLITE_STATE_FILE);
        let output_catalog = output.join(SQLITE_STATE_FILE);
        fs::write(&input_catalog, b"input catalog").unwrap();
        fs::write(&output_catalog, b"output catalog").unwrap();

        let error = ReviewCatalogLocation::prepare(&input, &output).unwrap_err();

        assert!(format!("{error:#}").contains("conflicting review catalog"));
        assert_eq!(fs::read(input_catalog).unwrap(), b"input catalog");
        assert_eq!(fs::read(output_catalog).unwrap(), b"output catalog");
    }
}
