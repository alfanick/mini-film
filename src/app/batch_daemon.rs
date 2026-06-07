use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use notify::{
    Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{AccessKind, ModifyKind},
};
use tempfile::Builder;
use walkdir::WalkDir;

use crate::app::apply::{ApplyArgs, ApplyJob, apply_resolved, resolve_grain_override};
use crate::app::export::validate_export_options;
use crate::app::profile::{ResolvedProfile, resolve_profile};
use crate::app::progress::{
    ApplyProgress, StageEstimates, batch_progress_style, file_progress_style, format_duration,
    progress_length,
};
use crate::app::util::{half_cpu_thread_count, is_supported_raw_file, time_of_day_seed};
use crate::cli::{BatchOutputFormat, ExportOptions};
use indicatif::{MultiProgress, ProgressBar};

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(75);

pub(crate) struct BatchDaemonArgs {
    pub(crate) input: PathBuf,
    pub(crate) output: PathBuf,
    pub(crate) profile: Vec<String>,
    pub(crate) hald_dir: PathBuf,
    pub(crate) profiles_root: PathBuf,
    pub(crate) hald_level: u32,
    pub(crate) rawtherapee: PathBuf,
    pub(crate) convert: PathBuf,
    pub(crate) no_grain: bool,
    pub(crate) grain: Option<String>,
    pub(crate) grain_preset: Option<String>,
    pub(crate) grain_seed: Option<u64>,
    pub(crate) jobs: Option<usize>,
    pub(crate) debounce_seconds: u64,
    pub(crate) output_format: BatchOutputFormat,
    pub(crate) export: ExportOptions,
}

struct DaemonProfile {
    selector: String,
    stem: String,
    resolved: ResolvedProfile,
}

struct PendingTask {
    raw: PathBuf,
    profile_index: usize,
}

struct InFlightTask {
    handle: thread::JoinHandle<Result<(), (PathBuf, anyhow::Error)>>,
}

struct PendingFile {
    path: PathBuf,
    process_at: Instant,
    size: u64,
    modified: Option<std::time::SystemTime>,
}

struct ProfileScheduleContext<'a> {
    input_root: &'a Path,
    output_root: &'a Path,
    output_format: BatchOutputFormat,
    skip_existing: bool,
}

/// Run a watcher that applies one or more profiles whenever RAW files appear.
///
/// The input folder is monitored recursively. New/changed RAW files are queued on
/// filesystem notifications and only processed after their size and mtime are
/// observed as stable.
pub(crate) fn run_batch_daemon(args: BatchDaemonArgs) -> Result<()> {
    validate_export_options(&args.export)?;
    let jobs = resolve_batch_daemon_jobs(args.jobs)?;
    if !args.input.is_dir() {
        bail!("daemon input is not a directory: {}", args.input.display());
    }
    fs::create_dir_all(&args.output)
        .with_context(|| format!("creating {}", args.output.display()))?;

    let debounce = Duration::from_secs(args.debounce_seconds);
    let temp_dir = Builder::new().prefix("mini-film-daemon-").tempdir()?;
    let start = Instant::now();

    let profiles = resolve_daemon_profiles(&args, temp_dir.path())?;
    let profiles = profiles.into_iter().map(Arc::new).collect::<Vec<_>>();
    let profiles = Arc::new(profiles);
    let base_seed = args.grain_seed.unwrap_or_else(time_of_day_seed);
    let args = Arc::new(args);

    let multi = MultiProgress::new();
    let batch = multi.add(ProgressBar::new(0));
    batch.set_style(batch_progress_style());
    batch.set_message("starting".to_string());

    batch.println(format!(
        "[{}] resolved profiles: {}",
        elapsed_human(start.elapsed()),
        profiles.len()
    ));
    for profile in &*profiles {
        let source = if let Some(hald_path) = profile.resolved.hald_path.as_ref() {
            hald_path.display().to_string()
        } else {
            "(no hald)".to_string()
        };
        batch.println(format!(
            "[{}]   - {} => {} [{}]",
            elapsed_human(start.elapsed()),
            profile.selector,
            profile.stem,
            source
        ));
        if !profile.resolved.rawtherapee_profiles.is_empty() {
            for pp3 in &profile.resolved.rawtherapee_profiles {
                batch.println(format!(
                    "[{}]       + pp3: {}",
                    elapsed_human(start.elapsed()),
                    pp3.display()
                ));
            }
        }
    }

    let (watch_tx, watch_rx) = mpsc::channel();
    let mut watcher = RecommendedWatcher::new(
        move |result| {
            if watch_tx.send(result).is_err() {
                // If the receiver disappeared, just exit the callback silently.
            }
        },
        Config::default(),
    )
    .context("starting filesystem watcher")?;
    watcher
        .watch(&args.input, RecursiveMode::Recursive)
        .with_context(|| format!("watching {}", args.input.display()))?;

    batch.println(format!(
        "[{}] daemon started, watching {}",
        elapsed_human(start.elapsed()),
        args.input.display()
    ));
    batch.println(format!(
        "[{}] output: {}, profiles: {}, jobs: {}, debounce: {}",
        elapsed_human(start.elapsed()),
        args.output.display(),
        profiles.len(),
        jobs,
        if debounce.is_zero() {
            "immediate".to_string()
        } else {
            format!("{}s", args.debounce_seconds)
        }
    ));
    batch.println(format!(
        "[{}] press Ctrl+C to stop",
        elapsed_human(start.elapsed())
    ));

    batch.set_message("waiting for pictures".to_string());

    let worker_bars: Vec<_> = (0..jobs)
        .map(|index| {
            let file = multi.add(ProgressBar::new(progress_length()));
            file.set_style(file_progress_style());
            file.set_message(format!("worker {} waiting", index + 1));
            file
        })
        .collect();
    let worker_bars = Arc::new(Mutex::new(worker_bars));

    let mut pending: HashMap<PathBuf, PendingFile> = HashMap::new();
    let startup_raws = collect_batch_inputs(&args.input)?;
    for raw in &startup_raws {
        queue_raw_file(&mut pending, raw.clone(), Duration::ZERO);
    }

    let mut queue: VecDeque<PendingTask> = VecDeque::new();
    let mut in_flight: Vec<InFlightTask> = Vec::new();
    let estimates = Arc::new(StageEstimates::default());
    let mut completed = 0u64;
    let mut failures = Vec::new();

    let queued_from_startup = schedule_pending_due_paths(
        &mut pending,
        Duration::ZERO,
        &mut queue,
        &profiles,
        &batch,
        &ProfileScheduleContext {
            input_root: &args.input,
            output_root: &args.output,
            output_format: args.output_format,
            skip_existing: true,
        },
    )?;
    if !startup_raws.is_empty() {
        batch.println(format!(
            "[{}] startup: {} files discovered, {} queued",
            elapsed_human(start.elapsed()),
            startup_raws.len(),
            queued_from_startup
        ));
    }

    loop {
        drain_watch_events(&watch_rx, &mut pending, debounce);
        schedule_pending_due_paths(
            &mut pending,
            debounce,
            &mut queue,
            &profiles,
            &batch,
            &ProfileScheduleContext {
                input_root: &args.input,
                output_root: &args.output,
                output_format: args.output_format,
                skip_existing: false,
            },
        )?;

        while in_flight.len() < jobs {
            let Some(task) = queue.pop_front() else {
                break;
            };
            let Some(profile) = profiles.get(task.profile_index).cloned() else {
                continue;
            };
            let bar = acquire_worker_bar(&worker_bars);
            let raw = task.raw.clone();
            let raw_name = raw
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("unknown")
                .to_string();
            let bar_pool = Arc::clone(&worker_bars);
            let thread_args = Arc::clone(&args);
            let thread_estimates = Arc::clone(&estimates);

            let handle = thread::spawn(move || {
                let profile_index = task.profile_index;
                let context = DaemonTaskContext {
                    args: thread_args,
                    base_seed,
                    estimates: thread_estimates,
                };
                let result = process_single_profile(
                    &raw,
                    &profile,
                    profile_index as u64,
                    &context,
                    &bar,
                    &raw_name,
                );
                release_worker_bar(&bar_pool, bar);
                result
            });
            in_flight.push(InFlightTask { handle });
        }

        let mut index = 0;
        while index < in_flight.len() {
            if !in_flight[index].handle.is_finished() {
                index += 1;
                continue;
            }

            let task = in_flight.swap_remove(index);
            completed += 1;
            batch.inc(1);

            match task.handle.join() {
                Ok(Ok(())) => {}
                Ok(Err((path, error))) => {
                    batch.println(format!("failed {}: {error:#}", path.display()));
                    failures.push((path, error));
                }
                Err(_) => {
                    batch.println("a worker thread panicked");
                    failures.push((PathBuf::from("worker thread"), anyhow!("worker panic")));
                }
            }
        }

        if queue.is_empty() && in_flight.is_empty() {
            batch.reset_eta();
            batch.set_length(0);
            batch.set_position(0);
            batch.set_message("waiting for pictures".to_string());
        } else {
            batch.set_message(format!(
                "queued {} running {} done {}",
                queue.len(),
                in_flight.len(),
                completed
            ));
        }

        if let Some((path, error)) = failures.pop() {
            return Err(anyhow!("daemon failed {}: {error:#}", path.display()));
        }

        std::thread::sleep(DEFAULT_POLL_INTERVAL);
    }
}

fn resolve_batch_daemon_jobs(jobs: Option<usize>) -> Result<usize> {
    let jobs = jobs.unwrap_or_else(half_cpu_thread_count);
    if jobs == 0 {
        bail!("--jobs must be at least 1");
    }
    Ok(jobs)
}

fn resolve_daemon_profiles(args: &BatchDaemonArgs, temp_dir: &Path) -> Result<Vec<DaemonProfile>> {
    args.profile
        .iter()
        .enumerate()
        .map(|(index, selector)| {
            let profile_tmp_dir = temp_dir.join(
                sanitize_filename::sanitize(format!("{:03}-{}", index + 1, selector)).into_owned(),
            );
            fs::create_dir_all(&profile_tmp_dir).with_context(|| {
                format!("creating profile temp dir {}", profile_tmp_dir.display())
            })?;

            let apply_args = ApplyArgs {
                raw: PathBuf::new(),
                output: PathBuf::new(),
                profile: selector.clone(),
                hald_dir: args.hald_dir.clone(),
                profiles_root: args.profiles_root.clone(),
                hald_level: args.hald_level,
                rawtherapee: args.rawtherapee.clone(),
                convert: args.convert.clone(),
                keep_intermediate: None,
                no_grain: args.no_grain,
                grain: args.grain.clone(),
                grain_preset: args.grain_preset.clone(),
                grain_seed: args.grain_seed,
                export: args.export.clone(),
            };
            let mut resolved = resolve_profile(&apply_args, &profile_tmp_dir)
                .with_context(|| format!("resolving profile {selector}"))?;
            if let Some(grain) =
                resolve_grain_override(args.grain.as_deref(), args.grain_preset.as_deref())?
            {
                resolved.grain = grain;
            }
            let stem = resolved.resolved_stem.clone();
            Ok(DaemonProfile {
                selector: selector.clone(),
                stem,
                resolved,
            })
        })
        .collect()
}

fn event_stability_delay(kind: &EventKind, debounce: Duration) -> Duration {
    if is_close_or_rename_event(kind) {
        Duration::ZERO
    } else if debounce.is_zero() {
        Duration::from_millis(100)
    } else {
        debounce
    }
}

fn is_close_or_rename_event(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Access(AccessKind::Close(_)) | EventKind::Modify(ModifyKind::Name(_))
    )
}

fn is_relevant_daemon_event(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Access(AccessKind::Close(_))
            | EventKind::Modify(ModifyKind::Data(_))
            | EventKind::Modify(ModifyKind::Metadata(_))
            | EventKind::Modify(ModifyKind::Name(_))
            | EventKind::Create(_)
            | EventKind::Any,
    )
}

fn drain_watch_events(
    watch_rx: &Receiver<Result<Event, notify::Error>>,
    pending: &mut HashMap<PathBuf, PendingFile>,
    debounce: Duration,
) {
    loop {
        match watch_rx.try_recv() {
            Ok(Ok(event)) => {
                if is_relevant_daemon_event(&event.kind) {
                    let delay = event_stability_delay(&event.kind, debounce);
                    for path in event.paths {
                        queue_raw_file(pending, path, delay);
                    }
                }
            }
            Ok(Err(_)) => {}
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => break,
        }
    }
}

fn schedule_pending_due_paths(
    pending: &mut HashMap<PathBuf, PendingFile>,
    debounce: Duration,
    queue: &mut VecDeque<PendingTask>,
    profiles: &[Arc<DaemonProfile>],
    batch: &ProgressBar,
    context: &ProfileScheduleContext<'_>,
) -> Result<usize> {
    let due = collect_due_paths(pending, debounce);
    if due.is_empty() {
        return Ok(0);
    }

    let mut queued_count = 0u64;
    for raw in due {
        queued_count += enqueue_profile_jobs(queue, profiles, raw, context)? as u64;
    }
    if queued_count == 0 {
        return Ok(0);
    }
    batch.inc_length(queued_count);
    Ok(queued_count as usize)
}

fn enqueue_profile_jobs(
    queue: &mut VecDeque<PendingTask>,
    profiles: &[Arc<DaemonProfile>],
    raw: PathBuf,
    context: &ProfileScheduleContext<'_>,
) -> Result<usize> {
    let mut queued = 0usize;
    for (profile_index, profile) in profiles.iter().enumerate() {
        if context.skip_existing {
            let expected_output = daemon_output_path(
                context.input_root,
                context.output_root,
                context.output_format,
                &raw,
                &profile.stem,
            )?;
            if expected_output.exists() {
                continue;
            }
        }

        queue.push_back(PendingTask {
            raw: raw.clone(),
            profile_index,
        });
        queued += 1;
    }
    Ok(queued)
}

fn collect_batch_inputs(input: &Path) -> Result<Vec<PathBuf>> {
    let mut raws = Vec::new();
    for entry in WalkDir::new(input).into_iter().filter_map(Result::ok) {
        if entry.file_type().is_file() && is_supported_raw_file(entry.path()) {
            raws.push(entry.path().to_path_buf());
        }
    }
    raws.sort();
    Ok(raws)
}

struct DaemonTaskContext {
    args: Arc<BatchDaemonArgs>,
    base_seed: u64,
    estimates: Arc<StageEstimates>,
}

fn process_single_profile(
    raw: &Path,
    profile: &DaemonProfile,
    profile_index: u64,
    context: &DaemonTaskContext,
    file: &ProgressBar,
    raw_name: &str,
) -> Result<(), (PathBuf, anyhow::Error)> {
    let args = &context.args;
    let output = daemon_output_path(
        &args.input,
        &args.output,
        args.output_format,
        raw,
        &profile.stem,
    )
    .map_err(|err| (raw.to_path_buf(), err))?;

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(|err| (raw.to_path_buf(), err.into()))?;
    }

    let temp_dir = Builder::new()
        .prefix("mini-film-daemon-job-")
        .tempdir()
        .map_err(|err| (raw.to_path_buf(), err.into()))?;

    file.set_position(0);
    file.set_message(format!("{} -> {}: queued", raw_name, profile.stem));

    let file_start = Instant::now();
    let progress = ApplyProgress {
        file,
        started: file_start,
        estimates: Some(Arc::clone(&context.estimates)),
    };
    let seed = stable_profile_seed(context.base_seed, raw, profile_index);
    apply_resolved(
        ApplyJob {
            raw,
            output: &output,
            rawtherapee: &args.rawtherapee,
            convert: &args.convert,
            keep_intermediate: None,
            no_grain: args.no_grain,
            export: &args.export,
            quiet: true,
        },
        &profile.resolved,
        seed,
        temp_dir.path(),
        Some(&progress),
    )
    .map_err(|error| (raw.to_path_buf(), error))?;

    file.set_message(format!(
        "{} -> {}: done in {}",
        raw_name,
        profile.stem,
        format_duration(file_start.elapsed())
    ));
    Ok(())
}

fn acquire_worker_bar(bar_pool: &Arc<Mutex<Vec<ProgressBar>>>) -> ProgressBar {
    loop {
        if let Some(file) = bar_pool
            .lock()
            .expect("worker progress bar pool poisoned")
            .pop()
        {
            return file;
        }
        thread::yield_now();
    }
}

fn release_worker_bar(bar_pool: &Arc<Mutex<Vec<ProgressBar>>>, file: ProgressBar) {
    bar_pool
        .lock()
        .expect("worker progress bar pool poisoned")
        .push(file);
}

fn daemon_output_path(
    input_root: &Path,
    output_root: &Path,
    output_format: BatchOutputFormat,
    raw: &Path,
    profile_stem: &str,
) -> Result<PathBuf> {
    let relative = raw
        .strip_prefix(input_root)
        .with_context(|| format!("mapping {} under {}", raw.display(), input_root.display()))?;
    let raw_stem = relative
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| anyhow!("raw path has no file stem: {}", raw.display()))?;
    let profile_stem = sanitize_filename::sanitize(profile_stem);
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    Ok(output_root.join(parent).join(format!(
        "{}/{}.{}",
        profile_stem,
        sanitize_filename::sanitize(raw_stem),
        output_format.extension()
    )))
}

fn queue_raw_file(pending: &mut HashMap<PathBuf, PendingFile>, path: PathBuf, debounce: Duration) {
    if !path.is_file() || !is_supported_raw_file(&path) {
        return;
    }
    if let Ok(metadata) = fs::metadata(&path) {
        pending.insert(
            path.clone(),
            PendingFile {
                path: path.clone(),
                process_at: Instant::now() + debounce,
                size: metadata.len(),
                modified: metadata.modified().ok(),
            },
        );
    }
}

fn collect_due_paths(
    pending: &mut HashMap<PathBuf, PendingFile>,
    debounce: Duration,
) -> Vec<PathBuf> {
    let now = Instant::now();
    let mut next = HashMap::new();
    let mut due = Vec::new();

    for (key, state) in pending.drain() {
        if state.process_at > now {
            next.insert(key, state);
            continue;
        }

        if let Ok(metadata) = fs::metadata(&state.path) {
            let size = metadata.len();
            let modified = metadata.modified().ok();
            if size == state.size && modified == state.modified {
                due.push(state.path);
            } else {
                next.insert(
                    state.path.clone(),
                    PendingFile {
                        path: state.path,
                        process_at: now + debounce,
                        size,
                        modified,
                    },
                );
            }
        }
    }

    *pending = next;
    due
}

fn stable_profile_seed(base_seed: u64, path: &Path, profile_index: u64) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    profile_index.hash(&mut hasher);
    path.hash(&mut hasher);
    base_seed.hash(&mut hasher);
    hasher.finish()
}

fn elapsed_human(elapsed: Duration) -> String {
    format!("{:>8.2}s", elapsed.as_secs_f32())
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{
        AccessKind, AccessMode, CreateKind, DataChange, EventKind, MetadataKind, ModifyKind,
        RenameMode,
    };
    use std::collections::HashMap;

    #[test]
    fn daemon_output_path_keeps_input_tree_and_uses_profile_dir() {
        let output = daemon_output_path(
            Path::new("/in"),
            Path::new("/out"),
            BatchOutputFormat::Jpg,
            Path::new("/in/day/DSC_0001.NEF"),
            "Portra 400 grainy",
        )
        .unwrap();
        assert_eq!(output, Path::new("/out/day/Portra 400 grainy/DSC_0001.jpg"));
    }

    #[test]
    fn daemon_stable_profile_seed_changes_with_profile_and_path() {
        assert_eq!(
            stable_profile_seed(1, Path::new("a.RAW"), 0),
            stable_profile_seed(1, Path::new("a.RAW"), 0)
        );
        assert_ne!(
            stable_profile_seed(1, Path::new("a.RAW"), 0),
            stable_profile_seed(1, Path::new("a.RAW"), 1)
        );
        assert_ne!(
            stable_profile_seed(1, Path::new("a.RAW"), 0),
            stable_profile_seed(2, Path::new("a.RAW"), 0)
        );
    }

    #[test]
    fn resolve_batch_daemon_jobs_validates_bounds() {
        assert_eq!(resolve_batch_daemon_jobs(Some(4)).unwrap(), 4);
        assert!(resolve_batch_daemon_jobs(Some(0)).is_err());
    }

    #[test]
    fn event_helpers_recognize_close_or_rename_events() {
        assert_eq!(
            event_stability_delay(
                &EventKind::Access(AccessKind::Close(AccessMode::Any)),
                Duration::ZERO
            ),
            Duration::ZERO
        );
        assert_eq!(
            event_stability_delay(
                &EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                Duration::from_millis(250),
            ),
            Duration::ZERO
        );
        assert_eq!(
            event_stability_delay(
                &EventKind::Modify(ModifyKind::Data(DataChange::Any)),
                Duration::from_millis(250),
            ),
            Duration::from_millis(250)
        );
    }

    #[test]
    fn event_relevance_matches_expected_variants() {
        assert!(is_relevant_daemon_event(&EventKind::Access(
            AccessKind::Close(AccessMode::Any)
        )));
        assert!(is_relevant_daemon_event(&EventKind::Modify(
            ModifyKind::Data(DataChange::Any)
        )));
        assert!(is_relevant_daemon_event(&EventKind::Modify(
            ModifyKind::Metadata(MetadataKind::Any)
        )));
        assert!(is_relevant_daemon_event(&EventKind::Modify(
            ModifyKind::Name(RenameMode::To)
        )));
        assert!(is_relevant_daemon_event(&EventKind::Create(
            CreateKind::File
        )));
        assert!(is_relevant_daemon_event(&EventKind::Any));
        assert!(!is_relevant_daemon_event(&EventKind::Remove(
            notify::event::RemoveKind::File
        )));
    }

    #[test]
    fn collect_due_paths_returns_stable_raws_and_requeues_changed() {
        let root = tempfile::tempdir().unwrap();
        let raw = root.path().join("frame.NEF");
        fs::write(&raw, b"hello").unwrap();
        let metadata = fs::metadata(&raw).unwrap();

        let mut pending = HashMap::new();
        pending.insert(
            raw.clone(),
            PendingFile {
                path: raw.clone(),
                process_at: Instant::now() - Duration::from_millis(1),
                size: metadata.len(),
                modified: metadata.modified().ok(),
            },
        );
        let due = collect_due_paths(&mut pending, Duration::ZERO);
        assert_eq!(due.len(), 1);
        assert!(pending.is_empty());

        let metadata = fs::metadata(&raw).unwrap();
        pending.insert(
            raw.clone(),
            PendingFile {
                path: raw.clone(),
                process_at: Instant::now() - Duration::from_millis(1),
                size: metadata.len(),
                modified: metadata.modified().ok(),
            },
        );
        fs::write(&raw, b"hello world").unwrap();
        let due = collect_due_paths(&mut pending, Duration::ZERO);
        assert!(due.is_empty());
        assert!(pending.contains_key(&raw));

        let future = Instant::now() + Duration::from_secs(30);
        pending.insert(
            raw.clone(),
            PendingFile {
                path: raw.clone(),
                process_at: future,
                size: metadata.len(),
                modified: metadata.modified().ok(),
            },
        );
        let due = collect_due_paths(&mut pending, Duration::from_secs(30));
        assert!(due.is_empty());
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn queue_raw_file_ignores_non_raw_and_keeps_supported() {
        let root = tempfile::tempdir().unwrap();
        let mut pending = HashMap::new();

        let supported = root.path().join("frame.NEF");
        fs::write(&supported, b"raw").unwrap();
        let unsupported = root.path().join("notes.txt");
        fs::write(&unsupported, b"text").unwrap();

        queue_raw_file(&mut pending, supported.clone(), Duration::ZERO);
        queue_raw_file(&mut pending, unsupported.clone(), Duration::ZERO);

        assert!(pending.contains_key(&supported));
        assert!(!pending.contains_key(&unsupported));
    }
}
