use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rayon::prelude::*;
use tempfile::Builder;
use walkdir::WalkDir;

use crate::app::apply::{ApplyArgs, ApplyJob, apply_resolved, resolve_grain_override};
use crate::app::export::validate_export_options;
use crate::app::profile::{ResolvedProfile, resolve_profile};
use crate::app::util::{half_cpu_thread_count, is_supported_raw_file, time_of_day_seed};
use crate::cli::{BatchOutputFormat, ExportOptions};

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(500);

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

struct PendingFile {
    path: PathBuf,
    process_at: Instant,
    size: u64,
    modified: Option<std::time::SystemTime>,
}

/// Run a watcher that applies one or more profiles whenever RAW files appear.
///
/// The input folder is monitored recursively. New/changed RAW files are debounced
/// and only processed after their size/mtime stays stable for the configured
/// period. Each output is named `<raw stem> - <profile stem>.<ext>` and stored
/// under the same directory layout as the input in the output root.
pub(crate) fn run_batch_daemon(args: BatchDaemonArgs) -> Result<()> {
    validate_export_options(&args.export)?;
    let jobs = resolve_batch_daemon_jobs(args.jobs)?;
    if !args.input.is_dir() {
        bail!(
            "batch-daemon input is not a directory: {}",
            args.input.display()
        );
    }
    fs::create_dir_all(&args.output)
        .with_context(|| format!("creating {}", args.output.display()))?;

    let debounce = Duration::from_secs(args.debounce_seconds);
    let temp_dir = Builder::new().prefix("mini-film-batch-daemon-").tempdir()?;
    let start = Instant::now();

    let profiles = resolve_daemon_profiles(&args, temp_dir.path())?;
    eprintln!("[{}] resolved profiles:", elapsed_human(start.elapsed()));
    for profile in &profiles {
        let source = if let Some(hald_path) = profile.resolved.hald_path.as_ref() {
            hald_path.display().to_string()
        } else {
            "(no hald)".to_string()
        };
        eprintln!(
            "[{}]   - {} => {} [{}]",
            elapsed_human(start.elapsed()),
            profile.selector,
            profile.stem,
            source
        );
        if !profile.resolved.rawtherapee_profiles.is_empty() {
            for pp3 in &profile.resolved.rawtherapee_profiles {
                eprintln!(
                    "[{}]       + pp3: {}",
                    elapsed_human(start.elapsed()),
                    pp3.display()
                );
            }
        }
    }
    let base_seed = args.grain_seed.unwrap_or_else(time_of_day_seed);
    let args = Arc::new(args);
    let profiles = Arc::new(profiles);

    let (watch_tx, watch_rx) = mpsc::channel();
    let mut watcher = RecommendedWatcher::new(
        move |result| {
            if watch_tx.send(result).is_err() {
                // If the receiver disappeared, just ignore; loop will exit.
            }
        },
        Config::default(),
    )
    .context("starting filesystem watcher")?;
    watcher
        .watch(&args.input, RecursiveMode::Recursive)
        .with_context(|| format!("watching {}", args.input.display()))?;

    eprintln!(
        "[{}] batch-daemon started, watching {}",
        elapsed_human(start.elapsed()),
        args.input.display()
    );
    eprintln!(
        "[{}] output: {}, profiles: {}, jobs: {}, debounce: {}s",
        elapsed_human(start.elapsed()),
        args.output.display(),
        profiles.len(),
        jobs,
        args.debounce_seconds
    );
    eprintln!("[{}] press Ctrl+C to stop", elapsed_human(start.elapsed()));

    let mut pending: HashMap<PathBuf, PendingFile> = HashMap::new();
    for raw in collect_batch_inputs(&args.input)? {
        queue_raw_file(&mut pending, raw, debounce);
    }

    loop {
        drain_watch_events(&watch_rx, &mut pending, debounce);
        let due = collect_due_paths(&mut pending, debounce);
        if !due.is_empty() {
            process_due_files(
                due,
                Arc::clone(&profiles),
                Arc::clone(&args),
                jobs,
                base_seed,
            )?;
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
        .map(|selector| {
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
            let mut resolved = resolve_profile(&apply_args, temp_dir)
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

fn drain_watch_events(
    watch_rx: &Receiver<Result<Event, notify::Error>>,
    pending: &mut HashMap<PathBuf, PendingFile>,
    debounce: Duration,
) {
    loop {
        match watch_rx.try_recv() {
            Ok(Ok(event)) => {
                if let EventKind::Create(_) | EventKind::Modify(_) | EventKind::Any = event.kind {
                    for path in event.paths {
                        queue_raw_file(pending, path, debounce);
                    }
                }
            }
            Ok(Err(err)) => eprintln!("watcher event error: {err}"),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => break,
        }
    }
}

fn process_due_files(
    due: Vec<PathBuf>,
    profiles: Arc<Vec<DaemonProfile>>,
    args: Arc<BatchDaemonArgs>,
    jobs: usize,
    base_seed: u64,
) -> Result<()> {
    let tasks: Vec<(PathBuf, usize)> = due
        .iter()
        .flat_map(|raw| (0..profiles.len()).map(|profile_index| (raw.clone(), profile_index)))
        .collect();

    let pool = rayon::ThreadPoolBuilder::new().num_threads(jobs).build()?;
    let results: Vec<_> = pool.install(|| {
        tasks
            .par_iter()
            .map(|(raw, profile_index)| {
                process_single_profile(
                    raw,
                    *profile_index,
                    &profiles[*profile_index],
                    &args,
                    base_seed,
                )
            })
            .collect()
    });

    let mut failures = Vec::new();
    for result in results {
        if let Err((path, err)) = result {
            failures.push((path, err));
        }
    }
    if !failures.is_empty() {
        for (path, err) in &failures {
            eprintln!("failed {}: {err:#}", path.display());
        }
        bail!(
            "batch-daemon processed {} failed profile jobs",
            failures.len()
        );
    }

    Ok(())
}

fn process_single_profile(
    raw: &Path,
    profile_index: usize,
    profile: &DaemonProfile,
    args: &BatchDaemonArgs,
    base_seed: u64,
) -> Result<(), (PathBuf, anyhow::Error)> {
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
    let seed = stable_profile_seed(base_seed, raw, profile_index as u64);
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
        None,
    )
    .map_err(|error| (raw.to_path_buf(), error))?;

    eprintln!(
        "wrote {} (raw={}, profile_selector={}, profile_resolved={})",
        output.display(),
        raw.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown"),
        profile.selector,
        profile.stem
    );
    Ok(())
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
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    Ok(output_root.join(parent).join(format!(
        "{} - {}.{}",
        sanitize_filename::sanitize(raw_stem),
        sanitize_filename::sanitize(profile_stem),
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

fn collect_due_paths(
    pending: &mut HashMap<PathBuf, PendingFile>,
    debounce: Duration,
) -> Vec<PathBuf> {
    let now = Instant::now();
    let mut next = HashMap::new();
    let mut due = Vec::new();

    for state in pending.drain() {
        if state.1.process_at > now {
            next.insert(state.0, state.1);
            continue;
        }

        if let Ok(metadata) = fs::metadata(&state.1.path) {
            let size = metadata.len();
            let modified = metadata.modified().ok();
            if size == state.1.size && modified == state.1.modified {
                due.push(state.1.path);
            } else {
                next.insert(
                    state.0,
                    PendingFile {
                        process_at: now + debounce,
                        size,
                        modified,
                        ..state.1
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

    #[test]
    fn daemon_output_path_keeps_input_tree_and_appends_profile_stem() {
        let output = daemon_output_path(
            Path::new("/in"),
            Path::new("/out"),
            BatchOutputFormat::Jpg,
            Path::new("/in/day/DSC_0001.NEF"),
            "Portra 400 grainy",
        )
        .unwrap();
        assert_eq!(
            output,
            Path::new("/out/day/DSC_0001 - Portra 400 grainy.jpg")
        );
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
}
