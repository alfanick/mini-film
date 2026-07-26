use super::{entities::*, *};
use crate::app::cache::{
    CACHE_STORAGE_NAMESPACE, LEGACY_GALLERY_THUMBNAILS_DIR, LEGACY_OUTPUT_CACHE_DIR,
    MIGRATION_CONFLICT_CACHE_DIR, RETOUCH_CACHE_DIR, cache_storage_path, is_cache_directory_name,
    is_cache_relative_path,
};
use filetime::{FileTime, set_file_times};
use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel, Set, TransactionTrait};
use walkdir::WalkDir;

const CACHE_ROOT_PREFIX: &str = "mini-film.";
const RETOUCH_CACHE_MARKER: &str = ".retouch-cache-";
const RETOUCH_TEMP_MARKER: &str = ".retouch-";

pub(super) async fn resolve_cache_root(connection: &DatabaseConnection) -> Result<PathBuf> {
    let settings = review_settings::Entity::find_by_id(1)
        .one(connection)
        .await
        .context("reading review cache settings")?;
    if let Some(settings) = settings.as_ref()
        && !settings.cache_root.trim().is_empty()
    {
        let root = PathBuf::from(&settings.cache_root);
        match ensure_cache_root(&root) {
            Ok(()) => return Ok(root),
            Err(error) => eprintln!(
                "review cache: replacing unusable cache root {}: {error:#}",
                root.display()
            ),
        }
    }

    let root = Builder::new()
        .prefix(CACHE_ROOT_PREFIX)
        .tempdir_in(env::temp_dir())
        .context("creating persistent mini-film cache directory")?
        .keep();
    if let Some(settings) = settings {
        let mut active = settings.into_active_model();
        active.cache_root = Set(root.to_string_lossy().into_owned());
        active
            .update(connection)
            .await
            .context("persisting review cache root")?;
    }
    Ok(root)
}

fn ensure_cache_root(root: &Path) -> Result<()> {
    if !root.is_absolute() {
        bail!("review cache root must be absolute: {}", root.display());
    }
    fs::create_dir_all(root)
        .with_context(|| format!("creating review cache root {}", root.display()))?;
    if !root.is_dir() {
        bail!("review cache root is not a directory: {}", root.display());
    }
    Builder::new()
        .prefix(".mini-film-cache-probe-")
        .tempfile_in(root)
        .with_context(|| format!("checking review cache root {}", root.display()))?
        .close()
        .with_context(|| format!("cleaning review cache probe in {}", root.display()))?;
    Ok(())
}

pub(super) async fn migrate_legacy_output_caches(
    connection: &DatabaseConnection,
    roots: &ReviewPathRoots,
) -> Result<usize> {
    if !roots.output_root().is_dir() {
        return Ok(0);
    }
    let mut moved = migrate_retouch_files(roots)?;
    rewrite_legacy_cache_paths(connection).await?;
    moved += migrate_gallery_thumbnails(roots)?;
    moved += migrate_owned_output_entries(roots)?;
    ensure_output_has_no_cache_entries(roots.output_root())?;
    Ok(moved)
}

fn migrate_retouch_files(roots: &ReviewPathRoots) -> Result<usize> {
    let mut paths = WalkDir::new(roots.output_root())
        .min_depth(1)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| is_legacy_retouch_file(entry.file_name()))
        .map(|entry| entry.into_path())
        .collect::<Vec<_>>();
    paths.sort();

    let mut moved = 0;
    for source in paths {
        let relative = source
            .strip_prefix(roots.output_root())
            .with_context(|| format!("resolving legacy retouch cache {}", source.display()))?;
        let destination = roots.cache_root().join(RETOUCH_CACHE_DIR).join(relative);
        move_path_preserving(
            &source,
            &destination,
            &roots
                .cache_root()
                .join(MIGRATION_CONFLICT_CACHE_DIR)
                .join(RETOUCH_CACHE_DIR),
        )?;
        moved += 1;
    }
    Ok(moved)
}

fn is_legacy_retouch_file(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|name| {
        name.starts_with('.')
            && (name.contains(RETOUCH_CACHE_MARKER) || name.contains(RETOUCH_TEMP_MARKER))
    })
}

async fn rewrite_legacy_cache_paths(connection: &DatabaseConnection) -> Result<()> {
    let images = images::Entity::find()
        .all(connection)
        .await
        .context("reading preview paths for cache migration")?;
    let renders = image_profile_renders::Entity::find()
        .all(connection)
        .await
        .context("reading profile render paths for cache migration")?;
    let panorama_previews = panorama_previews::Entity::find()
        .all(connection)
        .await
        .context("reading panorama preview paths for cache migration")?;
    let transaction = connection
        .begin()
        .await
        .context("starting cache path migration transaction")?;

    let result = async {
        for row in images {
            let Some(path) = row.preview_path.as_deref().and_then(migrated_cache_path) else {
                continue;
            };
            let mut active = row.into_active_model();
            active.preview_path = Set(Some(path));
            active.update(&transaction).await?;
        }
        for row in renders {
            let Some(path) = row.output_path.as_deref().and_then(migrated_cache_path) else {
                continue;
            };
            let mut active = row.into_active_model();
            active.output_path = Set(Some(path));
            active.update(&transaction).await?;
        }
        for row in panorama_previews {
            let Some(path) = row.preview_path.as_deref().and_then(migrated_cache_path) else {
                continue;
            };
            let mut active = row.into_active_model();
            active.preview_path = Set(Some(path));
            active.update(&transaction).await?;
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;

    match result {
        Ok(()) => transaction
            .commit()
            .await
            .context("committing cache path migration"),
        Err(error) => {
            transaction.rollback().await.ok();
            Err(error)
        }
    }
}

fn migrated_cache_path(path: &str) -> Option<String> {
    let path = Path::new(path);
    if path.is_absolute() || cache_storage_path(path).is_some() {
        return None;
    }
    let path = if is_legacy_retouch_file(path.file_name()?) {
        Path::new(RETOUCH_CACHE_DIR).join(path)
    } else {
        path.to_path_buf()
    };
    is_cache_relative_path(&path).then(|| {
        Path::new(CACHE_STORAGE_NAMESPACE)
            .join(path)
            .to_string_lossy()
            .into_owned()
    })
}

fn migrate_gallery_thumbnails(roots: &ReviewPathRoots) -> Result<usize> {
    let source = roots.output_root().join(LEGACY_GALLERY_THUMBNAILS_DIR);
    if fs::symlink_metadata(&source).is_err() {
        return Ok(0);
    }
    let destination = roots.output_root().join("thumbnails");
    move_path_preserving(
        &source,
        &destination,
        &roots
            .cache_root()
            .join(MIGRATION_CONFLICT_CACHE_DIR)
            .join("gallery-thumbnails"),
    )?;
    rewrite_legacy_gallery_links(roots.output_root())?;
    Ok(1)
}

fn rewrite_legacy_gallery_links(output_root: &Path) -> Result<()> {
    for entry in WalkDir::new(output_root)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let path = entry.path();
        if !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("html"))
        {
            continue;
        }
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading legacy gallery {}", path.display()))?;
        if !text.contains(LEGACY_GALLERY_THUMBNAILS_DIR) {
            continue;
        }
        fs::write(
            path,
            text.replace(LEGACY_GALLERY_THUMBNAILS_DIR, "thumbnails"),
        )
        .with_context(|| format!("updating legacy gallery {}", path.display()))?;
    }
    Ok(())
}

fn migrate_owned_output_entries(roots: &ReviewPathRoots) -> Result<usize> {
    let mut discovered = Vec::new();
    let walker = WalkDir::new(roots.output_root())
        .min_depth(1)
        .follow_links(false)
        .into_iter();
    for entry in walker {
        let entry = entry.with_context(|| {
            format!(
                "scanning legacy mini-film caches in {}",
                roots.output_root().display()
            )
        })?;
        if is_cache_directory_name(entry.file_name()) {
            discovered.push(entry.into_path());
        }
    }
    discovered.sort_by_key(|path| path.components().count());
    let mut candidates = Vec::new();
    for path in discovered {
        if !candidates
            .iter()
            .any(|parent: &PathBuf| path.starts_with(parent))
        {
            candidates.push(path);
        }
    }

    let mut moved = 0;
    for source in candidates {
        if fs::symlink_metadata(&source).is_err() {
            continue;
        }
        let relative = source
            .strip_prefix(roots.output_root())
            .with_context(|| format!("resolving legacy cache {}", source.display()))?;
        let destination = if relative.components().count() == 1 {
            roots.cache_root().join(relative)
        } else {
            roots
                .cache_root()
                .join(LEGACY_OUTPUT_CACHE_DIR)
                .join(relative)
        };
        move_path_preserving(
            &source,
            &destination,
            &roots.cache_root().join(MIGRATION_CONFLICT_CACHE_DIR),
        )?;
        moved += 1;
    }
    Ok(moved)
}

fn ensure_output_has_no_cache_entries(output_root: &Path) -> Result<()> {
    for entry in WalkDir::new(output_root)
        .min_depth(1)
        .follow_links(false)
        .into_iter()
    {
        let entry = entry
            .with_context(|| format!("checking output directory {}", output_root.display()))?;
        if is_cache_directory_name(entry.file_name()) {
            bail!(
                "legacy mini-film cache remains in output directory: {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn move_path_preserving(source: &Path, destination: &Path, conflicts: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("reading legacy cache entry {}", source.display()))?;
    if metadata.is_dir() {
        if fs::symlink_metadata(destination).is_ok_and(|metadata| !metadata.is_dir()) {
            preserve_conflict(destination, conflicts)?;
        }
        fs::create_dir_all(destination)
            .with_context(|| format!("creating cache directory {}", destination.display()))?;
        for entry in fs::read_dir(source)
            .with_context(|| format!("reading cache directory {}", source.display()))?
        {
            let entry =
                entry.with_context(|| format!("reading cache entry in {}", source.display()))?;
            move_path_preserving(
                &entry.path(),
                &destination.join(entry.file_name()),
                conflicts,
            )?;
        }
        fs::remove_dir(source)
            .with_context(|| format!("removing migrated cache directory {}", source.display()))?;
        return Ok(());
    }

    if fs::symlink_metadata(destination).is_ok() {
        preserve_conflict(destination, conflicts)?;
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating cache directory {}", parent.display()))?;
    }
    if fs::rename(source, destination).is_ok() {
        return Ok(());
    }
    if metadata.file_type().is_symlink() {
        copy_symlink(source, destination)?;
    } else {
        fs::copy(source, destination).with_context(|| {
            format!(
                "copying legacy cache {} to {}",
                source.display(),
                destination.display()
            )
        })?;
        let accessed = FileTime::from_last_access_time(&metadata);
        let modified = FileTime::from_last_modification_time(&metadata);
        set_file_times(destination, accessed, modified)
            .with_context(|| format!("preserving timestamps on {}", destination.display()))?;
    }
    fs::remove_file(source)
        .with_context(|| format!("removing migrated cache entry {}", source.display()))
}

fn preserve_conflict(path: &Path, conflicts: &Path) -> Result<()> {
    fs::create_dir_all(conflicts)
        .with_context(|| format!("creating cache conflict directory {}", conflicts.display()))?;
    let name = path
        .file_name()
        .ok_or_else(|| anyhow!("cache conflict has no file name: {}", path.display()))?;
    for index in 1.. {
        let candidate = conflicts.join(format!("{}-{index}", name.to_string_lossy()));
        if fs::symlink_metadata(&candidate).is_err() {
            return move_path_preserving(path, &candidate, &conflicts.join("nested"));
        }
    }
    unreachable!()
}

#[cfg(unix)]
fn copy_symlink(source: &Path, destination: &Path) -> Result<()> {
    let target =
        fs::read_link(source).with_context(|| format!("reading symlink {}", source.display()))?;
    std::os::unix::fs::symlink(&target, destination).with_context(|| {
        format!(
            "copying cache symlink {} -> {}",
            destination.display(),
            target.display()
        )
    })
}

#[cfg(windows)]
fn copy_symlink(source: &Path, destination: &Path) -> Result<()> {
    let target =
        fs::read_link(source).with_context(|| format!("reading symlink {}", source.display()))?;
    if source.is_dir() {
        std::os::windows::fs::symlink_dir(&target, destination)
    } else {
        std::os::windows::fs::symlink_file(&target, destination)
    }
    .with_context(|| {
        format!(
            "copying cache symlink {} -> {}",
            destination.display(),
            target.display()
        )
    })
}
