use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use indicatif::{MultiProgress, ProgressBar};
use rayon::prelude::*;
use tempfile::Builder;
use walkdir::WalkDir;

use crate::app::apply::{ApplyArgs, ApplyJob, apply_resolved, resolve_grain_override};
use crate::app::export::{finalize_output, validate_export_options};
use crate::app::info::profile_info_text_for_selector;
use crate::app::profile::resolve_profile;
use crate::app::progress::{
    ApplyProgress, StageEstimates, batch_progress_style, file_progress_style, format_duration,
    progress_length,
};
use crate::app::system_stats::{ResourceUsageSummary, sample_usage_block};
use crate::app::util::{half_cpu_thread_count, is_supported_raw_file, time_of_day_seed};
use crate::cli::{BatchOutputFormat, ExportOptions, GalleryTemplate};

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
    pub(crate) color_noise_iso_threshold: u32,
    pub(crate) jobs: Option<usize>,
    pub(crate) output_format: BatchOutputFormat,
    pub(crate) gallery: Option<GalleryTemplate>,
    pub(crate) gallery_thumbnail_long_edge: u32,
    pub(crate) gallery_columns: u32,
    pub(crate) export: ExportOptions,
}

struct BatchFileRecord {
    raw: PathBuf,
    output: PathBuf,
    duration: Duration,
    error: Option<String>,
}

#[derive(Clone)]
struct GalleryEntry {
    raw: PathBuf,
    output: PathBuf,
    thumb: PathBuf,
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
        bail!(
            "no supported RAW files found under {}",
            args.input.display()
        );
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
        color_noise_iso_threshold: args.color_noise_iso_threshold,
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
    let profile_report = profile_info_text_for_selector(
        &args.profile,
        &args.profiles_root,
        &args.hald_dir,
        args.hald_level,
    )?;
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
    let resource_usage = Arc::new(Mutex::new(ResourceUsageSummary::default()));
    let results: Vec<_> = pool.install(|| {
        raws.par_iter()
            .enumerate()
            .map(|(index, raw)| {
                let file = acquire_worker_bar(&bar_pool);
                let context = ProcessBatchFileContext {
                    args: &args,
                    resolved: &resolved,
                    base_seed,
                    temp_root: temp_dir.path(),
                    batch: &batch,
                    file: &file,
                    estimates: Arc::clone(&estimates),
                    resource_usage: Arc::clone(&resource_usage),
                    index,
                };
                let result = process_batch_file(&context, raw);
                release_worker_bar(&bar_pool, file);
                result
            })
            .collect()
    });
    for file in &worker_bars {
        file.finish_and_clear();
    }

    let (successes, failures): (Vec<_>, Vec<_>) = results
        .into_iter()
        .partition(|result| result.error.is_none());
    let end = batch_start.elapsed();
    let resource_usage = *resource_usage.lock().expect("resource usage lock poisoned");
    if let Some(gallery_style) = args.gallery {
        let gallery_output = args.output.join("index.html");
        run_batch_gallery(
            &gallery_output,
            &args.output,
            &args.profile,
            gallery_style,
            &successes,
            &args,
        )?;
    }
    write_batch_report(&BatchReportContext {
        output_dir: &args.output,
        args: &args,
        resolved_profile_stem: &resolved.resolved_stem,
        profile_report: &profile_report,
        raw_count: raws.len(),
        elapsed: end,
        resource_usage: Some(&resource_usage),
        successes: &successes,
        failures: &failures,
    });

    if failures.is_empty() {
        batch.finish_with_message(format!(
            "done {} files in {}",
            raws.len(),
            format_duration(end)
        ));
        Ok(())
    } else {
        batch.abandon_with_message(format!(
            "failed {}/{} files in {}",
            failures.len(),
            raws.len(),
            format_duration(end)
        ));
        for result in failures {
            batch.println(format!(
                "failed {}: {:#}",
                result.raw.display(),
                result.error.as_deref().unwrap_or("")
            ));
        }
        bail!("batch finished with failures")
    }
}

fn run_batch_gallery(
    gallery_output: &Path,
    output_root: &Path,
    profile: &str,
    gallery_template: GalleryTemplate,
    successes: &[BatchFileRecord],
    args: &BatchArgs,
) -> Result<()> {
    let gallery_root = gallery_output
        .parent()
        .map_or(Path::new("."), |parent| parent);
    fs::create_dir_all(gallery_root)
        .with_context(|| format!("creating {}", gallery_root.display()))?;

    let thumbs_root = gallery_root.join("thumbnails");
    fs::create_dir_all(&thumbs_root)
        .with_context(|| format!("creating {}", thumbs_root.display()))?;

    let entries: Vec<GalleryEntry> = successes
        .iter()
        .filter_map(|result| {
            let thumb = gallery_thumbnail_output(&thumbs_root, output_root, &result.output)?;
            Some(GalleryEntry {
                raw: result.raw.clone(),
                output: result.output.clone(),
                thumb,
            })
        })
        .collect();
    if entries.is_empty() {
        return Ok(());
    }

    let jobs = resolve_batch_jobs(args.jobs)?;
    let thread_pool = rayon::ThreadPoolBuilder::new().num_threads(jobs).build()?;
    let thumb_export = ExportOptions {
        jpg_quality: args.export.jpg_quality,
        resize: None,
        long_edge: Some(args.gallery_thumbnail_long_edge),
        max_width: None,
        max_height: None,
        jpeg_subsampling: args.export.jpeg_subsampling,
        strip_metadata: args.export.strip_metadata,
        progressive_jpeg: args.export.progressive_jpeg,
    };
    let convert = args.convert.clone();
    thread_pool.install(|| {
        entries.par_iter().for_each(|entry| {
            let _ = create_batch_gallery_thumbnail(
                &convert,
                &entry.output,
                &entry.thumb,
                &thumb_export,
            );
        });
    });

    let html = render_batch_gallery_html(
        args.gallery_thumbnail_long_edge,
        args.gallery_columns,
        gallery_template,
        profile,
        &entries,
        gallery_root,
    )?;
    fs::write(gallery_output, html)
        .with_context(|| format!("writing {}", gallery_output.display()))?;
    Ok(())
}

fn gallery_thumbnail_output(
    thumbs_root: &Path,
    output_root: &Path,
    output: &Path,
) -> Option<PathBuf> {
    let rel = output.strip_prefix(output_root).ok()?;
    let stem = rel.with_extension("jpg");
    Some(thumbs_root.join(stem))
}

fn create_batch_gallery_thumbnail(
    convert: &Path,
    source: &Path,
    output: &Path,
    export: &ExportOptions,
) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    finalize_output(convert, source, output, export)
        .with_context(|| format!("creating thumbnail {}", output.display()))
}

fn batch_relative_path(base: &Path, target: &Path) -> String {
    target
        .strip_prefix(base)
        .unwrap_or(target)
        .to_string_lossy()
        .replace('\\', "/")
}

fn render_batch_gallery_html(
    thumbnail_long_edge: u32,
    columns: u32,
    template: GalleryTemplate,
    profile: &str,
    entries: &[GalleryEntry],
    gallery_root: &Path,
) -> Result<String> {
    let mut gallery_items = String::new();
    for entry in entries {
        let full = batch_relative_path(gallery_root, &entry.output);
        let thumb = batch_relative_path(gallery_root, &entry.thumb);
        let raw_name = entry
            .raw
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("image");
        let caption = format!("{raw_name} — {profile}");
        gallery_items.push_str(&format!(
            r#"<button class="mf-thumb" type="button" data-full="{full}" data-caption="{caption}">
  <img src="{thumb}" alt="{caption}" loading="lazy" decoding="async" />
  <span class="mf-thumb-caption">{raw}</span>
  <span class="mf-thumb-meta">Max edge: {max_edge}px</span>
</button>
"#,
            full = html_escape(&full),
            caption = html_escape(&caption),
            thumb = html_escape(&thumb),
            raw = html_escape(raw_name),
            max_edge = thumbnail_long_edge
        ));
    }

    let style = gallery_style(template);
    let output_name = gallery_root
        .parent()
        .map_or_else(|| ".".to_string(), |parent| parent.display().to_string());
    let output_name = output_name.to_string();
    Ok(format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>mini-film batch gallery</title>
  <style>{style}</style>
</head>
<body>
  <main>
    <h1>mini-film batch gallery</h1>
    <p class="mf-subtitle">Profile: {profile}</p>
    <p class="mf-subtitle">Output: {output}</p>
    <section class="mf-grid" style="--columns: {columns}">
{items}
    </section>
  </main>
  <div class="mf-overlay" id="mf-overlay">
    <button class="mf-close" id="mf-overlay-close" type="button">&times;</button>
    <a id="mf-overlay-download" href="" download>Download</a>
    <img id="mf-overlay-image" src="" alt="" />
    <p id="mf-overlay-caption"></p>
  </div>
  <footer class="mf-footer">
    Generated by <a href="https://github.com/alfanick/mini-film">mini-film</a> {version}
  </footer>
  <script>{script}</script>
</body>
</html>
"#,
        style = style,
        profile = html_escape(profile),
        output = html_escape(&output_name),
        columns = columns.max(1),
        items = gallery_items,
        version = env!("CARGO_PKG_VERSION"),
        script = gallery_script()
    ))
}

fn gallery_style(template: GalleryTemplate) -> String {
    let theme = match template {
        GalleryTemplate::Modern => {
            r#"
            :root {
              --bg: #0f1114;
              --panel: #15181d;
              --border: #404550;
              --text: #eff2f6;
              --muted: #a8b0bb;
            }
            "#
        }
        GalleryTemplate::Soft => {
            r#"
            :root {
              --bg: #f6f4ef;
              --panel: #faf8f2;
              --border: #d4ccc2;
              --text: #272320;
              --muted: #655e58;
            }
            "#
        }
        GalleryTemplate::Compact => {
            r#"
            :root {
              --bg: #10131a;
              --panel: #191d27;
              --border: #3b4556;
              --text: #f0f5ff;
              --muted: #7f8a9d;
            }
            "#
        }
        GalleryTemplate::Hero => {
            r#"
            :root {
              --bg: #f7f8fb;
              --panel: #ffffff;
              --border: #d5dbe8;
              --text: #1d2430;
              --muted: #646f83;
            }
            "#
        }
        GalleryTemplate::Phone => {
            r#"
            :root {
              --bg: #f2f2f7;
              --panel: #ffffff;
              --border: rgba(0, 0, 0, 0.08);
              --text: #1d1d1f;
              --muted: #86868b;
            }
            "#
        }
    };

    format!(
        r#"body {{
  margin: 0;
  font-family: ui-monospace, "SFMono-Regular", Menlo, Monaco, Consolas, monospace;
  background: var(--bg);
  color: var(--text);
}}
* {{ box-sizing: border-box; }}
:root {{
  --radius: 14px;
  --bg: var(--bg);
  --panel: var(--panel);
  --border: var(--border);
  --text: var(--text);
  --muted: var(--muted);
  --columns: 4;
}}
{theme}
main {{ max-width: 1200px; margin: 0 auto; padding: 28px; }}
h1 {{ margin: 0; }}
.mf-subtitle {{ color: var(--muted); margin: 8px 0; }}
.mf-grid {{ margin-top: 22px; display: grid; gap: 16px; grid-template-columns: repeat(var(--columns), minmax(0, 1fr)); }}
.mf-thumb {{
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--panel);
  display: block;
  padding: 10px;
  color: inherit;
  text-align: left;
}}
.mf-thumb img {{
  display: block;
  width: 100%;
  aspect-ratio: 3 / 2;
  object-fit: cover;
  border-radius: 8px;
  background: #111;
}}
.mf-thumb-caption {{ display: block; margin-top: 8px; font-weight: 700; }}
.mf-thumb-meta {{ color: var(--muted); font-size: 0.9em; }}
.mf-overlay {{
  position: fixed;
  inset: 0;
  display: none;
  align-items: center;
  justify-content: center;
  flex-direction: column;
  gap: 10px;
  background: rgba(0, 0, 0, 0.9);
  padding: 20px;
}}
.mf-overlay.open {{ display: flex; }}
#mf-overlay-image {{ width: auto; max-width: min(92vw, 1900px); max-height: 78vh; object-fit: contain; }}
#mf-overlay-download {{
  color: #ffffff;
  position: absolute;
  top: 14px;
  right: 72px;
}}
.mf-close {{
  position: absolute;
  top: 8px;
  right: 18px;
  width: 40px;
  height: 40px;
  border: 1px solid rgba(255, 255, 255, 0.4);
  background: transparent;
  color: #fff;
  font-size: 30px;
  line-height: 32px;
}}
.mf-footer {{ max-width: 1200px; margin: 32px auto 20px; color: var(--muted); font-size: 0.9em; }}
.mf-thumb:hover {{ cursor: zoom-in; }}
.mf-footer a {{ color: inherit; }}
{template_extra}"#,
        theme = theme,
        template_extra = match template {
            GalleryTemplate::Hero =>
                "\n.mf-grid .mf-thumb:first-child { grid-column: 1 / -1; }\n.mf-thumb:first-child img {{ aspect-ratio: 16 / 9; }}",
            GalleryTemplate::Compact =>
                "\n.mf-grid {{ gap: 10px; }}\n.mf-thumb {{ padding: 6px; }}\n.mf-thumb-caption {{ font-size: 0.92em; }}",
            GalleryTemplate::Soft => "\n.mf-thumb {{ background: rgba(255,255,255,0.78); }}",
            GalleryTemplate::Modern => "\n.mf-overlay {{ backdrop-filter: blur(1px); }}",
            GalleryTemplate::Phone => {
                "
                .mf-grid { gap: 2px; }
                .mf-thumb {
                  border: none;
                  border-radius: 2px;
                  background: transparent;
                  padding: 0;
                  margin: 0;
                }
                .mf-thumb img { aspect-ratio: 1 / 1; border-radius: 0; }
                .mf-thumb-caption { margin-top: 4px; font-size: 0.78em; line-height: 1.2; }
                .mf-thumb-meta { font-size: 0.64em; opacity: 0.9; }
                .mf-overlay { backdrop-filter: none; }
                .mf-thumb:hover { cursor: zoom-in; transform: none; }
                "
            }
        }
    )
}

fn gallery_script() -> String {
    r#"const overlay = document.getElementById("mf-overlay");
const overlayImage = document.getElementById("mf-overlay-image");
const overlayCaption = document.getElementById("mf-overlay-caption");
const overlayDownload = document.getElementById("mf-overlay-download");
const closeButton = document.getElementById("mf-overlay-close");

function openOverlay(full, caption) {
  overlayImage.src = full;
  overlayImage.alt = caption;
  overlayDownload.href = full;
  overlayCaption.textContent = caption;
  overlay.classList.add("open");
}

function closeOverlay() {
  overlay.classList.remove("open");
  overlayImage.removeAttribute("src");
}

document.querySelectorAll(".mf-thumb").forEach((button) => {
  button.addEventListener("click", (event) => {
    event.preventDefault();
    const full = button.getAttribute("data-full") || "";
    const caption = button.getAttribute("data-caption") || "";
    openOverlay(full, caption);
  });
});

overlay.addEventListener("click", (event) => {
  if (event.target === overlay || event.target === closeButton) {
    closeOverlay();
  }
});

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    closeOverlay();
  }
});
"#
    .to_string()
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
        .replace('\'', "&#39;")
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

struct ProcessBatchFileContext<'a> {
    args: &'a BatchArgs,
    resolved: &'a crate::app::profile::ResolvedProfile,
    base_seed: u64,
    temp_root: &'a Path,
    batch: &'a ProgressBar,
    file: &'a ProgressBar,
    estimates: Arc<StageEstimates>,
    resource_usage: Arc<Mutex<ResourceUsageSummary>>,
    index: usize,
}

fn process_batch_file(context: &ProcessBatchFileContext<'_>, raw: &Path) -> BatchFileRecord {
    match process_batch_file_inner(context, raw) {
        Ok((output, duration)) => BatchFileRecord {
            raw: raw.to_path_buf(),
            output,
            duration,
            error: None,
        },
        Err(error) => BatchFileRecord {
            raw: raw.to_path_buf(),
            output: PathBuf::new(),
            duration: Duration::ZERO,
            error: Some(format!("{error:#}")),
        },
    }
}

fn process_batch_file_inner(
    context: &ProcessBatchFileContext<'_>,
    raw: &Path,
) -> Result<(PathBuf, Duration)> {
    let output = batch_output_path(
        &context.args.input,
        &context.args.output,
        context.args.output_format,
        raw,
    )?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let display_name = raw
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<unknown>")
        .to_string();
    context.batch.set_message(display_name.clone());
    context.file.set_position(0);
    context.file.set_message(format!("{display_name}: queued"));

    let file_start = Instant::now();
    let progress = ApplyProgress {
        file: context.file,
        started: file_start,
        estimates: Some(Arc::clone(&context.estimates)),
    };
    let file_temp = context.temp_root.join(format!("file-{}", context.index));
    fs::create_dir_all(&file_temp).with_context(|| format!("creating {}", file_temp.display()))?;
    let seed = per_file_seed(context.base_seed, context.index as u64, raw);
    let result = apply_resolved(
        ApplyJob {
            raw,
            output: &output,
            rawtherapee: &context.args.rawtherapee,
            convert: &context.args.convert,
            keep_intermediate: None,
            no_grain: context.args.no_grain,
            color_noise_iso_threshold: context.args.color_noise_iso_threshold,
            export: &context.args.export,
            quiet: true,
            exif_comment: Some(format!(
                "mini-film {} usage=batch profile={}",
                env!("CARGO_PKG_VERSION"),
                context.resolved.resolved_stem
            )),
        },
        context.resolved,
        seed,
        &file_temp,
        Some(&progress),
    );
    maybe_sample_resource_usage(&context.resource_usage);

    context.batch.inc(1);
    match result {
        Ok(()) => {
            context.file.set_message(format!(
                "{}: done in {}",
                display_name,
                format_duration(file_start.elapsed())
            ));
            Ok((output, file_start.elapsed()))
        }
        Err(err) => {
            context.file.set_message(format!(
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
        if is_supported_raw_file(entry.path()) {
            raws.push(entry.path().to_path_buf());
        }
    }
    raws.sort();
    Ok(raws)
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

fn write_batch_report(context: &BatchReportContext<'_>) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let started = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    writeln!(out, "mini-film batch report").ok();
    writeln!(out, "Generated: {started}").ok();
    writeln!(out, "Mini-film version: {}", env!("CARGO_PKG_VERSION")).ok();
    writeln!(out, "Input directory: {}", context.args.input.display()).ok();
    writeln!(out, "Output directory: {}", context.args.output.display()).ok();
    writeln!(out, "Profile selector: {}", context.args.profile).ok();
    writeln!(out, "Resolved profile: {}", context.resolved_profile_stem).ok();
    writeln!(out, "Output format: {:?}", context.args.output_format).ok();
    writeln!(out, "Detected files: {}", context.raw_count).ok();
    writeln!(out, "Completed: {}", context.successes.len()).ok();
    writeln!(out, "Failed: {}", context.failures.len()).ok();
    writeln!(out, "Elapsed: {}", format_duration(context.elapsed)).ok();
    writeln!(out, "Profile info command output:").ok();
    out.push_str(context.profile_report);
    writeln!(out).ok();
    if let Some(usage) = context.resource_usage {
        out.push_str(&usage.report_block());
        writeln!(out).ok();
    } else {
        writeln!(out, "Resource usage: unavailable").ok();
        writeln!(out).ok();
    }

    writeln!(out, "File statistics:").ok();
    for file in context.successes.iter().chain(context.failures.iter()) {
        if let Some(error) = &file.error {
            writeln!(
                out,
                "FAILURE | {} -> {} | {} | {}",
                file.raw.display(),
                file.output.display(),
                format_duration(file.duration),
                error
            )
            .ok();
        } else {
            writeln!(
                out,
                "OK     | {} -> {} | {}",
                file.raw.display(),
                file.output.display(),
                format_duration(file.duration)
            )
            .ok();
        }
    }

    let report_path = context.output_dir.join("info.txt");
    if let Err(error) = std::fs::write(&report_path, out.clone()) {
        eprintln!("failed writing {}: {error:#}", report_path.display());
    }
    out
}

struct BatchReportContext<'a> {
    output_dir: &'a Path,
    args: &'a BatchArgs,
    resolved_profile_stem: &'a str,
    profile_report: &'a str,
    raw_count: usize,
    elapsed: Duration,
    resource_usage: Option<&'a ResourceUsageSummary>,
    successes: &'a [BatchFileRecord],
    failures: &'a [BatchFileRecord],
}

fn maybe_sample_resource_usage(resource_usage: &Arc<Mutex<ResourceUsageSummary>>) {
    if let Some(usage) = sample_usage_block()
        && let Ok(mut summary) = resource_usage.lock()
    {
        summary.add(&usage);
    }
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
        assert!(is_supported_raw_file(Path::new("a.dng")));
        assert!(is_supported_raw_file(Path::new("a.DNG")));
        assert!(is_supported_raw_file(Path::new("a.nef")));
        assert!(is_supported_raw_file(Path::new("a.NEF")));
        assert!(is_supported_raw_file(Path::new("a.cr2")));
        assert!(is_supported_raw_file(Path::new("a.Cr3")));
        assert!(is_supported_raw_file(Path::new("a.arw")));
        assert!(is_supported_raw_file(Path::new("a.RAF")));
        assert!(is_supported_raw_file(Path::new("a.orf")));
        assert!(!is_supported_raw_file(Path::new("a.jpg")));
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

    #[test]
    fn collect_batch_inputs_recurse_and_sort_supported_raw_files() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("day");
        let b = root.path().join("day2");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();

        let files = [
            b.join("frame3.ARW"),
            a.join("frame1.NEF"),
            a.join("notes.txt"),
            b.join("frame2.nef"),
        ];
        for path in &files {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, b"raw").unwrap();
        }

        let raw_files = collect_batch_inputs(root.path()).unwrap();
        assert_eq!(raw_files.len(), 3);
        assert_eq!(
            raw_files,
            vec![
                root.path().join("day/frame1.NEF"),
                root.path().join("day2/frame2.nef"),
                root.path().join("day2/frame3.ARW"),
            ]
        );
    }

    #[test]
    fn batch_output_path_errors_if_raw_has_no_stem() {
        let input = Path::new("/input");
        let output = Path::new("/output");
        let raw = Path::new("/input/.");

        let err = batch_output_path(input, output, BatchOutputFormat::Jpg, raw).unwrap_err();
        assert!(err.to_string().contains("input has no valid stem"));
    }
}
