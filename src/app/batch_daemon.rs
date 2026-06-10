use std::{
    collections::{HashMap, HashSet, VecDeque},
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

use crate::app::apply::{
    ApplyArgs, ApplyJob, apply_resolved, resolve_apply_effects, resolve_grain_override,
};
use crate::app::export::validate_export_options;
use crate::app::info::profile_info_text_for_selector;
use crate::app::nikon_wtu::{NikonWtuConfig, NikonWtuReceiver, start_nikon_wtu_receiver};
use crate::app::profile::{ResolvedProfile, resolve_profile};
use crate::app::progress::{
    ApplyProgress, StageEstimates, batch_progress_style, file_progress_style, format_duration,
    progress_length,
};
use crate::app::system_stats::{ResourceUsageSummary, sample_usage_block};
use crate::app::util::{half_cpu_thread_count, is_supported_raw_file, time_of_day_seed};
use crate::cli::{BatchOutputFormat, ExportOptions, LensCorrections};
use indicatif::{MultiProgress, ProgressBar};

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(75);
const RESOURCE_USAGE_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

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
    pub(crate) lens_corrections: LensCorrections,
    pub(crate) grain: Option<String>,
    pub(crate) grain_preset: Option<String>,
    pub(crate) grain_seed: Option<u64>,
    pub(crate) color_noise_iso_threshold: u32,
    pub(crate) jobs: Option<usize>,
    pub(crate) debounce_seconds: u64,
    pub(crate) nikon_wtu: Option<String>,
    pub(crate) nikon_wtu_port: u16,
    pub(crate) nikon_wtu_name: Option<String>,
    pub(crate) nikon_wtu_guid: Option<String>,
    pub(crate) output_format: BatchOutputFormat,
    pub(crate) export: ExportOptions,
}

struct DaemonProfile {
    selector: String,
    stem: String,
    resolved: ResolvedProfile,
    profile_report: String,
}

struct PendingTask {
    raw: PathBuf,
    profile_index: usize,
}

struct InFlightTask {
    profile_index: usize,
    raw: PathBuf,
    handle: thread::JoinHandle<DaemonFileResult>,
}

struct PendingFile {
    path: PathBuf,
    process_at: Instant,
    size: u64,
    modified: Option<std::time::SystemTime>,
}

struct DaemonProgressState {
    total_processed: u64,
    total_succeeded: u64,
    total_failed: u64,
    total_elapsed_ms: u64,
    started_at: Instant,
    files: Vec<DaemonFileResult>,
    profile_stats: Vec<DaemonProfileStats>,
    profile_output_dirs: Vec<HashSet<PathBuf>>,
    resource_usage: ResourceUsageSummary,
    last_resource_sample: Instant,
}

#[derive(Default, Clone, Copy)]
struct DaemonProfileStats {
    processed: u64,
    succeeded: u64,
    failed: u64,
    elapsed_ms: u64,
}

impl DaemonProfileStats {
    fn avg_ms(&self) -> u64 {
        if self.processed == 0 {
            return 0;
        }
        self.elapsed_ms / self.processed
    }
}

struct DaemonFileResult {
    raw: PathBuf,
    output: PathBuf,
    duration: Duration,
    profile_index: usize,
    error: Option<String>,
    lens_profile_status: String,
    sharpening_status: String,
    denoise_status: String,
}

fn lens_profile_status(corrections: LensCorrections, applied: bool) -> String {
    if !corrections.is_enabled() {
        return "lens-profile: disabled".to_string();
    }

    let mut parts = Vec::new();
    if corrections.distortion {
        parts.push("distortion");
    }
    if corrections.ca {
        parts.push("chromatic-aberration");
    }
    if corrections.vignetting {
        parts.push("vignetting");
    }

    format!(
        "lens-profile: requested[{}], found=true, applied={}",
        parts.join("+"),
        if applied { "yes" } else { "no" }
    )
}

impl Clone for DaemonFileResult {
    fn clone(&self) -> Self {
        Self {
            raw: self.raw.clone(),
            output: self.output.clone(),
            duration: self.duration,
            profile_index: self.profile_index,
            error: self.error.clone(),
            lens_profile_status: self.lens_profile_status.clone(),
            sharpening_status: self.sharpening_status.clone(),
            denoise_status: self.denoise_status.clone(),
        }
    }
}

impl DaemonProgressState {
    fn profile_stats_mut(&mut self, profile_index: usize) -> &mut DaemonProfileStats {
        while self.profile_stats.len() <= profile_index {
            self.profile_stats.push(DaemonProfileStats::default());
        }
        &mut self.profile_stats[profile_index]
    }

    fn record(&mut self, result: &DaemonFileResult) {
        self.total_processed += 1;
        self.total_elapsed_ms += result.duration.as_millis() as u64;
        if result.error.is_some() {
            self.total_failed += 1;
        } else {
            self.total_succeeded += 1;
        }
        let profile = self.profile_stats_mut(result.profile_index);
        profile.processed += 1;
        profile.elapsed_ms += result.duration.as_millis() as u64;
        if result.error.is_some() {
            profile.failed += 1;
        } else {
            profile.succeeded += 1;
        }
        self.files.push(result.clone());
        if let Some(parent) = result.output.parent() {
            self.profile_output_dirs_mut(result.profile_index)
                .insert(parent.to_path_buf());
        }
        if self.files.len() > 3000 {
            self.files.remove(0);
        }
    }

    fn profile_output_dirs_mut(&mut self, profile_index: usize) -> &mut HashSet<PathBuf> {
        while self.profile_output_dirs.len() <= profile_index {
            self.profile_output_dirs.push(HashSet::new());
        }
        &mut self.profile_output_dirs[profile_index]
    }

    fn sample_resources(&mut self) {
        if let Some(usage) = sample_usage_block() {
            self.resource_usage.add(&usage);
        }
    }

    fn resource_usage_if_needed(&mut self, now: Instant) {
        if now.duration_since(self.last_resource_sample) >= RESOURCE_USAGE_SAMPLE_INTERVAL {
            self.sample_resources();
            self.last_resource_sample = now;
        }
    }
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
    let nikon_wtu_config = if let Some(camera) = args
        .nikon_wtu
        .clone()
        .filter(|camera| !camera.trim().is_empty())
    {
        Some(NikonWtuConfig {
            camera,
            port: args.nikon_wtu_port,
            output_dir: args.input.clone(),
            computer_name: args.nikon_wtu_name.clone(),
            guid: args.nikon_wtu_guid.clone(),
        })
    } else {
        None
    };

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

    let nikon_wtu_receiver = if let Some(config) = nikon_wtu_config {
        batch.println(format!(
            "[{}] nikon-wtu: enabled, camera {}:{}, inbox {}",
            elapsed_human(start.elapsed()),
            config.camera,
            config.port,
            config.output_dir.display()
        ));
        Some(start_nikon_wtu_receiver(config)?)
    } else {
        None
    };

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
    let mut state = DaemonProgressState {
        total_processed: 0,
        total_succeeded: 0,
        total_failed: 0,
        total_elapsed_ms: 0,
        started_at: Instant::now(),
        files: Vec::new(),
        profile_stats: vec![DaemonProfileStats::default(); profiles.len()],
        profile_output_dirs: vec![HashSet::new(); profiles.len()],
        resource_usage: ResourceUsageSummary::default(),
        last_resource_sample: Instant::now(),
    };

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

    state.sample_resources();
    state.last_resource_sample = Instant::now();
    write_daemon_info_txt(
        &args.output,
        &args,
        &profiles,
        &state,
        Duration::ZERO,
        Some(&state.resource_usage),
    )?;

    loop {
        drain_nikon_wtu_logs(nikon_wtu_receiver.as_ref(), &batch, start);
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
            let worker_raw = raw.clone();
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
                    &worker_raw,
                    &profile,
                    profile_index as u64,
                    &profile.stem,
                    &context,
                    &bar,
                    &raw_name,
                );
                release_worker_bar(&bar_pool, bar);
                result
            });
            in_flight.push(InFlightTask {
                profile_index: task.profile_index,
                raw,
                handle,
            });
        }

        let mut index = 0;
        while index < in_flight.len() {
            if !in_flight[index].handle.is_finished() {
                index += 1;
                continue;
            }

            let task = in_flight.swap_remove(index);
            batch.inc(1);
            let result = match task.handle.join() {
                Ok(result) => result,
                Err(_) => DaemonFileResult {
                    raw: task.raw,
                    output: PathBuf::new(),
                    duration: Duration::ZERO,
                    profile_index: task.profile_index,
                    error: Some("worker thread panicked".to_string()),
                    lens_profile_status: lens_profile_status(LensCorrections::default(), false),
                    sharpening_status: sharpening_status(false),
                    denoise_status: denoise_status(false),
                },
            };
            if let Some(error) = &result.error {
                batch.println(format!("failed {}: {}", result.raw.display(), error));
            }
            state.record(&result);
            state.sample_resources();
            state.last_resource_sample = Instant::now();
            write_daemon_info_txt(
                &args.output,
                &args,
                &profiles,
                &state,
                start.elapsed(),
                Some(&state.resource_usage),
            )?;
        }

        state.resource_usage_if_needed(Instant::now());
        drain_nikon_wtu_logs(nikon_wtu_receiver.as_ref(), &batch, start);
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
                state.total_processed
            ));
        }

        std::thread::sleep(DEFAULT_POLL_INTERVAL);
    }
}

fn drain_nikon_wtu_logs(receiver: Option<&NikonWtuReceiver>, batch: &ProgressBar, start: Instant) {
    let Some(receiver) = receiver else {
        return;
    };
    for log in receiver.drain_logs() {
        batch.println(format!("[{}] {log}", elapsed_human(start.elapsed())));
    }
}

fn resolve_batch_daemon_jobs(jobs: Option<usize>) -> Result<usize> {
    let jobs = jobs.unwrap_or_else(half_cpu_thread_count);
    if jobs == 0 {
        bail!("--jobs must be at least 1");
    }
    Ok(jobs)
}

fn write_daemon_info_txt(
    output_root: &Path,
    args: &BatchDaemonArgs,
    profiles: &[Arc<DaemonProfile>],
    state: &DaemonProgressState,
    elapsed: Duration,
    resource_usage: Option<&ResourceUsageSummary>,
) -> Result<()> {
    use std::fmt::Write;

    let mut out = String::new();
    let started = chrono::Local::now();
    let started_str = started.format("%Y-%m-%d %H:%M:%S").to_string();
    let time_of_day = started.format("%H:%M:%S").to_string();
    let runtime = format_duration(elapsed);
    let uptime = format_duration(state.started_at.elapsed());

    writeln!(out, "mini-film daemon report").ok();
    writeln!(out, "Generated: {started_str}").ok();
    writeln!(out, "Time of day: {time_of_day}").ok();
    writeln!(out, "Timezone: {}", started.format("%:z")).ok();
    writeln!(out, "Mini-film version: {}", env!("CARGO_PKG_VERSION")).ok();
    writeln!(out, "Input directory: {}", args.input.display()).ok();
    writeln!(out, "Output directory: {}", args.output.display()).ok();
    writeln!(out, "Profiles: {}", profiles.len()).ok();
    writeln!(out, "Output format: {:?}", args.output_format).ok();
    writeln!(
        out,
        "Jobs: {}",
        args.jobs.unwrap_or_else(half_cpu_thread_count)
    )
    .ok();
    writeln!(out, "Elapsed: {runtime} (up since report start: {uptime})").ok();
    writeln!(
        out,
        "Files: processed={}, succeeded={}, failed={}",
        state.total_processed, state.total_succeeded, state.total_failed
    )
    .ok();
    append_resource_usage(&mut out, resource_usage);

    writeln!(out, "\nProfiles:").ok();
    for (index, profile) in profiles.iter().enumerate() {
        let stats = &state.profile_stats[index];
        writeln!(
            out,
            "  - [{}] {} => {} ({}/{} success/fail, avg {}/file ms)",
            index + 1,
            profile.selector,
            profile.stem,
            stats.succeeded,
            stats.failed,
            stats.avg_ms()
        )
        .ok();
        out.push_str("    Profile report:\n");
        for line in profile.profile_report.lines() {
            writeln!(out, "      {line}").ok();
        }
    }

    writeln!(out, "\nLatest files:").ok();
    for file in &state.files {
        if let Some(error) = &file.error {
            writeln!(
                out,
                "  FAILURE | {} | {} | {} | {} | {} | {}",
                file.raw.display(),
                file.output.display(),
                error,
                file.lens_profile_status,
                file.sharpening_status,
                file.denoise_status
            )
            .ok();
        } else {
            writeln!(
                out,
                "  OK | {} | {} | {} | {} | {} | {}",
                file.raw.display(),
                file.output.display(),
                format_duration(file.duration),
                file.lens_profile_status,
                file.sharpening_status,
                file.denoise_status
            )
            .ok();
        }
    }

    if let Some(parent) = output_root.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating output root parent {}", parent.display()))?;
    }
    if !output_root.exists() {
        fs::create_dir_all(output_root)
            .with_context(|| format!("creating output root {}", output_root.display()))?;
    }

    fs::write(output_root.join("info.txt"), out)?;

    for (index, profile) in profiles.iter().enumerate() {
        let report_for_profile =
            profile_daemon_info(args, profile, state, elapsed, resource_usage, index)?;
        for profile_dir in state
            .profile_output_dirs
            .get(index)
            .into_iter()
            .flat_map(|dirs| dirs.iter())
        {
            if let Err(error) = fs::create_dir_all(profile_dir) {
                return Err(anyhow::anyhow!(
                    "creating profile info directory {}: {error:#}",
                    profile_dir.display()
                ));
            }
            let report_path = profile_dir.join("info.txt");
            fs::write(&report_path, &report_for_profile).with_context(|| {
                format!("writing daemon profile report {}", report_path.display())
            })?;
        }
    }

    Ok(())
}

fn append_resource_usage(out: &mut String, resource_usage: Option<&ResourceUsageSummary>) {
    if let Some(usage) = resource_usage {
        out.push_str(&usage.report_block());
        out.push('\n');
        return;
    }
    out.push_str("Resource usage: unavailable\n");
}

fn profile_daemon_info(
    args: &BatchDaemonArgs,
    profile: &DaemonProfile,
    state: &DaemonProgressState,
    elapsed: Duration,
    resource_usage: Option<&ResourceUsageSummary>,
    profile_index: usize,
) -> Result<String> {
    use std::fmt::Write;

    let mut out = String::new();
    let started = chrono::Local::now();
    let started_str = started.format("%Y-%m-%d %H:%M:%S").to_string();
    let time_of_day = started.format("%H:%M:%S").to_string();
    let runtime = format_duration(elapsed);
    let uptime = format_duration(state.started_at.elapsed());
    let stats = state
        .profile_stats
        .get(profile_index)
        .cloned()
        .unwrap_or_default();

    writeln!(out, "mini-film daemon profile report").ok();
    writeln!(out, "Generated: {started_str}").ok();
    writeln!(out, "Time of day: {time_of_day}").ok();
    writeln!(out, "Timezone: {}", started.format("%:z")).ok();
    writeln!(out, "Mini-film version: {}", env!("CARGO_PKG_VERSION")).ok();
    writeln!(out, "Input directory: {}", args.input.display()).ok();
    writeln!(out, "Output directory: {}", args.output.display()).ok();
    writeln!(out, "Profile: {}", profile.selector).ok();
    writeln!(out, "Resolved profile: {}", profile.stem).ok();
    writeln!(out, "Output format: {:?}", args.output_format).ok();
    writeln!(out, "Elapsed: {runtime} (up since report start: {uptime})").ok();
    writeln!(
        out,
        "Profile files: processed={}, succeeded={}, failed={}",
        stats.processed, stats.succeeded, stats.failed
    )
    .ok();
    writeln!(out, "Profile avg: {} ms/file", stats.avg_ms()).ok();

    append_resource_usage(&mut out, resource_usage);

    writeln!(out, "\nProfile info details:").ok();
    for line in profile.profile_report.lines() {
        writeln!(out, "{line}").ok();
    }
    writeln!(out).ok();

    writeln!(out, "Files for this profile:").ok();
    for file in state
        .files
        .iter()
        .filter(|file| file.profile_index == profile_index)
    {
        if let Some(error) = &file.error {
            writeln!(
                out,
                "  FAILURE | {} | {} | {} | {} | {} | {}",
                file.raw.display(),
                file.output.display(),
                error,
                file.lens_profile_status,
                file.sharpening_status,
                file.denoise_status
            )
            .ok();
        } else {
            writeln!(
                out,
                "  OK | {} | {} | {} | {} | {} | {}",
                file.raw.display(),
                file.output.display(),
                format_duration(file.duration),
                file.lens_profile_status,
                file.sharpening_status,
                file.denoise_status
            )
            .ok();
        }
    }

    Ok(out)
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
                lens_corrections: args.lens_corrections,
                color_noise_iso_threshold: args.color_noise_iso_threshold,
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
            let profile_report = profile_info_text_for_selector(
                selector,
                &args.profiles_root,
                &args.hald_dir,
                args.hald_level,
            )?;
            let stem = resolved.resolved_stem.clone();
            Ok(DaemonProfile {
                selector: selector.clone(),
                stem,
                resolved,
                profile_report,
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
    profile_stem: &str,
    context: &DaemonTaskContext,
    file: &ProgressBar,
    raw_name: &str,
) -> DaemonFileResult {
    let args = &context.args;
    let output = match daemon_output_path(
        &args.input,
        &args.output,
        args.output_format,
        raw,
        &profile.stem,
    ) {
        Ok(output) => output,
        Err(error) => {
            return DaemonFileResult {
                raw: raw.to_path_buf(),
                output: PathBuf::new(),
                duration: Duration::ZERO,
                profile_index: profile_index as usize,
                error: Some(error.to_string()),
                lens_profile_status: lens_profile_status(args.lens_corrections, false),
                sharpening_status: sharpening_status(false),
                denoise_status: denoise_status(false),
            };
        }
    };

    if let Some(parent) = output.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return DaemonFileResult {
            raw: raw.to_path_buf(),
            output,
            duration: Duration::ZERO,
            profile_index: profile_index as usize,
            error: Some(error.to_string()),
            lens_profile_status: lens_profile_status(args.lens_corrections, false),
            sharpening_status: sharpening_status(false),
            denoise_status: denoise_status(false),
        };
    }

    let temp_dir = match Builder::new().prefix("mini-film-daemon-job-").tempdir() {
        Ok(temp_dir) => temp_dir,
        Err(error) => {
            return DaemonFileResult {
                raw: raw.to_path_buf(),
                output,
                duration: Duration::ZERO,
                profile_index: profile_index as usize,
                error: Some(error.to_string()),
                lens_profile_status: lens_profile_status(args.lens_corrections, false),
                sharpening_status: sharpening_status(false),
                denoise_status: denoise_status(false),
            };
        }
    };

    file.set_position(0);
    file.set_message(format!("{} -> {}: queued", raw_name, profile.stem));

    let file_start = Instant::now();
    let progress = ApplyProgress {
        file,
        started: file_start,
        estimates: Some(Arc::clone(&context.estimates)),
    };
    let seed = stable_profile_seed(context.base_seed, raw, profile_index);
    if let Err(error) = apply_resolved(
        ApplyJob {
            raw,
            output: &output,
            rawtherapee: &args.rawtherapee,
            convert: &args.convert,
            keep_intermediate: None,
            no_grain: args.no_grain,
            lens_corrections: args.lens_corrections,
            color_noise_iso_threshold: args.color_noise_iso_threshold,
            export: &args.export,
            quiet: true,
            exif_comment: Some(format!(
                "mini-film {} usage=daemon profile={}",
                env!("CARGO_PKG_VERSION"),
                profile_stem
            )),
        },
        &profile.resolved,
        seed,
        temp_dir.path(),
        Some(&progress),
    ) {
        return DaemonFileResult {
            raw: raw.to_path_buf(),
            output,
            duration: file_start.elapsed(),
            profile_index: profile_index as usize,
            error: Some(error.to_string()),
            lens_profile_status: lens_profile_status(args.lens_corrections, false),
            sharpening_status: sharpening_status(false),
            denoise_status: denoise_status(false),
        };
    }

    let (sharpening_applied, denoise_applied) =
        resolve_apply_effects(raw, &profile.resolved, args.color_noise_iso_threshold);

    file.set_message(format!(
        "{} -> {}: done in {}",
        raw_name,
        profile.stem,
        format_duration(file_start.elapsed())
    ));

    DaemonFileResult {
        raw: raw.to_path_buf(),
        output,
        duration: file_start.elapsed(),
        profile_index: profile_index as usize,
        error: None,
        lens_profile_status: lens_profile_status(args.lens_corrections, true),
        sharpening_status: sharpening_status(sharpening_applied),
        denoise_status: denoise_status(denoise_applied),
    }
}

fn sharpening_status(applied: bool) -> String {
    format!("sharpening: {}", if applied { "yes" } else { "no" })
}

fn denoise_status(applied: bool) -> String {
    format!("denoise: {}", if applied { "yes" } else { "no" })
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
    let profile_stem = sanitize_filename::sanitize(profile_stem);
    let profile_dir = daemon_output_dir(input_root, output_root, raw, &profile_stem)?;
    let raw_stem = relative_raw_stem(raw)?;
    Ok(profile_dir.join(format!(
        "{}.{}",
        sanitize_filename::sanitize(raw_stem),
        output_format.extension()
    )))
}

fn daemon_output_dir(
    input_root: &Path,
    output_root: &Path,
    raw: &Path,
    profile_stem: &str,
) -> Result<PathBuf> {
    let relative = raw
        .strip_prefix(input_root)
        .with_context(|| format!("mapping {} under {}", raw.display(), input_root.display()))?;
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    Ok(output_root.join(parent).join(profile_stem))
}

fn relative_raw_stem(raw: &Path) -> Result<&str> {
    raw.file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| anyhow!("raw path has no file stem: {}", raw.display()))
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
    use std::collections::{HashMap, HashSet};

    #[test]
    fn write_daemon_info_txt_emits_tree_profile_level_info_file() {
        let root = tempfile::tempdir().unwrap();
        let input_root = root.path().join("in");
        let output_root = root.path().join("out");
        let raw = input_root.join("day1").join("DSC_0001.NEF");
        fs::create_dir_all(raw.parent().unwrap()).unwrap();
        fs::write(&raw, b"raw").unwrap();

        let args = BatchDaemonArgs {
            input: input_root.clone(),
            output: output_root.clone(),
            profile: vec!["Portra 400 grainy".into()],
            hald_dir: root.path().to_path_buf(),
            profiles_root: root.path().to_path_buf(),
            hald_level: 16,
            rawtherapee: PathBuf::from("rawtherapee"),
            convert: PathBuf::from("convert"),
            no_grain: false,
            grain: None,
            grain_preset: None,
            grain_seed: None,
            color_noise_iso_threshold: 1600,
            lens_corrections: LensCorrections::default(),
            jobs: Some(2),
            debounce_seconds: 0,
            nikon_wtu: None,
            nikon_wtu_port: 15740,
            nikon_wtu_name: None,
            nikon_wtu_guid: None,
            output_format: BatchOutputFormat::Jpg,
            export: ExportOptions {
                jpg_quality: 80,
                resize: None,
                long_edge: None,
                max_width: None,
                max_height: None,
                jpeg_subsampling: crate::cli::JpegSubsampling::S420,
                strip_metadata: true,
                progressive_jpeg: false,
            },
        };

        let mut output_dirs = HashSet::new();
        let output = daemon_output_path(
            &input_root,
            &output_root,
            BatchOutputFormat::Jpg,
            &raw,
            "Portra 400 grainy",
        )
        .unwrap();
        output_dirs.insert(output.parent().unwrap().to_path_buf());

        let profile = Arc::new(DaemonProfile {
            selector: "Portra 400 grainy".to_string(),
            stem: "Portra 400 grainy".to_string(),
            resolved: ResolvedProfile {
                hald_path: None,
                rawtherapee_profiles: Vec::new(),
                grain: mini_film::GrainSettings::default(),
                sharpening_applied: false,
                resolved_stem: "Portra 400 grainy".to_string(),
            },
            profile_report: "profile report".to_string(),
        });

        let state = DaemonProgressState {
            total_processed: 1,
            total_succeeded: 1,
            total_failed: 0,
            total_elapsed_ms: 100,
            started_at: Instant::now(),
            files: vec![DaemonFileResult {
                raw: raw.clone(),
                output,
                duration: Duration::from_millis(100),
                profile_index: 0,
                error: None,
                lens_profile_status: lens_profile_status(LensCorrections::default(), false),
                sharpening_status: sharpening_status(false),
                denoise_status: denoise_status(false),
            }],
            profile_stats: vec![DaemonProfileStats {
                processed: 1,
                succeeded: 1,
                failed: 0,
                elapsed_ms: 100,
            }],
            profile_output_dirs: vec![output_dirs],
            resource_usage: ResourceUsageSummary::default(),
            last_resource_sample: Instant::now(),
        };

        write_daemon_info_txt(
            &output_root,
            &args,
            std::slice::from_ref(&profile),
            &state,
            Duration::ZERO,
            None,
        )
        .unwrap();

        let tree_profile_info = output_root
            .join("day1")
            .join("Portra 400 grainy")
            .join("info.txt");
        assert!(tree_profile_info.exists());
        assert!(output_root.join("info.txt").exists());
        let txt = fs::read_to_string(tree_profile_info).unwrap();
        assert!(txt.contains("Portra 400 grainy"));
        assert!(txt.contains("Resource usage: unavailable"));
        assert!(txt.contains("Mini-film version:"));
        assert!(txt.contains("Time of day:"));
    }

    #[test]
    fn write_daemon_info_txt_includes_resource_usage_and_timing() {
        let root = tempfile::tempdir().unwrap();
        let args = BatchDaemonArgs {
            input: PathBuf::from("/input"),
            output: root.path().join("out"),
            profile: vec!["Portra 400 grainy".into()],
            hald_dir: root.path().to_path_buf(),
            profiles_root: root.path().to_path_buf(),
            hald_level: 16,
            rawtherapee: PathBuf::from("rawtherapee"),
            convert: PathBuf::from("convert"),
            no_grain: false,
            grain: None,
            grain_preset: None,
            grain_seed: None,
            color_noise_iso_threshold: 1600,
            lens_corrections: LensCorrections::default(),
            jobs: Some(2),
            debounce_seconds: 0,
            nikon_wtu: None,
            nikon_wtu_port: 15740,
            nikon_wtu_name: None,
            nikon_wtu_guid: None,
            output_format: BatchOutputFormat::Jpg,
            export: ExportOptions {
                jpg_quality: 80,
                resize: None,
                long_edge: None,
                max_width: None,
                max_height: None,
                jpeg_subsampling: crate::cli::JpegSubsampling::S420,
                strip_metadata: true,
                progressive_jpeg: false,
            },
        };
        let profile = Arc::new(DaemonProfile {
            selector: "Portra 400 grainy".to_string(),
            stem: "Portra 400 grainy".to_string(),
            resolved: ResolvedProfile {
                hald_path: None,
                rawtherapee_profiles: Vec::new(),
                grain: mini_film::GrainSettings::default(),
                sharpening_applied: false,
                resolved_stem: "Portra 400 grainy".to_string(),
            },
            profile_report: "profile report".to_string(),
        });
        let state = DaemonProgressState {
            total_processed: 1,
            total_succeeded: 1,
            total_failed: 0,
            total_elapsed_ms: 1234,
            started_at: Instant::now(),
            files: vec![DaemonFileResult {
                raw: PathBuf::from("/input/DSC_0001.NEF"),
                output: PathBuf::from("/out/day/Portra 400 grainy/DSC_0001.jpg"),
                duration: Duration::from_millis(120),
                profile_index: 0,
                error: None,
                lens_profile_status: lens_profile_status(LensCorrections::default(), true),
                sharpening_status: sharpening_status(false),
                denoise_status: denoise_status(false),
            }],
            profile_stats: vec![DaemonProfileStats {
                processed: 1,
                succeeded: 1,
                failed: 0,
                elapsed_ms: 1234,
            }],
            profile_output_dirs: vec![{
                let mut dirs = HashSet::new();
                dirs.insert(
                    root.path()
                        .join("out")
                        .join("day")
                        .join("Portra 400 grainy"),
                );
                dirs
            }],
            resource_usage: ResourceUsageSummary::from(&crate::app::system_stats::ResourceUsage {
                process_cpu_percent: 12.3,
                process_memory_bytes: 2048,
                system_cpu_percent: 10.0,
                system_memory_used_bytes: 10_000,
                system_memory_total_bytes: 16_000,
            }),
            last_resource_sample: Instant::now(),
        };

        write_daemon_info_txt(
            &args.output,
            &args,
            std::slice::from_ref(&profile),
            &state,
            Duration::from_secs(1),
            Some(&state.resource_usage),
        )
        .unwrap();

        let root_info = args.output.join("info.txt");
        let tree_profile_info = args
            .output
            .join("day")
            .join("Portra 400 grainy")
            .join("info.txt");
        let root_txt = fs::read_to_string(root_info).unwrap();
        let profile_txt = fs::read_to_string(tree_profile_info).unwrap();
        assert!(root_txt.contains("CPU usage:"));
        assert!(root_txt.contains("Mini-film version:"));
        assert!(root_txt.contains("Time of day:"));
        assert!(profile_txt.contains("CPU usage:"));
        assert!(profile_txt.contains("Mini-film version:"));
        assert!(profile_txt.contains("Time of day:"));
        assert!(profile_txt.contains("Profile files: processed=1, succeeded=1, failed=0"));
        assert!(profile_txt.contains("Files for this profile:"));
        assert!(profile_txt.contains("DSC_0001.NEF"));
    }

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
