use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Instant,
};

use anyhow::{Context, Result, bail};
use indicatif::{MultiProgress, ProgressBar};
use rayon::prelude::*;
use tempfile::Builder;
use walkdir::WalkDir;

use crate::app::apply::{ApplyArgs, ApplyJob, apply_resolved, resolve_grain_override};
use crate::app::export::validate_export_options;
use crate::app::profile::resolve_profile;
use crate::app::progress::{
    ApplyProgress, StageEstimates, batch_progress_style, file_progress_style, format_duration,
    progress_length,
};
use crate::app::util::{half_cpu_thread_count, time_of_day_seed};
use crate::cli::{BatchOutputFormat, ExportOptions};

pub(crate) struct BatchArgs {
    pub(crate) input: PathBuf,
    pub(crate) output: PathBuf,
    pub(crate) profile: String,
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
    pub(crate) output_format: BatchOutputFormat,
    pub(crate) export: ExportOptions,
}

/// Run the batch command over every supported RAW file under an input tree.
///
/// The batch pipeline validates shared export options once, creates the output
/// directory, resolves the profile once into a reusable Hald, and then processes
/// a bounded number of files at a time with per-file temp directories. It
/// preserves relative input paths under the output root, drives a batch progress
/// bar plus one per-worker step bar, derives a stable grain seed per file,
/// records failures, and reports all failures after the parallel work finishes
/// instead of stopping at the first bad image.
pub(crate) fn run_batch(args: BatchArgs) -> Result<()> {
    validate_export_options(&args.export)?;
    let jobs = resolve_batch_jobs(args.jobs)?;
    if !args.input.is_dir() {
        bail!("batch input is not a directory: {}", args.input.display());
    }
    fs::create_dir_all(&args.output)
        .with_context(|| format!("creating {}", args.output.display()))?;

    let raws = collect_batch_inputs(&args.input)?;
    if raws.is_empty() {
        bail!("no DNG/NEF files found under {}", args.input.display());
    }

    let temp_dir = Builder::new().prefix("mini-film-batch-").tempdir()?;
    let apply_args = ApplyArgs {
        raw: PathBuf::new(),
        output: PathBuf::new(),
        profile: args.profile.clone(),
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
    let mut resolved = resolve_profile(&apply_args, temp_dir.path())?;
    if let Some(grain) =
        resolve_grain_override(args.grain.as_deref(), args.grain_preset.as_deref())?
    {
        resolved.grain = grain;
    }
    let base_seed = args.grain_seed.unwrap_or_else(time_of_day_seed);

    let multi = MultiProgress::new();
    let batch = multi.add(ProgressBar::new(raws.len() as u64));
    batch.set_style(batch_progress_style());
    batch.set_message("starting");
    let worker_bars: Vec<_> = (0..jobs)
        .map(|index| {
            let file = multi.add(ProgressBar::new(progress_length()));
            file.set_style(file_progress_style());
            file.set_message(format!("worker {} waiting", index + 1));
            file
        })
        .collect();

    let batch_start = Instant::now();
    let pool = rayon::ThreadPoolBuilder::new().num_threads(jobs).build()?;
    let bar_pool = Arc::new(Mutex::new(worker_bars.clone()));
    let estimates = Arc::new(StageEstimates::default());
    let results: Vec<_> = pool.install(|| {
        raws.par_iter()
            .enumerate()
            .map(|(index, raw)| {
                let file = acquire_worker_bar(&bar_pool);
                let result = process_batch_file(
                    &args,
                    &resolved,
                    base_seed,
                    temp_dir.path(),
                    &batch,
                    &file,
                    Arc::clone(&estimates),
                    index,
                    raw,
                );
                release_worker_bar(&bar_pool, file);
                result
            })
            .collect()
    });
    for file in &worker_bars {
        file.finish_and_clear();
    }

    let failures: Vec<_> = results.into_iter().filter_map(Result::err).collect();
    if failures.is_empty() {
        batch.finish_with_message(format!(
            "done {} files in {}",
            raws.len(),
            format_duration(batch_start.elapsed())
        ));
        Ok(())
    } else {
        batch.abandon_with_message(format!(
            "failed {}/{} files in {}",
            failures.len(),
            raws.len(),
            format_duration(batch_start.elapsed())
        ));
        for (path, err) in failures {
            batch.println(format!("failed {}: {err:#}", path.display()));
        }
        bail!("batch finished with failures")
    }
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

fn process_batch_file(
    args: &BatchArgs,
    resolved: &crate::app::profile::ResolvedProfile,
    base_seed: u64,
    temp_root: &Path,
    batch: &ProgressBar,
    file: &ProgressBar,
    estimates: Arc<StageEstimates>,
    index: usize,
    raw: &Path,
) -> Result<(), (PathBuf, anyhow::Error)> {
    process_batch_file_inner(
        args, resolved, base_seed, temp_root, batch, file, estimates, index, raw,
    )
    .map_err(|err| (raw.to_path_buf(), err))
}

fn process_batch_file_inner(
    args: &BatchArgs,
    resolved: &crate::app::profile::ResolvedProfile,
    base_seed: u64,
    temp_root: &Path,
    batch: &ProgressBar,
    file: &ProgressBar,
    estimates: Arc<StageEstimates>,
    index: usize,
    raw: &Path,
) -> Result<()> {
    let output = batch_output_path(&args.input, &args.output, args.output_format, raw)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let display_name = raw
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<unknown>")
        .to_string();
    batch.set_message(display_name.clone());
    file.set_position(0);
    file.set_message(format!("{display_name}: queued"));

    let file_start = Instant::now();
    let progress = ApplyProgress {
        file,
        started: file_start,
        estimates: Some(estimates),
    };
    let file_temp = temp_root.join(format!("file-{index}"));
    fs::create_dir_all(&file_temp).with_context(|| format!("creating {}", file_temp.display()))?;
    let seed = per_file_seed(base_seed, index as u64, raw);
    let result = apply_resolved(
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
        resolved,
        seed,
        &file_temp,
        Some(&progress),
    );

    batch.inc(1);
    match result {
        Ok(()) => {
            file.set_message(format!(
                "{}: done in {}",
                display_name,
                format_duration(file_start.elapsed())
            ));
            Ok(())
        }
        Err(err) => {
            file.set_message(format!(
                "{}: failed after {}",
                display_name,
                format_duration(file_start.elapsed())
            ));
            Err(err)
        }
    }
}

fn collect_batch_inputs(input: &Path) -> Result<Vec<PathBuf>> {
    let mut raws = Vec::new();
    for entry in WalkDir::new(input).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        if is_batch_raw(entry.path()) {
            raws.push(entry.path().to_path_buf());
        }
    }
    raws.sort();
    Ok(raws)
}

fn is_batch_raw(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some(ext)
            if ext.eq_ignore_ascii_case("dng") || ext.eq_ignore_ascii_case("nef")
    )
}

fn resolve_batch_jobs(jobs: Option<usize>) -> Result<usize> {
    let jobs = jobs.unwrap_or_else(half_cpu_thread_count);
    if jobs == 0 {
        bail!("--jobs must be at least 1");
    }
    Ok(jobs)
}

fn batch_output_path(
    input_root: &Path,
    output_root: &Path,
    output_format: BatchOutputFormat,
    raw: &Path,
) -> Result<PathBuf> {
    let rel = raw
        .strip_prefix(input_root)
        .with_context(|| format!("mapping {} under {}", raw.display(), input_root.display()))?;
    let parent = rel.parent().unwrap_or_else(|| Path::new(""));
    let stem = rel
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("input has no valid stem: {}", raw.display()))?;
    Ok(output_root
        .join(parent)
        .join(format!("{}.{}", stem, output_format.extension())))
}

fn per_file_seed(base_seed: u64, index: u64, path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    base_seed ^ index.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_output_path_preserves_relative_folders_and_extension() {
        let input = Path::new("/input");
        let output = Path::new("/output");
        let raw = Path::new("/input/day1/DSC_0001.NEF");

        assert_eq!(
            batch_output_path(input, output, BatchOutputFormat::Jpg, raw).unwrap(),
            Path::new("/output/day1/DSC_0001.jpg")
        );
        assert_eq!(
            batch_output_path(input, output, BatchOutputFormat::Tiff, raw).unwrap(),
            Path::new("/output/day1/DSC_0001.tif")
        );
    }

    #[test]
    fn batch_raw_detection_is_case_insensitive_for_supported_raws() {
        assert!(is_batch_raw(Path::new("a.dng")));
        assert!(is_batch_raw(Path::new("a.DNG")));
        assert!(is_batch_raw(Path::new("a.nef")));
        assert!(is_batch_raw(Path::new("a.NEF")));
        assert!(!is_batch_raw(Path::new("a.jpg")));
    }

    #[test]
    fn resolve_batch_jobs_defaults_and_rejects_zero() {
        assert!(resolve_batch_jobs(None).unwrap() >= 1);
        assert_eq!(resolve_batch_jobs(Some(3)).unwrap(), 3);
        assert!(resolve_batch_jobs(Some(0)).is_err());
    }

    #[test]
    fn per_file_seed_changes_with_index_path_or_base_seed() {
        let a = per_file_seed(1, 0, Path::new("a.dng"));
        assert_eq!(a, per_file_seed(1, 0, Path::new("a.dng")));
        assert_ne!(a, per_file_seed(2, 0, Path::new("a.dng")));
        assert_ne!(a, per_file_seed(1, 1, Path::new("a.dng")));
        assert_ne!(a, per_file_seed(1, 0, Path::new("b.dng")));
    }
}
