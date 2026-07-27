mod metadata;

#[cfg(target_os = "linux")]
mod linux;

use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use filetime::{FileTime, set_file_mtime};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tempfile::NamedTempFile;
use walkdir::WalkDir;

use crate::app::{
    dng::DngFallbackConfig,
    review::{
        AutoImportAsset, AutoImportCatalog, AutoImportDevice, AutoImportGroup, AutoImportIdentity,
        AutoImportMediaKind, AutoImportRecord, AutoImportSourceRecord, AutoImportStorage,
    },
    util::is_raw_input_file,
};

use metadata::{read_identity, strong_identity_match};

const RECONCILE_INTERVAL: Duration = Duration::from_secs(5);
const COPY_BUFFER_SIZE: usize = 256 * 1024;
const MAX_DESTINATION_ATTEMPTS: usize = 10_000;

#[derive(Clone)]
pub(crate) struct AutoImportConfig {
    pub(crate) input_root: PathBuf,
    pub(crate) catalog: AutoImportCatalog,
    pub(crate) dng_fallback: DngFallbackConfig,
    pub(crate) exiftool: PathBuf,
    pub(crate) progress: MultiProgress,
    pub(crate) progress_anchor: ProgressBar,
}

pub(crate) struct AutoImportReceiver {
    logs: Receiver<String>,
}

impl AutoImportReceiver {
    pub(crate) fn drain_logs(&self) -> Vec<String> {
        let mut logs = Vec::new();
        loop {
            match self.logs.try_recv() {
                Ok(log) => logs.push(log),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return logs,
            }
        }
    }
}

pub(crate) fn start_auto_import(config: AutoImportConfig) -> Result<AutoImportReceiver> {
    #[cfg(target_os = "linux")]
    {
        start_linux_auto_import(config)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = config;
        bail!("--auto-import is supported only on Linux with mounted GVfs PTP/MTP cameras")
    }
}

#[cfg(target_os = "linux")]
fn start_linux_auto_import(config: AutoImportConfig) -> Result<AutoImportReceiver> {
    let gvfs_root = linux::gvfs_root()?;
    let (log_tx, log_rx) = mpsc::channel();
    let (reconcile_tx, reconcile_rx) = mpsc::sync_channel(1);
    linux::start_mount_signal_listeners(reconcile_tx.clone(), log_tx.clone());
    thread::Builder::new()
        .name("mini-film-auto-import".to_string())
        .spawn(move || {
            run_camera_manager(config, gvfs_root, reconcile_rx, log_tx);
        })
        .context("starting auto-import camera manager")?;
    let _ = reconcile_tx.try_send(());
    Ok(AutoImportReceiver { logs: log_rx })
}

#[cfg(target_os = "linux")]
struct CameraWorker {
    updates: SyncSender<linux::MountedCamera>,
}

#[cfg(target_os = "linux")]
fn run_camera_manager(
    config: AutoImportConfig,
    gvfs_root: PathBuf,
    reconcile_rx: Receiver<()>,
    logs: mpsc::Sender<String>,
) {
    let mut workers = HashMap::<String, CameraWorker>::new();
    loop {
        while reconcile_rx.try_recv().is_ok() {}
        match linux::discover_mounted_cameras(&gvfs_root) {
            Ok(cameras) => {
                let present = cameras
                    .iter()
                    .map(|camera| camera.device_key.clone())
                    .collect::<HashSet<_>>();
                workers.retain(|key, _| {
                    let keep = present.contains(key);
                    if !keep {
                        let _ = logs.send(format!("auto-import: camera disconnected ({key})"));
                    }
                    keep
                });
                for camera in cameras {
                    if let Some(worker) = workers.get(&camera.device_key) {
                        let _ = worker.updates.try_send(camera);
                        continue;
                    }
                    let (updates, receiver) = mpsc::sync_channel(1);
                    if updates.try_send(camera.clone()).is_err() {
                        continue;
                    }
                    let bar = config
                        .progress
                        .insert_after(&config.progress_anchor, ProgressBar::new(0));
                    bar.set_style(import_spinner_style());
                    bar.enable_steady_tick(Duration::from_millis(120));
                    bar.set_message(format!("{}: connected", camera.display_name));
                    let worker_config = config.clone();
                    let worker_logs = logs.clone();
                    let worker_key = camera.device_key.clone();
                    if thread::Builder::new()
                        .name(format!(
                            "mini-film-auto-import-{}",
                            sanitize_thread_name(&worker_key)
                        ))
                        .spawn(move || {
                            run_camera_worker(worker_config, receiver, worker_logs, bar);
                        })
                        .is_ok()
                    {
                        let _ = logs.send(format!(
                            "auto-import: connected {} with {} storage(s)",
                            camera.display_name,
                            camera.storages.len()
                        ));
                        workers.insert(worker_key, CameraWorker { updates });
                    }
                }
            }
            Err(error) => {
                let _ = logs.send(format!(
                    "auto-import: could not scan mounted GVfs cameras: {error:#}"
                ));
            }
        }

        match reconcile_rx.recv_timeout(RECONCILE_INTERVAL) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(target_os = "linux")]
fn run_camera_worker(
    config: AutoImportConfig,
    updates: Receiver<linux::MountedCamera>,
    logs: mpsc::Sender<String>,
    bar: ProgressBar,
) {
    while let Ok(mut camera) = updates.recv() {
        while let Ok(newer) = updates.try_recv() {
            camera = newer;
        }
        if let Err(error) = import_camera(&config, &camera, &bar, &logs) {
            let _ = logs.send(format!(
                "auto-import: {} scan failed: {error:#}",
                camera.display_name
            ));
            bar.set_style(import_spinner_style());
            bar.set_message(format!("{}: waiting to retry", camera.display_name));
        }
    }
    bar.finish_and_clear();
}

#[cfg(target_os = "linux")]
fn import_camera(
    config: &AutoImportConfig,
    camera: &linux::MountedCamera,
    bar: &ProgressBar,
    logs: &mpsc::Sender<String>,
) -> Result<()> {
    bar.set_style(import_spinner_style());
    bar.enable_steady_tick(Duration::from_millis(120));
    bar.set_message(format!("{}: scanning", camera.display_name));
    let device = config.catalog.register_device(
        &camera.device_key,
        &camera.display_name,
        camera.serial.as_deref(),
    )?;
    let mut sources = Vec::new();
    let mut scan_failures = 0usize;
    for mounted in &camera.storages {
        let storage = config.catalog.register_storage(
            device.id,
            &mounted.storage_key,
            &mounted.display_name,
        )?;
        match scan_storage(&storage, &mounted.root) {
            Ok(mut found) => sources.append(&mut found),
            Err(error) => {
                scan_failures += 1;
                let _ = logs.send(format!(
                    "auto-import: {} / {} is unavailable: {error:#}",
                    camera.display_name, mounted.display_name
                ));
            }
        }
    }
    sources.sort_by(compare_sources);

    let total_bytes = sources.iter().map(|source| source.size).sum();
    bar.disable_steady_tick();
    bar.set_style(import_progress_style());
    bar.set_length(total_bytes);
    bar.set_position(0);

    let mut stats = ImportStats {
        failed: scan_failures,
        ..ImportStats::default()
    };
    for source in sources {
        bar.set_message(format!("{}: {}", camera.display_name, source.filename));
        let start_position = bar.position();
        match import_source(config, &device, &source, bar) {
            Ok(ImportDisposition::Imported) => stats.imported += 1,
            Ok(ImportDisposition::Adopted) => stats.adopted += 1,
            Ok(ImportDisposition::Skipped) => stats.skipped += 1,
            Err(error) => {
                stats.failed += 1;
                let _ = logs.send(format!(
                    "auto-import: {} / {} failed: {error:#}",
                    camera.display_name, source.relative_path
                ));
            }
        }
        let consumed = bar.position().saturating_sub(start_position);
        bar.inc(source.size.saturating_sub(consumed));
    }

    bar.set_style(import_spinner_style());
    bar.enable_steady_tick(Duration::from_millis(120));
    bar.set_message(format!(
        "{}: idle ({} imported, {} adopted, {} skipped, {} failed)",
        camera.display_name, stats.imported, stats.adopted, stats.skipped, stats.failed
    ));
    if stats.imported > 0 || stats.adopted > 0 || stats.failed > 0 {
        let _ = logs.send(format!(
            "auto-import: {} completed: {} imported, {} adopted, {} skipped, {} failed",
            camera.display_name, stats.imported, stats.adopted, stats.skipped, stats.failed
        ));
    }
    Ok(())
}

#[derive(Default)]
struct ImportStats {
    imported: usize,
    adopted: usize,
    skipped: usize,
    failed: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ImportDisposition {
    Imported,
    Adopted,
    Skipped,
}

#[derive(Clone, Debug)]
struct SourceFile {
    storage: AutoImportStorage,
    path: PathBuf,
    relative_path: String,
    relative_path_key: String,
    filename: String,
    filename_key: String,
    stem: String,
    stem_key: String,
    extension: String,
    media_kind: AutoImportMediaKind,
    size: u64,
    modified: SystemTime,
    modified_ns: i64,
}

fn scan_storage(storage: &AutoImportStorage, root: &Path) -> Result<Vec<SourceFile>> {
    let mut sources = Vec::new();
    for entry in WalkDir::new(root).min_depth(1).follow_links(false) {
        let entry = entry.with_context(|| format!("walking camera storage {}", root.display()))?;
        if !entry.file_type().is_file() || !is_auto_import_source(entry.path()) {
            continue;
        }
        let metadata = entry
            .metadata()
            .with_context(|| format!("reading camera file {}", entry.path().display()))?;
        if metadata.len() == 0 {
            continue;
        }
        let modified = metadata
            .modified()
            .with_context(|| format!("reading camera timestamp {}", entry.path().display()))?;
        let relative = entry
            .path()
            .strip_prefix(root)
            .with_context(|| {
                format!(
                    "resolving camera path {} under {}",
                    entry.path().display(),
                    root.display()
                )
            })?
            .to_str()
            .with_context(|| format!("camera path is not valid UTF-8: {}", entry.path().display()))?
            .to_string();
        let filename = entry
            .file_name()
            .to_str()
            .with_context(|| {
                format!(
                    "camera filename is not valid UTF-8: {}",
                    entry.path().display()
                )
            })?
            .to_string();
        let stem = entry
            .path()
            .file_stem()
            .and_then(|value| value.to_str())
            .with_context(|| format!("camera filename has no stem: {filename}"))?
            .to_string();
        let extension = entry
            .path()
            .extension()
            .and_then(|value| value.to_str())
            .with_context(|| format!("camera filename has no extension: {filename}"))?
            .to_string();
        sources.push(SourceFile {
            storage: storage.clone(),
            path: entry.path().to_path_buf(),
            relative_path_key: relative.to_lowercase(),
            relative_path: relative,
            filename_key: filename.to_lowercase(),
            filename,
            stem_key: stem.to_lowercase(),
            stem,
            extension,
            media_kind: if is_raw_input_file(entry.path()) {
                AutoImportMediaKind::Raw
            } else {
                AutoImportMediaKind::Jpeg
            },
            size: metadata.len(),
            modified,
            modified_ns: system_time_ns(modified)?,
        });
    }
    Ok(sources)
}

fn compare_sources(left: &SourceFile, right: &SourceFile) -> Ordering {
    left.modified_ns
        .cmp(&right.modified_ns)
        .then_with(|| left.stem_key.cmp(&right.stem_key))
        .then_with(|| media_rank(left.media_kind).cmp(&media_rank(right.media_kind)))
        .then_with(|| left.relative_path_key.cmp(&right.relative_path_key))
}

const fn media_rank(kind: AutoImportMediaKind) -> u8 {
    match kind {
        AutoImportMediaKind::Raw => 0,
        AutoImportMediaKind::Jpeg => 1,
    }
}

fn is_auto_import_source(path: &Path) -> bool {
    if is_raw_input_file(path) {
        return true;
    }
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg")
        })
}

fn import_source(
    config: &AutoImportConfig,
    device: &AutoImportDevice,
    source: &SourceFile,
    bar: &ProgressBar,
) -> Result<ImportDisposition> {
    if let Some(asset) = config.catalog.find_source(
        source.storage.id,
        &source.relative_path_key,
        source.modified_ns,
    )? {
        return reuse_asset(config, source, &asset, bar);
    }

    let group: Option<AutoImportGroup> =
        config
            .catalog
            .find_group(device.id, &source.stem_key, source.modified_ns)?;
    if let Some(asset) = group
        .as_ref()
        .map(|group| config.catalog.find_group_asset(group.id, source.media_kind))
        .transpose()?
        .flatten()
    {
        config
            .catalog
            .record_source(source_record(asset.id, source)?)?;
        return reuse_asset(config, source, &asset, bar);
    }

    let candidates = config
        .catalog
        .find_filename_candidates(&source.filename_key, source.media_kind)?;
    if let Some(asset) = candidates.iter().find(|asset| {
        asset.device_id == device.id && asset.source_modified_ns == source.modified_ns
    }) {
        config
            .catalog
            .record_source(source_record(asset.id, source)?)?;
        return reuse_asset(config, source, asset, bar);
    }

    let mut source_identity = None;
    for mut candidate in candidates {
        let identity = source_identity
            .get_or_insert_with(|| identity_or_default(&config.exiftool, &source.path, device));
        if candidate.identity.is_empty() {
            let active = config.input_root.join(&candidate.active_filename);
            if active.is_file() {
                let read = identity_or_default_without_device(&config.exiftool, &active);
                if !read.is_empty() {
                    config.catalog.update_identity(candidate.id, &read)?;
                    candidate.identity = read;
                }
            }
        }
        if strong_identity_match(identity, &candidate.identity) {
            config
                .catalog
                .record_source(source_record(candidate.id, source)?)?;
            return reuse_asset(config, source, &candidate, bar);
        }
    }

    let preferred_stem = group.as_ref().map_or(source.stem.as_str(), |group| {
        group.destination_stem.as_str()
    });
    let destination = resolve_destination(
        config,
        device,
        source,
        preferred_stem,
        group.is_some(),
        &mut source_identity,
    )?;
    match destination {
        Destination::Adopt {
            destination_filename,
            active_filename,
            destination_stem,
            identity,
        } => {
            config.catalog.record_import(import_record(
                device,
                source,
                destination_stem,
                destination_filename,
                active_filename,
                *identity,
            )?)?;
            Ok(ImportDisposition::Adopted)
        }
        Destination::Copy {
            mut path,
            mut destination_stem,
            preserve_group_stem,
        } => {
            for _ in 0..MAX_DESTINATION_ATTEMPTS {
                match copy_atomic(source, &path, bar)? {
                    PublishResult::Published => {
                        let identity = identity_or_default(&config.exiftool, &path, device);
                        let destination_filename = file_name_text(&path)?;
                        config.catalog.record_import(import_record(
                            device,
                            source,
                            destination_stem,
                            destination_filename.clone(),
                            destination_filename,
                            identity,
                        )?)?;
                        return Ok(ImportDisposition::Imported);
                    }
                    PublishResult::Occupied => {
                        if let Some(Destination::Adopt {
                            destination_filename,
                            active_filename,
                            destination_stem: adopted_stem,
                            identity,
                        }) = adopt_existing_destination(
                            config,
                            device,
                            source,
                            &destination_stem,
                            &path,
                            &mut source_identity,
                            UntrackedAdoption::RequireIdentity,
                        )? {
                            config.catalog.record_import(import_record(
                                device,
                                source,
                                adopted_stem,
                                destination_filename,
                                active_filename,
                                *identity,
                            )?)?;
                            return Ok(ImportDisposition::Adopted);
                        }
                        bar.inc_length(source.size);
                        let next = next_available_filename(
                            &config.input_root,
                            &destination_stem,
                            &source.extension,
                        )?;
                        if !preserve_group_stem {
                            destination_stem = file_stem_text(&next)?;
                        }
                        path = next;
                    }
                }
            }
            bail!(
                "could not find a free destination name for {} after {} attempts",
                source.filename,
                MAX_DESTINATION_ATTEMPTS
            )
        }
    }
}

fn reuse_asset(
    config: &AutoImportConfig,
    source: &SourceFile,
    asset: &AutoImportAsset,
    bar: &ProgressBar,
) -> Result<ImportDisposition> {
    let active = config.input_root.join(&asset.active_filename);
    if active.is_file() {
        return Ok(ImportDisposition::Skipped);
    }
    let requested = config.input_root.join(&asset.destination_filename);
    if let Some(successor) = validated_dng_successor(config, &requested)? {
        let active_filename = file_name_text(successor.active())?;
        config
            .catalog
            .update_active_filename(asset.id, &active_filename)?;
        return Ok(ImportDisposition::Skipped);
    }
    if requested.is_file() {
        config
            .catalog
            .update_active_filename(asset.id, &asset.destination_filename)?;
        return Ok(ImportDisposition::Skipped);
    }

    match copy_atomic(source, &requested, bar)? {
        PublishResult::Published => {
            config
                .catalog
                .update_active_filename(asset.id, &asset.destination_filename)?;
            let identity = identity_or_default_without_device(&config.exiftool, &requested);
            if !identity.is_empty() {
                config.catalog.update_identity(asset.id, &identity)?;
            }
            Ok(ImportDisposition::Imported)
        }
        PublishResult::Occupied if requested.is_file() => {
            config
                .catalog
                .update_active_filename(asset.id, &asset.destination_filename)?;
            Ok(ImportDisposition::Skipped)
        }
        PublishResult::Occupied => {
            bail!(
                "destination became occupied but is not a file: {}",
                requested.display()
            )
        }
    }
}

enum Destination {
    Adopt {
        destination_filename: String,
        active_filename: String,
        destination_stem: String,
        identity: Box<AutoImportIdentity>,
    },
    Copy {
        path: PathBuf,
        destination_stem: String,
        preserve_group_stem: bool,
    },
}

#[derive(Clone, Copy)]
enum UntrackedAdoption {
    AllowNameAndTime,
    RequireIdentity,
}

fn resolve_destination(
    config: &AutoImportConfig,
    device: &AutoImportDevice,
    source: &SourceFile,
    preferred_stem: &str,
    existing_group: bool,
    source_identity: &mut Option<AutoImportIdentity>,
) -> Result<Destination> {
    let preferred = config
        .input_root
        .join(format!("{preferred_stem}.{}", source.extension));
    if let Some(adopted) = adopt_existing_destination(
        config,
        device,
        source,
        preferred_stem,
        &preferred,
        source_identity,
        UntrackedAdoption::AllowNameAndTime,
    )? {
        return Ok(adopted);
    }

    if existing_group {
        if !preferred.exists() {
            return Ok(Destination::Copy {
                path: preferred,
                destination_stem: preferred_stem.to_string(),
                preserve_group_stem: true,
            });
        }
        let path = next_available_filename(&config.input_root, preferred_stem, &source.extension)?;
        return Ok(Destination::Copy {
            destination_stem: preferred_stem.to_string(),
            path,
            preserve_group_stem: true,
        });
    }

    let occupied = files_with_stem(&config.input_root, preferred_stem)?;
    let catalog_occupied = config.catalog.destination_stem_exists(preferred_stem)?;
    if occupied.is_empty() && !catalog_occupied {
        return Ok(Destination::Copy {
            path: preferred,
            destination_stem: preferred_stem.to_string(),
            preserve_group_stem: false,
        });
    }
    let identity = source_identity
        .get_or_insert_with(|| identity_or_default(&config.exiftool, &source.path, device));
    if !catalog_occupied
        && occupied.iter().any(|candidate| {
            strong_identity_match(
                identity,
                &identity_or_default_without_device(&config.exiftool, candidate),
            )
        })
    {
        return Ok(Destination::Copy {
            path: preferred,
            destination_stem: preferred_stem.to_string(),
            preserve_group_stem: false,
        });
    }

    let stem = next_available_group_stem(config, preferred_stem)?;
    Ok(Destination::Copy {
        path: config
            .input_root
            .join(format!("{stem}.{}", source.extension)),
        destination_stem: stem,
        preserve_group_stem: false,
    })
}

fn adopt_existing_destination(
    config: &AutoImportConfig,
    device: &AutoImportDevice,
    source: &SourceFile,
    destination_stem: &str,
    requested: &Path,
    source_identity: &mut Option<AutoImportIdentity>,
    untracked_adoption: UntrackedAdoption,
) -> Result<Option<Destination>> {
    if requested.is_file() {
        let metadata = fs::metadata(requested)
            .with_context(|| format!("reading existing import {}", requested.display()))?;
        let same_name_and_time = system_time_ns(metadata.modified()?)? == source.modified_ns;
        let tracked = config
            .catalog
            .find_destination_asset(&file_name_text(requested)?)?;
        let may_adopt_by_name_and_time = same_name_and_time
            && match tracked.as_ref() {
                Some(asset) => asset.device_id == device.id,
                None => matches!(untracked_adoption, UntrackedAdoption::AllowNameAndTime),
            };
        let identity = identity_or_default(&config.exiftool, requested, device);
        let matches_identity = if may_adopt_by_name_and_time {
            true
        } else {
            let source_identity = source_identity
                .get_or_insert_with(|| identity_or_default(&config.exiftool, &source.path, device));
            strong_identity_match(source_identity, &identity)
        };
        if matches_identity {
            let filename = file_name_text(requested)?;
            return Ok(Some(Destination::Adopt {
                destination_filename: filename.clone(),
                active_filename: filename,
                destination_stem: destination_stem.to_string(),
                identity: Box::new(identity),
            }));
        }
    }

    if let Some(successor) = validated_dng_successor(config, requested)? {
        let active = successor.active();
        let identity = identity_or_default_without_device(&config.exiftool, active);
        let source_identity = source_identity
            .get_or_insert_with(|| identity_or_default(&config.exiftool, &source.path, device));
        if strong_identity_match(source_identity, &identity) {
            return Ok(Some(Destination::Adopt {
                destination_filename: file_name_text(requested)?,
                active_filename: file_name_text(active)?,
                destination_stem: destination_stem.to_string(),
                identity: Box::new(identity),
            }));
        }
    }
    Ok(None)
}

fn validated_dng_successor(
    config: &AutoImportConfig,
    requested: &Path,
) -> Result<Option<crate::app::dng::PreparedRawSource>> {
    if !is_raw_input_file(requested)
        || requested
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dng"))
    {
        return Ok(None);
    }
    let candidate = requested.with_extension("dng");
    if !candidate.is_file() {
        return Ok(None);
    }
    config.dng_fallback.existing_successor(requested)
}

fn import_record(
    device: &AutoImportDevice,
    source: &SourceFile,
    destination_stem: String,
    destination_filename: String,
    active_filename: String,
    identity: AutoImportIdentity,
) -> Result<AutoImportRecord> {
    Ok(AutoImportRecord {
        device_id: device.id,
        storage_id: source.storage.id,
        source_stem: source.stem.clone(),
        source_stem_key: source.stem_key.clone(),
        source_modified_ns: source.modified_ns,
        destination_stem,
        media_kind: source.media_kind,
        source_filename: source.filename.clone(),
        source_filename_key: source.filename_key.clone(),
        source_size_bytes: i64_size(source.size)?,
        relative_path: source.relative_path.clone(),
        relative_path_key: source.relative_path_key.clone(),
        destination_filename,
        active_filename,
        identity,
    })
}

fn source_record(asset_id: i64, source: &SourceFile) -> Result<AutoImportSourceRecord> {
    Ok(AutoImportSourceRecord {
        asset_id,
        storage_id: source.storage.id,
        relative_path: source.relative_path.clone(),
        relative_path_key: source.relative_path_key.clone(),
        source_filename: source.filename.clone(),
        source_modified_ns: source.modified_ns,
        source_size_bytes: i64_size(source.size)?,
    })
}

fn identity_or_default(
    exiftool: &Path,
    source: &Path,
    device: &AutoImportDevice,
) -> AutoImportIdentity {
    let mut identity = identity_or_default_without_device(exiftool, source);
    if identity.camera_serial.is_none() {
        identity.camera_serial.clone_from(&device.serial);
    }
    identity
}

fn identity_or_default_without_device(exiftool: &Path, source: &Path) -> AutoImportIdentity {
    read_identity(exiftool, source).unwrap_or_default()
}

enum PublishResult {
    Published,
    Occupied,
}

fn copy_atomic(
    source: &SourceFile,
    destination: &Path,
    progress: &ProgressBar,
) -> Result<PublishResult> {
    let parent = destination.parent().with_context(|| {
        format!(
            "import destination has no parent: {}",
            destination.display()
        )
    })?;
    let before = fs::metadata(&source.path)
        .with_context(|| format!("reading camera file {}", source.path.display()))?;
    if before.len() != source.size || before.modified().ok() != Some(source.modified) {
        bail!("camera file changed before transfer began");
    }

    let mut temporary = NamedTempFile::with_prefix_in(".mini-film-auto-import-", parent)
        .with_context(|| format!("creating auto-import staging file in {}", parent.display()))?;
    let mut reader = BufReader::with_capacity(
        COPY_BUFFER_SIZE,
        File::open(&source.path)
            .with_context(|| format!("opening camera file {}", source.path.display()))?,
    );
    {
        let mut writer = BufWriter::with_capacity(COPY_BUFFER_SIZE, temporary.as_file_mut());
        let mut buffer = [0_u8; COPY_BUFFER_SIZE];
        let mut copied = 0_u64;
        loop {
            let read = reader
                .read(&mut buffer)
                .with_context(|| format!("reading camera file {}", source.path.display()))?;
            if read == 0 {
                break;
            }
            writer
                .write_all(&buffer[..read])
                .with_context(|| format!("writing staged import {}", destination.display()))?;
            let read = u64::try_from(read).expect("copy buffer length fits u64");
            copied += read;
            progress.inc(read);
        }
        writer
            .flush()
            .with_context(|| format!("flushing staged import {}", destination.display()))?;
        if copied != source.size {
            bail!(
                "camera file expected {} bytes, copied {} bytes",
                source.size,
                copied
            );
        }
    }
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("syncing staged import {}", destination.display()))?;
    let after = fs::metadata(&source.path)
        .with_context(|| format!("rechecking camera file {}", source.path.display()))?;
    if after.len() != source.size || after.modified().ok() != Some(source.modified) {
        bail!("camera file changed during transfer");
    }
    set_file_mtime(
        temporary.path(),
        FileTime::from_system_time(source.modified),
    )
    .with_context(|| format!("preserving timestamp for {}", destination.display()))?;

    match fs::hard_link(temporary.path(), destination) {
        Ok(()) => {
            File::open(parent)
                .and_then(|directory| directory.sync_all())
                .with_context(|| format!("syncing auto-import directory {}", parent.display()))?;
            Ok(PublishResult::Published)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            Ok(PublishResult::Occupied)
        }
        Err(error) => Err(error)
            .with_context(|| format!("publishing auto-import file {}", destination.display())),
    }
}

fn files_with_stem(root: &Path, stem: &str) -> Result<Vec<PathBuf>> {
    let mut matches = fs::read_dir(root)
        .with_context(|| format!("reading daemon input {}", root.display()))?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_auto_import_source(path))
        .filter(|path| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(stem))
        })
        .collect::<Vec<_>>();
    matches.sort();
    Ok(matches)
}

fn next_available_group_stem(config: &AutoImportConfig, base: &str) -> Result<String> {
    for suffix in 1..=MAX_DESTINATION_ATTEMPTS {
        let candidate = format!("{base}-{suffix}");
        if files_with_stem(&config.input_root, &candidate)?.is_empty()
            && !config.catalog.destination_stem_exists(&candidate)?
        {
            return Ok(candidate);
        }
    }
    bail!(
        "could not find a free destination stem for {base:?} after {} attempts",
        MAX_DESTINATION_ATTEMPTS
    )
}

fn next_available_filename(root: &Path, base: &str, extension: &str) -> Result<PathBuf> {
    for suffix in 1..=MAX_DESTINATION_ATTEMPTS {
        let candidate = root.join(format!("{base}-{suffix}.{extension}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    bail!(
        "could not find a free destination filename for {base:?} after {} attempts",
        MAX_DESTINATION_ATTEMPTS
    )
}

fn system_time_ns(value: SystemTime) -> Result<i64> {
    let duration = value
        .duration_since(UNIX_EPOCH)
        .context("camera file modification time is before the Unix epoch")?;
    let nanoseconds =
        u128::from(duration.as_secs()) * 1_000_000_000 + u128::from(duration.subsec_nanos());
    i64::try_from(nanoseconds).context("camera file modification time does not fit SQLite INTEGER")
}

fn i64_size(size: u64) -> Result<i64> {
    i64::try_from(size).context("camera file size does not fit SQLite INTEGER")
}

fn file_name_text(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string)
        .with_context(|| format!("path has no UTF-8 filename: {}", path.display()))
}

fn file_stem_text(path: &Path) -> Result<String> {
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(str::to_string)
        .with_context(|| format!("path has no UTF-8 stem: {}", path.display()))
}

fn sanitize_thread_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .take(32)
        .collect()
}

fn import_spinner_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner:.green} import {msg:.72}").unwrap()
}

fn import_progress_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{spinner:.green} import [{wide_bar:.yellow/blue}] {bytes:>10}/{total_bytes:<10} {bytes_per_sec:>12} {msg:.52}",
    )
    .unwrap()
    .progress_chars("█▌░")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage() -> AutoImportStorage {
        AutoImportStorage {
            id: 1,
            device_id: 1,
            key: "card".to_string(),
        }
    }

    #[test]
    fn scan_orders_raw_before_jpeg_for_same_capture() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("DSC_0001.JPG"), b"jpeg").unwrap();
        fs::write(root.path().join("DSC_0001.NEF"), b"raw").unwrap();
        let now = FileTime::from_unix_time(1_700_000_000, 0);
        set_file_mtime(root.path().join("DSC_0001.JPG"), now).unwrap();
        set_file_mtime(root.path().join("DSC_0001.NEF"), now).unwrap();

        let mut sources = scan_storage(&storage(), root.path()).unwrap();
        sources.sort_by(compare_sources);
        assert_eq!(sources[0].filename, "DSC_0001.NEF");
        assert_eq!(sources[1].filename, "DSC_0001.JPG");
    }

    #[test]
    fn atomic_copy_preserves_mtime_and_never_overwrites() {
        let source_root = tempfile::tempdir().unwrap();
        let destination_root = tempfile::tempdir().unwrap();
        let source_path = source_root.path().join("DSC_0001.NEF");
        fs::write(&source_path, b"camera raw").unwrap();
        let modified = FileTime::from_unix_time(1_700_000_000, 123_000_000);
        set_file_mtime(&source_path, modified).unwrap();
        let source = scan_storage(&storage(), source_root.path())
            .unwrap()
            .pop()
            .unwrap();
        let destination = destination_root.path().join("DSC_0001.NEF");
        let progress = ProgressBar::hidden();

        assert!(matches!(
            copy_atomic(&source, &destination, &progress).unwrap(),
            PublishResult::Published
        ));
        assert_eq!(fs::read(&destination).unwrap(), b"camera raw");
        assert_eq!(
            system_time_ns(fs::metadata(&destination).unwrap().modified().unwrap()).unwrap(),
            source.modified_ns
        );

        fs::write(&source_path, b"changed raw").unwrap();
        let changed = scan_storage(&storage(), source_root.path())
            .unwrap()
            .pop()
            .unwrap();
        assert!(matches!(
            copy_atomic(&changed, &destination, &progress).unwrap(),
            PublishResult::Occupied
        ));
        assert_eq!(fs::read(&destination).unwrap(), b"camera raw");
    }

    #[test]
    fn numeric_collision_names_leave_the_original_untouched() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("DSC_0001.NEF"), b"first").unwrap();
        fs::write(root.path().join("DSC_0001-1.JPG"), b"second").unwrap();
        let output = tempfile::tempdir().unwrap();
        let config = AutoImportConfig {
            input_root: root.path().to_path_buf(),
            catalog: AutoImportCatalog::open(root.path(), output.path()).unwrap(),
            dng_fallback: DngFallbackConfig::default(),
            exiftool: PathBuf::from("exiftool"),
            progress: MultiProgress::new(),
            progress_anchor: ProgressBar::hidden(),
        };
        assert_eq!(
            next_available_group_stem(&config, "DSC_0001").unwrap(),
            "DSC_0001-2"
        );
        assert_eq!(
            next_available_filename(root.path(), "DSC_0001", "NEF")
                .unwrap()
                .file_name()
                .unwrap(),
            "DSC_0001-1.NEF"
        );
    }

    #[test]
    fn auto_import_accepts_raw_and_jpeg_but_not_heic_or_tiff() {
        assert!(is_auto_import_source(Path::new("frame.NEF")));
        assert!(is_auto_import_source(Path::new("frame.jpg")));
        assert!(!is_auto_import_source(Path::new("frame.heic")));
        assert!(!is_auto_import_source(Path::new("frame.tif")));
    }

    #[test]
    fn cataloged_destination_from_another_camera_requires_exif_match() {
        let root = tempfile::tempdir().unwrap();
        let first_camera = root.path().join("camera-a");
        let second_camera = root.path().join("camera-b");
        let input = root.path().join("input");
        let output = root.path().join("output");
        fs::create_dir_all(&first_camera).unwrap();
        fs::create_dir_all(&second_camera).unwrap();
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&output).unwrap();
        for directory in [&first_camera, &second_camera, &input] {
            fs::write(directory.join("DSC_0001.NEF"), b"raw").unwrap();
            set_file_mtime(
                directory.join("DSC_0001.NEF"),
                FileTime::from_unix_time(1_700_000_000, 0),
            )
            .unwrap();
        }

        let catalog = AutoImportCatalog::open(&input, &output).unwrap();
        let first = catalog
            .register_device("camera-a", "Camera A", Some("camera-a"))
            .unwrap();
        let first_card = catalog
            .register_storage(first.id, "card-a", "Card A")
            .unwrap();
        let first_source = scan_storage(&first_card, &first_camera)
            .unwrap()
            .pop()
            .unwrap();
        catalog
            .record_import(
                import_record(
                    &first,
                    &first_source,
                    "DSC_0001".to_string(),
                    "DSC_0001.NEF".to_string(),
                    "DSC_0001.NEF".to_string(),
                    AutoImportIdentity::default(),
                )
                .unwrap(),
            )
            .unwrap();

        let second = catalog
            .register_device("camera-b", "Camera B", Some("camera-b"))
            .unwrap();
        let second_card = catalog
            .register_storage(second.id, "card-b", "Card B")
            .unwrap();
        let second_source = scan_storage(&second_card, &second_camera)
            .unwrap()
            .pop()
            .unwrap();
        let config = AutoImportConfig {
            input_root: input,
            catalog,
            dng_fallback: DngFallbackConfig::default(),
            exiftool: root.path().join("missing-exiftool"),
            progress: MultiProgress::new(),
            progress_anchor: ProgressBar::hidden(),
        };
        let mut identity = None;
        let resolved = resolve_destination(
            &config,
            &second,
            &second_source,
            "DSC_0001",
            false,
            &mut identity,
        )
        .unwrap();
        let Destination::Copy {
            path,
            destination_stem,
            ..
        } = resolved
        else {
            panic!("different cameras without strong EXIF must not be adopted");
        };
        assert_eq!(path.file_name().unwrap(), "DSC_0001-1.NEF");
        assert_eq!(destination_stem, "DSC_0001-1");
    }

    #[test]
    fn newly_occupied_destination_requires_identity_before_adoption() {
        let root = tempfile::tempdir().unwrap();
        let camera = root.path().join("camera");
        let input = root.path().join("input");
        let output = root.path().join("output");
        fs::create_dir_all(&camera).unwrap();
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&output).unwrap();
        for directory in [&camera, &input] {
            fs::write(directory.join("DSC_0001.NEF"), b"raw").unwrap();
            set_file_mtime(
                directory.join("DSC_0001.NEF"),
                FileTime::from_unix_time(1_700_000_000, 0),
            )
            .unwrap();
        }

        let catalog = AutoImportCatalog::open(&input, &output).unwrap();
        let device = catalog
            .register_device("camera-a", "Camera A", Some("camera-a"))
            .unwrap();
        let card = catalog
            .register_storage(device.id, "card-a", "Card A")
            .unwrap();
        let source = scan_storage(&card, &camera).unwrap().pop().unwrap();
        let config = AutoImportConfig {
            input_root: input.clone(),
            catalog,
            dng_fallback: DngFallbackConfig::default(),
            exiftool: root.path().join("missing-exiftool"),
            progress: MultiProgress::new(),
            progress_anchor: ProgressBar::hidden(),
        };

        assert!(
            adopt_existing_destination(
                &config,
                &device,
                &source,
                "DSC_0001",
                &input.join("DSC_0001.NEF"),
                &mut None,
                UntrackedAdoption::RequireIdentity,
            )
            .unwrap()
            .is_none()
        );
        assert!(
            adopt_existing_destination(
                &config,
                &device,
                &source,
                "DSC_0001",
                &input.join("DSC_0001.NEF"),
                &mut None,
                UntrackedAdoption::AllowNameAndTime,
            )
            .unwrap()
            .is_some()
        );
    }

    #[cfg(unix)]
    #[test]
    fn untracked_dng_from_another_camera_does_not_absorb_raw() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let camera = root.path().join("camera-b");
        let input = root.path().join("input");
        let output = root.path().join("output");
        fs::create_dir_all(&camera).unwrap();
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&output).unwrap();
        fs::write(camera.join("DSC_0001.NEF"), b"camera b raw").unwrap();
        fs::write(input.join("DSC_0001.dng"), b"camera a dng").unwrap();
        let modified = FileTime::from_unix_time(1_700_000_000, 0);
        set_file_mtime(camera.join("DSC_0001.NEF"), modified).unwrap();
        set_file_mtime(input.join("DSC_0001.dng"), modified).unwrap();

        let exiftool = root.path().join("exiftool");
        fs::write(
            &exiftool,
            r#"#!/bin/sh
for last; do :; done
case "$last" in
  */camera-b/DSC_0001.NEF)
    printf '%s\n' '[{"ImageUniqueID":"camera-b-image","DateTimeOriginal":"2026:07:27 12:00:00","SubSecTimeOriginal":"22","OffsetTimeOriginal":"+02:00","SerialNumber":"camera-b"}]'
    ;;
  */input/DSC_0001.dng)
    printf '%s\n' '[{"FileType":"DNG","DNGVersion":"1.7.1.0","Compression":7,"BitsPerSample":16,"NewRawImageDigest":"0123456789abcdef0123456789abcdef","OriginalRawFileName":"DSC_0001.NEF","ImageWidth":8256,"ImageHeight":5504,"ImageUniqueID":"camera-a-image","Make":"NIKON","Model":"Z 9","DateTimeOriginal":"2026:07:27 12:00:00","SubSecTimeOriginal":11,"OffsetTimeOriginal":"+02:00","SerialNumber":"camera-a"}]'
    ;;
  *)
    printf '%s\n' '[]'
    ;;
esac
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&exiftool).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&exiftool, permissions).unwrap();

        let catalog = AutoImportCatalog::open(&input, &output).unwrap();
        let device = catalog
            .register_device("camera-b", "Camera B", Some("camera-b"))
            .unwrap();
        let card = catalog
            .register_storage(device.id, "card-b", "Card B")
            .unwrap();
        let source = scan_storage(&card, &camera).unwrap().pop().unwrap();
        let config = AutoImportConfig {
            input_root: input,
            catalog,
            dng_fallback: DngFallbackConfig::default().with_exiftool(exiftool.clone()),
            exiftool,
            progress: MultiProgress::new(),
            progress_anchor: ProgressBar::hidden(),
        };
        let resolved =
            resolve_destination(&config, &device, &source, "DSC_0001", false, &mut None).unwrap();
        let Destination::Copy {
            path,
            destination_stem,
            ..
        } = resolved
        else {
            panic!("DNG from another camera must not be adopted");
        };
        assert_eq!(path.file_name().unwrap(), "DSC_0001-1.NEF");
        assert_eq!(destination_stem, "DSC_0001-1");
    }

    #[test]
    fn catalog_persists_device_card_capture_asset_and_source_relationships() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("input");
        let output = root.path().join("output");
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&output).unwrap();
        let catalog = AutoImportCatalog::open(&input, &output).unwrap();
        let device = catalog
            .register_device("usb:04b0:044b:camera-1", "Nikon Z", Some("camera-1"))
            .unwrap();
        let first_card = catalog
            .register_storage(device.id, "gphoto2:store_1", "Card 1")
            .unwrap();
        let second_card = catalog
            .register_storage(device.id, "gphoto2:store_2", "Card 2")
            .unwrap();
        let identity = AutoImportIdentity {
            camera_serial: Some("camera-1".to_string()),
            capture_timestamp: Some("2026:07:27 12:00:00".to_string()),
            capture_subsecond: Some("12".to_string()),
            capture_offset: Some("+02:00".to_string()),
            ..AutoImportIdentity::default()
        };
        let asset = catalog
            .record_import(AutoImportRecord {
                device_id: device.id,
                storage_id: first_card.id,
                source_stem: "DSC_0001".to_string(),
                source_stem_key: "dsc_0001".to_string(),
                source_modified_ns: 1_700_000_000_000_000_000,
                destination_stem: "DSC_0001".to_string(),
                media_kind: AutoImportMediaKind::Raw,
                source_filename: "DSC_0001.NEF".to_string(),
                source_filename_key: "dsc_0001.nef".to_string(),
                source_size_bytes: 42,
                relative_path: "DCIM/100NIKON/DSC_0001.NEF".to_string(),
                relative_path_key: "dcim/100nikon/dsc_0001.nef".to_string(),
                destination_filename: "DSC_0001.NEF".to_string(),
                active_filename: "DSC_0001.NEF".to_string(),
                identity: identity.clone(),
            })
            .unwrap();
        catalog
            .record_source(AutoImportSourceRecord {
                asset_id: asset.id,
                storage_id: second_card.id,
                relative_path: "DCIM/100NIKON/DSC_0001.NEF".to_string(),
                relative_path_key: "dcim/100nikon/dsc_0001.nef".to_string(),
                source_filename: "DSC_0001.NEF".to_string(),
                source_modified_ns: 1_700_000_000_000_000_000,
                source_size_bytes: 42,
            })
            .unwrap();
        catalog
            .update_active_filename(asset.id, "DSC_0001.dng")
            .unwrap();
        drop(catalog);

        let reopened = AutoImportCatalog::open(&input, &output).unwrap();
        let device = reopened
            .register_device("usb:04b0:044b:camera-1", "Nikon Z", Some("camera-1"))
            .unwrap();
        let second_card = reopened
            .register_storage(device.id, "gphoto2:store_2", "Card 2")
            .unwrap();
        let stored = reopened
            .find_source(
                second_card.id,
                "dcim/100nikon/dsc_0001.nef",
                1_700_000_000_000_000_000,
            )
            .unwrap()
            .unwrap();
        assert_eq!(stored.active_filename, "DSC_0001.dng");
        assert_eq!(stored.identity, identity);
        assert_eq!(
            reopened
                .find_filename_candidates("dsc_0001.nef", AutoImportMediaKind::Raw)
                .unwrap()
                .len(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn validated_dng_successor_prevents_nef_restore() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let camera = root.path().join("camera");
        let input = root.path().join("input");
        let output = root.path().join("output");
        fs::create_dir_all(&camera).unwrap();
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&output).unwrap();
        fs::write(camera.join("frame.NEF"), b"camera raw").unwrap();
        fs::write(input.join("frame.dng"), b"converted raw").unwrap();
        let exiftool = root.path().join("exiftool");
        fs::write(
            &exiftool,
            "#!/bin/sh\nprintf '%s\\n' '[{\"FileType\":\"DNG\",\"DNGVersion\":\"1.7.1.0\",\"Compression\":7,\"BitsPerSample\":16,\"NewRawImageDigest\":\"0123456789abcdef0123456789abcdef\",\"OriginalRawFileName\":\"frame.NEF\",\"ImageWidth\":8256,\"ImageHeight\":5504,\"Make\":\"NIKON\",\"Model\":\"Z 9\",\"DateTimeOriginal\":\"2026:07:27 12:00:00\",\"SubSecTimeOriginal\":12,\"SerialNumber\":\"camera-1\"}]'\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&exiftool).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&exiftool, permissions).unwrap();

        let catalog = AutoImportCatalog::open(&input, &output).unwrap();
        let device = catalog
            .register_device("camera-1", "Nikon Z 9", Some("camera-1"))
            .unwrap();
        let card = catalog
            .register_storage(device.id, "card-1", "Card 1")
            .unwrap();
        let source = scan_storage(&card, &camera).unwrap().pop().unwrap();
        let asset = catalog
            .record_import(
                import_record(
                    &device,
                    &source,
                    "frame".to_string(),
                    "frame.NEF".to_string(),
                    "frame.NEF".to_string(),
                    AutoImportIdentity::default(),
                )
                .unwrap(),
            )
            .unwrap();
        let config = AutoImportConfig {
            input_root: input,
            catalog: catalog.clone(),
            dng_fallback: DngFallbackConfig::default().with_exiftool(exiftool.clone()),
            exiftool,
            progress: MultiProgress::new(),
            progress_anchor: ProgressBar::hidden(),
        };

        assert_eq!(
            reuse_asset(&config, &source, &asset, &ProgressBar::hidden()).unwrap(),
            ImportDisposition::Skipped
        );
        let stored = catalog
            .find_source(card.id, &source.relative_path_key, source.modified_ns)
            .unwrap()
            .unwrap();
        assert_eq!(stored.active_filename, "frame.dng");
        assert!(!config.input_root.join("frame.NEF").exists());
    }
}
