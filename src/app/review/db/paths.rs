use super::{entities::*, *};
use crate::app::cache::{CACHE_STORAGE_NAMESPACE, cache_storage_path, is_cache_relative_path};
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, EntityTrait, IntoActiveModel, Set, TransactionTrait,
};

const OUTPUT_ROOT_MARKERS: [&str; 2] = [".mini-film-review-previews", ".mini-film-panoramas"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ReviewPathRoots {
    input_root: PathBuf,
    output_root: PathBuf,
    cache_root: PathBuf,
}

impl ReviewPathRoots {
    pub(super) fn new(input_root: &Path, output_root: &Path, cache_root: &Path) -> Result<Self> {
        if input_root.as_os_str().is_empty() {
            bail!("review input root cannot be empty");
        }
        if output_root.as_os_str().is_empty() {
            bail!("review output root cannot be empty");
        }
        if cache_root.as_os_str().is_empty() {
            bail!("review cache root cannot be empty");
        }
        Ok(Self {
            input_root: input_root.to_path_buf(),
            output_root: output_root.to_path_buf(),
            cache_root: cache_root.to_path_buf(),
        })
    }

    pub(super) fn input_root(&self) -> &Path {
        &self.input_root
    }

    pub(super) fn output_root(&self) -> &Path {
        &self.output_root
    }

    pub(super) fn cache_root(&self) -> &Path {
        &self.cache_root
    }

    pub(super) fn source_to_storage(&self, path: &Path, field: &str) -> Result<String> {
        path_to_storage(path, &self.input_root, field)
    }

    pub(super) fn output_to_storage(&self, path: &Path, field: &str) -> Result<String> {
        if path.is_absolute() && path.starts_with(&self.cache_root) {
            let relative = path
                .strip_prefix(&self.cache_root)
                .with_context(|| format!("resolving {field} cache path {}", path.display()))?;
            if !is_cache_relative_path(relative) {
                bail!(
                    "{field} cache path {} has no mini-film cache namespace",
                    path.display()
                );
            }
            return relative_path_text(&Path::new(CACHE_STORAGE_NAMESPACE).join(relative), field);
        }
        path_to_storage(path, &self.output_root, field)
    }

    pub(super) fn source_from_storage(&self, path: &str, field: &str) -> Result<PathBuf> {
        path_from_storage(path, &self.input_root, field)
    }

    pub(super) fn output_from_storage(&self, path: &str, field: &str) -> Result<PathBuf> {
        let relative = Path::new(path);
        if let Some(cache_path) = cache_storage_path(relative) {
            validate_relative_path(cache_path, field)?;
            if !is_cache_relative_path(cache_path) {
                bail!("{field} has an invalid mini-film cache path: {path}");
            }
            return Ok(self.cache_root.join(cache_path));
        }
        path_from_storage(path, &self.output_root, field)
    }

    fn input_root_text(&self) -> String {
        self.input_root.to_string_lossy().into_owned()
    }

    fn output_root_text(&self) -> String {
        self.output_root.to_string_lossy().into_owned()
    }
}

pub(super) async fn prepare_relative_path_storage(
    connection: &DatabaseConnection,
    roots: &ReviewPathRoots,
) -> Result<()> {
    let Some(settings) = review_settings::Entity::find_by_id(1)
        .one(connection)
        .await
        .context("reading review path settings")?
    else {
        return Ok(());
    };
    if !settings.input_root.trim().is_empty() && !settings.output_root.trim().is_empty() {
        if settings.input_root == roots.input_root_text()
            && settings.output_root == roots.output_root_text()
        {
            return Ok(());
        }
        let mut active = settings.into_active_model();
        active.input_root = Set(roots.input_root_text());
        active.output_root = Set(roots.output_root_text());
        active
            .update(connection)
            .await
            .context("updating moved review path roots")?;
        return Ok(());
    }

    let transaction = connection
        .begin()
        .await
        .context("starting review path migration transaction")?;
    let result = convert_paths_to_relative(&transaction, settings, roots).await;
    match result {
        Ok(()) => transaction
            .commit()
            .await
            .context("committing review path migration"),
        Err(error) => {
            transaction.rollback().await.ok();
            Err(error)
        }
    }
}

pub(super) async fn restore_absolute_path_storage<C>(connection: &C) -> Result<()>
where
    C: ConnectionTrait,
{
    let Some(settings) = review_settings::Entity::find_by_id(1)
        .one(connection)
        .await
        .context("reading review path settings before migration rollback")?
    else {
        return Ok(());
    };
    if settings.input_root.trim().is_empty() || settings.output_root.trim().is_empty() {
        bail!("review path roots are missing; cannot roll back relative path storage");
    }
    let cache_root = if settings.cache_root.trim().is_empty() {
        Path::new(&settings.output_root)
    } else {
        Path::new(&settings.cache_root)
    };
    let roots = ReviewPathRoots::new(
        Path::new(&settings.input_root),
        Path::new(&settings.output_root),
        cache_root,
    )?;

    for row in images::Entity::find().all(connection).await? {
        let mut active = row.clone().into_active_model();
        active.raw_path = Set(absolute_storage_path(
            &row.raw_path,
            roots.input_root(),
            "images.raw_path",
        )?);
        active.sooc_sidecar_path = Set(row
            .sooc_sidecar_path
            .as_deref()
            .map(|path| absolute_storage_path(path, roots.input_root(), "images.sooc_sidecar_path"))
            .transpose()?);
        active.preview_path = Set(row
            .preview_path
            .as_deref()
            .map(|path| absolute_output_storage_path(&roots, path, "images.preview_path"))
            .transpose()?);
        active.update(connection).await?;
    }
    for row in image_profile_renders::Entity::find()
        .all(connection)
        .await?
    {
        let mut active = row.clone().into_active_model();
        active.output_path = Set(row
            .output_path
            .as_deref()
            .map(|path| {
                absolute_output_storage_path(&roots, path, "image_profile_renders.output_path")
            })
            .transpose()?);
        active.update(connection).await?;
    }
    for row in panorama_projects::Entity::find().all(connection).await? {
        let mut active = row.clone().into_active_model();
        active.output_path = Set(row
            .output_path
            .as_deref()
            .map(|path| {
                absolute_storage_path(path, roots.input_root(), "panorama_projects.output_path")
            })
            .transpose()?);
        active.update(connection).await?;
    }
    for row in panorama_previews::Entity::find().all(connection).await? {
        let mut active = row.clone().into_active_model();
        active.preview_path = Set(row
            .preview_path
            .as_deref()
            .map(|path| {
                absolute_output_storage_path(&roots, path, "panorama_previews.preview_path")
            })
            .transpose()?);
        active.update(connection).await?;
    }
    Ok(())
}

async fn convert_paths_to_relative<C>(
    connection: &C,
    settings: review_settings::Model,
    roots: &ReviewPathRoots,
) -> Result<()>
where
    C: ConnectionTrait,
{
    let image_rows = images::Entity::find()
        .all(connection)
        .await
        .context("reading image paths for migration")?;
    let render_rows = image_profile_renders::Entity::find()
        .all(connection)
        .await
        .context("reading render paths for migration")?;
    let project_rows = panorama_projects::Entity::find()
        .all(connection)
        .await
        .context("reading panorama output paths for migration")?;
    let preview_rows = panorama_previews::Entity::find()
        .all(connection)
        .await
        .context("reading panorama preview paths for migration")?;

    let mut input_roots = Vec::new();
    push_root_text(&mut input_roots, &settings.input_root);
    if let Some(inferred) = infer_legacy_input_root(&image_rows)? {
        push_root(&mut input_roots, inferred);
    }
    push_root(&mut input_roots, roots.input_root().to_path_buf());

    let mut output_roots = Vec::new();
    push_root_text(&mut output_roots, &settings.output_root);
    if let Some(inferred) = infer_legacy_output_root(&image_rows, &render_rows, &preview_rows)? {
        push_root(&mut output_roots, inferred);
    }
    push_root(&mut output_roots, roots.output_root().to_path_buf());

    for row in image_rows {
        let raw_fallback = Path::new(&row.relative_path);
        let raw_path = legacy_path_to_storage(
            Path::new(&row.raw_path),
            &input_roots,
            Some(raw_fallback),
            "images.raw_path",
        )?;
        let sooc_sidecar_path = row
            .sooc_sidecar_path
            .as_deref()
            .map(|path| {
                legacy_path_to_storage(
                    Path::new(path),
                    &input_roots,
                    None,
                    "images.sooc_sidecar_path",
                )
            })
            .transpose()?;
        let preview_path = row
            .preview_path
            .as_deref()
            .map(|path| {
                legacy_path_to_storage(Path::new(path), &output_roots, None, "images.preview_path")
            })
            .transpose()?;
        if raw_path != row.raw_path
            || sooc_sidecar_path != row.sooc_sidecar_path
            || preview_path != row.preview_path
        {
            let mut active = row.into_active_model();
            active.raw_path = Set(raw_path);
            active.sooc_sidecar_path = Set(sooc_sidecar_path);
            active.preview_path = Set(preview_path);
            active.update(connection).await?;
        }
    }

    for row in render_rows {
        let output_path = row
            .output_path
            .as_deref()
            .map(|path| {
                legacy_path_to_storage(
                    Path::new(path),
                    &output_roots,
                    None,
                    "image_profile_renders.output_path",
                )
            })
            .transpose()?;
        if output_path != row.output_path {
            let mut active = row.into_active_model();
            active.output_path = Set(output_path);
            active.update(connection).await?;
        }
    }

    for row in project_rows {
        let output_path = row
            .output_path
            .as_deref()
            .map(|path| {
                legacy_path_to_storage(
                    Path::new(path),
                    &input_roots,
                    None,
                    "panorama_projects.output_path",
                )
            })
            .transpose()?;
        if output_path != row.output_path {
            let mut active = row.into_active_model();
            active.output_path = Set(output_path);
            active.update(connection).await?;
        }
    }

    for row in preview_rows {
        let preview_path = row
            .preview_path
            .as_deref()
            .map(|path| {
                legacy_path_to_storage(
                    Path::new(path),
                    &output_roots,
                    None,
                    "panorama_previews.preview_path",
                )
            })
            .transpose()?;
        if preview_path != row.preview_path {
            let mut active = row.into_active_model();
            active.preview_path = Set(preview_path);
            active.update(connection).await?;
        }
    }

    let mut active = settings.into_active_model();
    active.input_root = Set(roots.input_root_text());
    active.output_root = Set(roots.output_root_text());
    active
        .update(connection)
        .await
        .context("updating review path roots")?;
    Ok(())
}

fn path_to_storage(path: &Path, root: &Path, field: &str) -> Result<String> {
    if path.is_relative() {
        return relative_path_text(path, field);
    }
    let relative = path.strip_prefix(root).with_context(|| {
        format!(
            "{field} {} is outside its configured root {}",
            path.display(),
            root.display()
        )
    })?;
    relative_path_text(relative, field)
}

fn path_from_storage(path: &str, root: &Path, field: &str) -> Result<PathBuf> {
    let relative = Path::new(path);
    validate_relative_path(relative, field)?;
    Ok(root.join(relative))
}

fn legacy_path_to_storage(
    path: &Path,
    roots: &[PathBuf],
    fallback: Option<&Path>,
    field: &str,
) -> Result<String> {
    if path.is_relative() {
        return relative_path_text(path, field);
    }
    for root in roots {
        if let Ok(relative) = path.strip_prefix(root) {
            return relative_path_text(relative, field);
        }
    }
    if let Some(fallback) = fallback
        && path.ends_with(fallback)
    {
        return relative_path_text(fallback, field);
    }
    bail!(
        "cannot migrate {field} {} relative to any known root",
        path.display()
    )
}

fn absolute_storage_path(path: &str, root: &Path, field: &str) -> Result<String> {
    let path = Path::new(path);
    if path.is_absolute() {
        return Ok(path.to_string_lossy().into_owned());
    }
    validate_relative_path(path, field)?;
    Ok(root.join(path).to_string_lossy().into_owned())
}

fn absolute_output_storage_path(
    roots: &ReviewPathRoots,
    path: &str,
    field: &str,
) -> Result<String> {
    let path = Path::new(path);
    if path.is_absolute() {
        return Ok(path.to_string_lossy().into_owned());
    }
    validate_relative_path(path, field)?;
    if let Some(cache_path) = cache_storage_path(path) {
        validate_relative_path(cache_path, field)?;
        return Ok(roots
            .cache_root()
            .join(cache_path)
            .to_string_lossy()
            .into_owned());
    }
    Ok(roots
        .output_root()
        .join(path)
        .to_string_lossy()
        .into_owned())
}

fn relative_path_text(path: &Path, field: &str) -> Result<String> {
    validate_relative_path(path, field)?;
    Ok(path.to_string_lossy().into_owned())
}

fn validate_relative_path(path: &Path, field: &str) -> Result<()> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        bail!("{field} must be a non-empty relative path");
    }
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!(
            "{field} contains an unsafe path component: {}",
            path.display()
        );
    }
    Ok(())
}

fn infer_legacy_input_root(rows: &[images::Model]) -> Result<Option<PathBuf>> {
    let mut inferred: Option<PathBuf> = None;
    for row in rows {
        let raw = Path::new(&row.raw_path);
        let relative = Path::new(&row.relative_path);
        if !raw.is_absolute()
            || validate_relative_path(relative, "images.relative_path").is_err()
            || !raw.ends_with(relative)
        {
            continue;
        }
        let Some(candidate) = raw.ancestors().nth(relative.components().count()) else {
            continue;
        };
        if let Some(existing) = &inferred
            && existing != candidate
        {
            bail!(
                "cannot infer one legacy input root from {} and {}",
                existing.display(),
                candidate.display()
            );
        }
        inferred = Some(candidate.to_path_buf());
    }
    Ok(inferred)
}

fn infer_legacy_output_root(
    images: &[images::Model],
    renders: &[image_profile_renders::Model],
    panorama_previews: &[panorama_previews::Model],
) -> Result<Option<PathBuf>> {
    let images_by_id = images
        .iter()
        .map(|image| (image.image_id, image))
        .collect::<HashMap<_, _>>();
    let mut inferred: Option<PathBuf> = None;
    for candidate in images
        .iter()
        .filter_map(|row| row.preview_path.as_deref())
        .chain(
            panorama_previews
                .iter()
                .filter_map(|row| row.preview_path.as_deref()),
        )
        .filter_map(output_root_from_marker)
        .chain(render_output_roots(&images_by_id, renders))
    {
        if let Some(existing) = &inferred
            && existing != &candidate
        {
            bail!(
                "cannot infer one legacy output root from {} and {}",
                existing.display(),
                candidate.display()
            );
        }
        inferred = Some(candidate);
    }
    Ok(inferred)
}

fn render_output_roots<'a>(
    images_by_id: &'a HashMap<i64, &'a images::Model>,
    renders: &'a [image_profile_renders::Model],
) -> impl Iterator<Item = PathBuf> + 'a {
    renders.iter().filter_map(|render| {
        let output = Path::new(render.output_path.as_deref()?);
        let output_parent = output.is_absolute().then(|| output.parent())??;
        let image = images_by_id.get(&render.image_id)?;
        let relative_parent = Path::new(&image.relative_path).parent()?;
        if !valid_relative_directory(relative_parent) {
            return None;
        }

        let mut suffix = relative_parent.to_path_buf();
        let profile_stem = sanitize_filename::sanitize(&render.profile_stem);
        if !profile_stem.trim().is_empty() {
            suffix.push(profile_stem.as_ref());
        }
        if !output_parent.ends_with(&suffix) {
            return None;
        }
        output_parent
            .ancestors()
            .nth(suffix.components().count())
            .map(Path::to_path_buf)
    })
}

fn valid_relative_directory(path: &Path) -> bool {
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn output_root_from_marker(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return None;
    }
    path.ancestors().find_map(|ancestor| {
        let name = ancestor.file_name()?.to_str()?;
        OUTPUT_ROOT_MARKERS
            .contains(&name)
            .then(|| ancestor.parent().map(Path::to_path_buf))
            .flatten()
    })
}

fn push_root_text(roots: &mut Vec<PathBuf>, root: &str) {
    if !root.trim().is_empty() {
        push_root(roots, PathBuf::from(root));
    }
}

fn push_root(roots: &mut Vec<PathBuf>, root: PathBuf) {
    if !roots.contains(&root) {
        roots.push(root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_mapping_round_trips_under_each_root() {
        let roots = ReviewPathRoots::new(
            Path::new("/input"),
            Path::new("/output"),
            Path::new("/tmp/mini-film.test"),
        )
        .unwrap();

        assert_eq!(
            roots
                .source_to_storage(Path::new("/input/day/frame.nef"), "source")
                .unwrap(),
            "day/frame.nef"
        );
        assert_eq!(
            roots
                .source_from_storage("day/frame.nef", "source")
                .unwrap(),
            Path::new("/input/day/frame.nef")
        );
        assert_eq!(
            roots
                .output_to_storage(Path::new("/output/day/frame.jpg"), "output")
                .unwrap(),
            "day/frame.jpg"
        );
        assert_eq!(
            roots
                .output_from_storage("day/frame.jpg", "output")
                .unwrap(),
            Path::new("/output/day/frame.jpg")
        );
        assert_eq!(
            roots
                .output_to_storage(
                    Path::new("/tmp/mini-film.test/.mini-film-review-previews/frame.jpg"),
                    "cache",
                )
                .unwrap(),
            ".mini-film-cache/.mini-film-review-previews/frame.jpg"
        );
        assert_eq!(
            roots
                .output_from_storage(
                    ".mini-film-cache/.mini-film-review-previews/frame.jpg",
                    "cache",
                )
                .unwrap(),
            Path::new("/tmp/mini-film.test/.mini-film-review-previews/frame.jpg")
        );
    }

    #[test]
    fn path_mapping_rejects_escape_and_wrong_root() {
        let roots = ReviewPathRoots::new(
            Path::new("/input"),
            Path::new("/output"),
            Path::new("/tmp/mini-film.test"),
        )
        .unwrap();

        assert!(
            roots
                .source_from_storage("../outside.nef", "source")
                .is_err()
        );
        assert!(
            roots
                .source_to_storage(Path::new("/elsewhere/frame.nef"), "source")
                .is_err()
        );
    }
}
