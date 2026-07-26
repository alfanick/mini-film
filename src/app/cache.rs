use std::{
    ffi::OsStr,
    path::{Component, Path},
};

pub(crate) const CACHE_DIRECTORY_PREFIX: &str = ".mini-film-";
pub(crate) const CACHE_STORAGE_NAMESPACE: &str = ".mini-film-cache";
pub(crate) const REVIEW_PREVIEWS_CACHE_DIR: &str = ".mini-film-review-previews";
pub(crate) const PANORAMA_CACHE_DIR: &str = ".mini-film-panoramas";
pub(crate) const SAMPLER_CACHE_DIR: &str = ".mini-film-sampler";
pub(crate) const PROFILE_INPUT_CACHE_DIR: &str = ".mini-film-profile-inputs";
pub(crate) const GALLERY_DOWNLOAD_CACHE_DIR: &str = ".mini-film-gallery-downloads";
pub(crate) const RETOUCH_CACHE_DIR: &str = ".mini-film-retouch";
pub(crate) const PROFILE_DETAILS_CACHE_DIR: &str = ".mini-film-profile-details";
pub(crate) const DAEMON_PROFILE_OUTPUTS_CACHE_DIR: &str = ".mini-film-profile-outputs";
pub(crate) const LEGACY_OUTPUT_CACHE_DIR: &str = ".mini-film-legacy-output";
pub(crate) const MIGRATION_CONFLICT_CACHE_DIR: &str = ".mini-film-migration-conflicts";
pub(crate) const LEGACY_GALLERY_THUMBNAILS_DIR: &str = ".mini-film-gallery-thumbnails";

pub(crate) fn is_cache_directory_name(name: &OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| name.starts_with(CACHE_DIRECTORY_PREFIX))
}

pub(crate) fn is_cache_relative_path(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(Component::Normal(name)) if is_cache_directory_name(name)
    )
}

pub(crate) fn cache_storage_path(path: &Path) -> Option<&Path> {
    let relative = path.strip_prefix(CACHE_STORAGE_NAMESPACE).ok()?;
    (!relative.as_os_str().is_empty()).then_some(relative)
}
