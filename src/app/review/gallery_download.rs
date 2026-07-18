use super::{prelude::*, preview::short_path_sha1};
use std::{
    io::{self, BufReader},
    time::{SystemTime, UNIX_EPOCH},
};
use walkdir::WalkDir;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const GALLERY_ARCHIVE_CACHE_DIR: &str = ".mini-film-gallery-downloads";

#[derive(Clone, Debug)]
pub(super) struct GalleryArchiveSpec {
    gallery_root: PathBuf,
    cache_path: PathBuf,
    archive_root_name: String,
    download_name: String,
}

impl GalleryArchiveSpec {
    pub(super) fn new(output_root: &Path, album: &Path) -> Self {
        let gallery_root = output_root.join(album);
        let raw_name = album
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("gallery");
        let archive_root_name = nonempty_sanitized_name(raw_name, "gallery");
        let download_base = ascii_download_name(&archive_root_name);
        let album_digest = short_path_sha1(album);
        let cache_path = output_root
            .join(GALLERY_ARCHIVE_CACHE_DIR)
            .join(format!("{archive_root_name}-{album_digest}.zip"));
        Self {
            gallery_root,
            cache_path,
            archive_root_name,
            download_name: format!("{download_base}.zip"),
        }
    }

    pub(super) fn download_name(&self) -> &str {
        &self.download_name
    }
}

#[derive(Debug)]
struct GalleryArchiveFile {
    source: PathBuf,
    relative: PathBuf,
    size: u64,
    modified: SystemTime,
}

pub(super) fn build_gallery_archive(spec: &GalleryArchiveSpec) -> Result<PathBuf> {
    if !spec.gallery_root.is_dir() {
        bail!(
            "published gallery directory is missing: {}",
            spec.gallery_root.display()
        );
    }
    let files = collect_gallery_files(&spec.gallery_root)?;
    if !files
        .iter()
        .any(|file| file.relative.ends_with("index.html"))
    {
        bail!(
            "published gallery has no index.html: {}",
            spec.gallery_root.display()
        );
    }
    let newest_source = files
        .iter()
        .map(|file| file.modified)
        .max()
        .unwrap_or(UNIX_EPOCH);
    if archive_is_current(&spec.cache_path, newest_source) {
        return Ok(spec.cache_path.clone());
    }

    let cache_root = spec
        .cache_path
        .parent()
        .ok_or_else(|| anyhow!("gallery archive cache has no parent directory"))?;
    fs::create_dir_all(cache_root).with_context(|| format!("creating {}", cache_root.display()))?;
    let mut temporary = Builder::new()
        .prefix(".gallery-")
        .suffix(".zip.tmp")
        .tempfile_in(cache_root)
        .with_context(|| format!("creating gallery archive in {}", cache_root.display()))?;
    {
        let mut archive = ZipWriter::new(temporary.as_file_mut());
        for file in &files {
            let entry_name = archive_entry_name(&spec.archive_root_name, &file.relative);
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Stored)
                .large_file(file.size > u64::from(u32::MAX))
                .unix_permissions(0o644);
            archive
                .start_file(&entry_name, options)
                .with_context(|| format!("adding {entry_name} to gallery archive"))?;
            let source = fs::File::open(&file.source)
                .with_context(|| format!("opening {}", file.source.display()))?;
            io::copy(&mut BufReader::new(source), &mut archive)
                .with_context(|| format!("archiving {}", file.source.display()))?;
        }
        archive.finish().context("finishing gallery archive")?;
    }
    temporary
        .as_file()
        .sync_all()
        .context("syncing gallery archive")?;
    temporary
        .persist(&spec.cache_path)
        .map_err(|error| error.error)
        .with_context(|| format!("installing {}", spec.cache_path.display()))?;
    Ok(spec.cache_path.clone())
}

fn collect_gallery_files(root: &Path) -> Result<Vec<GalleryArchiveFile>> {
    let mut files = Vec::new();
    let entries = WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| entry.depth() == 0 || !excluded_gallery_directory(entry.file_name()));
    for entry in entries {
        let entry = entry.with_context(|| format!("walking gallery {}", root.display()))?;
        if entry.file_type().is_dir() {
            continue;
        }
        let source = entry.path();
        let relative = source
            .strip_prefix(root)
            .with_context(|| format!("resolving gallery path {}", source.display()))?;
        if !gallery_asset_path(relative) {
            continue;
        }
        let metadata = fs::metadata(source)
            .with_context(|| format!("reading gallery asset {}", source.display()))?;
        if !metadata.is_file() {
            continue;
        }
        files.push(GalleryArchiveFile {
            source: source.to_path_buf(),
            relative: relative.to_path_buf(),
            size: metadata.len(),
            modified: metadata.modified().unwrap_or(UNIX_EPOCH),
        });
    }
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(files)
}

fn excluded_gallery_directory(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_str(),
        Some(".mini-film-profile-inputs")
            | Some(".mini-film-review-previews")
            | Some(GALLERY_ARCHIVE_CACHE_DIR)
    )
}

fn gallery_asset_path(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "html"
            | "htm"
            | "css"
            | "js"
            | "mjs"
            | "json"
            | "jpg"
            | "jpeg"
            | "png"
            | "gif"
            | "webp"
            | "avif"
            | "svg"
            | "tif"
            | "tiff"
            | "ico"
            | "woff"
            | "woff2"
            | "ttf"
            | "otf"
    )
}

fn archive_is_current(path: &Path, newest_source: SystemTime) -> bool {
    fs::metadata(path).is_ok_and(|metadata| {
        metadata.len() > 0
            && metadata
                .modified()
                .is_ok_and(|modified| modified >= newest_source)
    })
}

fn archive_entry_name(root_name: &str, relative: &Path) -> String {
    let relative = relative.to_string_lossy().replace('\\', "/");
    format!("{root_name}/{relative}")
}

fn nonempty_sanitized_name(raw: &str, fallback: &str) -> String {
    let name = sanitize_filename::sanitize(raw).into_owned();
    if name.trim().is_empty() {
        fallback.to_string()
    } else {
        name
    }
}

fn ascii_download_name(raw: &str) -> String {
    let name = raw
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, ' ' | '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    nonempty_sanitized_name(&name, "gallery")
}
