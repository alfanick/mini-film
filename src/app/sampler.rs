use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use mini_film::{apply_grain_8bit, write_rawtherapee_resize_profile};
use rayon::prelude::*;
use tempfile::Builder;
use walkdir::WalkDir;

use crate::app::export::{add_convert_thread_limit, finalize_output, validate_output_format};
use crate::app::profile::{profile_from_xmp_quiet, rawtherapee_profiles_with_hald};
use crate::app::progress::{
    ApplyProgress, StageEstimates, format_duration, progress_length, progress_position,
    progress_stage, progress_stage_adaptive, set_progress,
};
use crate::app::raw::run_raw_develop_jpeg;
use crate::app::util::{half_cpu_thread_count, remove_temp_file, time_of_day_seed};
use crate::cli::{ExportOptions, JpegSubsampling};

pub(crate) struct SamplerArgs {
    pub(crate) raw: PathBuf,
    pub(crate) output: PathBuf,
    pub(crate) profiles_root: PathBuf,
    pub(crate) hald_dir: PathBuf,
    pub(crate) hald_level: u32,
    pub(crate) rawtherapee: PathBuf,
    pub(crate) convert: PathBuf,
    pub(crate) no_grain: bool,
    pub(crate) grain_seed: Option<u64>,
    pub(crate) jobs: Option<usize>,
    pub(crate) thumbnail_long_edge: u32,
    pub(crate) jpg_quality: u8,
    pub(crate) jpeg_subsampling: JpegSubsampling,
    pub(crate) strip_metadata: bool,
    pub(crate) progressive_jpeg: bool,
}

struct SampleThumb {
    image: PathBuf,
    name: String,
    filename: String,
    parts: Vec<String>,
    width: u32,
    height: u32,
}

struct SheetEntry<'a> {
    label: String,
    full_name: String,
    sort_key: String,
    thumb: &'a SampleThumb,
}

#[derive(Default)]
struct ProfileTrie {
    thumbs: Vec<SampleThumb>,
    children: BTreeMap<String, ProfileTrie>,
}

struct SheetLayout {
    body: String,
    width: u32,
    height: u32,
}

struct SamplerProgress {
    profile: ProgressBar,
    started: Instant,
    estimates: Arc<StageEstimates>,
}

/// Render a structured contact sheet showing every resolvable XMP profile.
///
/// Each XMP is resolved to a temporary Hald plus generated RawTherapee `.pp3`
/// profiles. The RAW is developed per profile so RawTherapee-side tone/color
/// settings and the Hald Film Simulation are reflected in the thumbnail. After
/// optional grain and final thumbnail sizing, mini-film groups profile names in
/// a trie and asks ImageMagick/GraphicsMagick `convert` to render an SVG sheet
/// where indentation shows each shared-name level.
pub(crate) fn run_sampler(args: SamplerArgs) -> Result<()> {
    validate_sampler_args(&args)?;
    let jobs = resolve_sampler_jobs(args.jobs)?;

    let emulation_root = emulation_root(&args.profiles_root);
    let profiles = collect_xmp_profiles(&emulation_root)?;
    if profiles.is_empty() {
        bail!("no XMP files found under {}", emulation_root.display());
    }

    let temp_dir = Builder::new().prefix("mini-film-sampler-").tempdir()?;
    let base_seed = args.grain_seed.unwrap_or_else(time_of_day_seed);
    let export = ExportOptions {
        jpg_quality: args.jpg_quality,
        resize: None,
        long_edge: Some(args.thumbnail_long_edge),
        max_width: None,
        max_height: None,
        jpeg_subsampling: args.jpeg_subsampling,
        strip_metadata: args.strip_metadata,
        progressive_jpeg: args.progressive_jpeg,
    };

    let multi = MultiProgress::new();
    let sampler = multi.add(ProgressBar::new(profiles.len() as u64));
    sampler.set_style(sampler_progress_style());
    sampler.set_message("starting");

    let started = Instant::now();
    let workers: Vec<_> = (0..jobs)
        .map(|index| {
            let bar = multi.add(ProgressBar::new(progress_length()));
            bar.set_style(profile_progress_style());
            bar.set_message(format!("worker {} waiting", index + 1));
            bar
        })
        .collect();
    let next_worker = AtomicUsize::new(0);
    let estimates = Arc::new(StageEstimates::default());
    let pool = rayon::ThreadPoolBuilder::new().num_threads(jobs).build()?;
    let results: Vec<_> = pool.install(|| {
        profiles
            .par_iter()
            .enumerate()
            .map_init(
                || {
                    let worker = next_worker.fetch_add(1, Ordering::Relaxed) % jobs;
                    workers[worker].clone()
                },
                |profile_progress, (index, profile)| {
                    sampler.set_message(
                        profile
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("<unknown>")
                            .to_string(),
                    );
                    profile_progress.set_length(progress_length());
                    profile_progress.set_position(0);
                    profile_progress.set_message("queued");
                    let profile_started = Instant::now();
                    let progress = SamplerProgress {
                        profile: profile_progress.clone(),
                        started: profile_started,
                        estimates: Arc::clone(&estimates),
                    };
                    let result = render_profile_thumbnail(
                        &args,
                        temp_dir.path(),
                        profile,
                        &emulation_root,
                        index,
                        base_seed,
                        &export,
                        &progress,
                    );
                    if result.is_err() {
                        profile_progress.set_message(format!(
                            "failed after {}",
                            format_duration(profile_started.elapsed())
                        ));
                    }
                    sampler.inc(1);
                    (profile.to_path_buf(), result)
                },
            )
            .collect()
    });
    for worker in &workers {
        worker.finish_and_clear();
    }
    let mut thumbs = Vec::new();
    let mut skipped = 0usize;
    for (profile, result) in results {
        match result {
            Ok(thumb) => thumbs.push(thumb),
            Err(err) => {
                skipped += 1;
                sampler.println(format!("skip {}: {err:#}", profile.display()));
            }
        }
    }
    sampler.set_position(profiles.len() as u64);

    if thumbs.is_empty() {
        sampler.abandon_with_message(format!(
            "no profiles rendered in {}",
            format_duration(started.elapsed())
        ));
        bail!(
            "no resolvable profiles found under {}",
            emulation_root.display()
        );
    }

    sampler.set_message("sheet");
    let sheet_progress = multi.add(ProgressBar::new(progress_length()));
    sheet_progress.set_style(profile_progress_style());
    sheet_progress.set_message(format!("sheet {} thumbnails", thumbs.len()));
    let sheet_started = Instant::now();
    let sheet_apply_progress = ApplyProgress {
        file: &sheet_progress,
        started: sheet_started,
        estimates: None,
    };
    let sheet_stage = progress_stage(
        Some(&sheet_apply_progress),
        0,
        5,
        "sheet",
        estimate_sheet_duration(thumbs.len()),
    );
    run_structured_sheet(
        &args.convert,
        &args.output,
        &thumbs,
        args.thumbnail_long_edge,
        args.jpg_quality,
        args.progressive_jpeg,
    )?;
    sheet_stage.finish();
    sheet_progress.finish_and_clear();
    sampler.finish_with_message(format!(
        "wrote {} thumbnails, skipped {} in {}",
        thumbs.len(),
        skipped,
        format_duration(started.elapsed())
    ));
    eprintln!("wrote {}", args.output.display());
    Ok(())
}

fn validate_sampler_args(args: &SamplerArgs) -> Result<()> {
    validate_output_format(&args.output)?;
    let ext = args
        .output
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if ext != "jpg" && ext != "jpeg" {
        bail!("sampler output must be .jpg or .jpeg");
    }
    if !args.profiles_root.is_dir() {
        bail!(
            "profiles root is not a directory: {}",
            args.profiles_root.display()
        );
    }
    if args.thumbnail_long_edge == 0 {
        bail!("--thumbnail-long-edge must be greater than zero");
    }
    Ok(())
}

fn resolve_sampler_jobs(jobs: Option<usize>) -> Result<usize> {
    let jobs = jobs.unwrap_or_else(half_cpu_thread_count);
    if jobs == 0 {
        bail!("--jobs must be at least 1");
    }
    Ok(jobs)
}

fn collect_xmp_profiles(root: &Path) -> Result<Vec<PathBuf>> {
    let mut profiles = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry
            .path()
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("xmp"))
        {
            profiles.push(entry.path().to_path_buf());
        }
    }
    profiles.sort();
    Ok(profiles)
}

fn emulation_root(root: &Path) -> PathBuf {
    let direct = root.join("emulations");
    if direct.is_dir() {
        return direct;
    }
    if root
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("emulations"))
    {
        return root.to_path_buf();
    }
    if let Some(parent) = root
        .canonicalize()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
    {
        let sibling = parent.join("emulations");
        if sibling.is_dir() {
            return sibling;
        }
    }
    root.to_path_buf()
}

fn render_profile_thumbnail(
    args: &SamplerArgs,
    temp_root: &Path,
    profile: &Path,
    emulation_root: &Path,
    index: usize,
    base_seed: u64,
    export: &ExportOptions,
    progress: &SamplerProgress,
) -> Result<SampleThumb> {
    sampler_step(progress, 1, "resolve");
    let profile_temp = temp_root.join(format!("profile-{index}"));
    fs::create_dir_all(&profile_temp)
        .with_context(|| format!("creating {}", profile_temp.display()))?;

    let resolved = profile_from_xmp_quiet(
        profile,
        args.hald_level,
        &args.profiles_root,
        &args.hald_dir,
        &profile_temp,
    )
    .with_context(|| format!("resolving profile {}", profile.display()))?;
    let developed = profile_temp.join("rawtherapee.jpg");
    let mut rawtherapee_profiles = rawtherapee_profiles_with_hald(&resolved, &profile_temp)?;
    rawtherapee_profiles.push(write_rawtherapee_resize_profile(
        &profile_temp.join("resize.pp3"),
        args.thumbnail_long_edge,
    )?);
    let apply_progress = ApplyProgress {
        file: &progress.profile,
        started: progress.started,
        estimates: Some(Arc::clone(&progress.estimates)),
    };
    let raw_stage = progress_stage_adaptive(
        Some(&apply_progress),
        2,
        3,
        "sampler-rawtherapee",
        "rawtherapee",
        estimate_sampler_raw_duration(args.thumbnail_long_edge),
    );
    run_raw_develop_jpeg(
        &args.rawtherapee,
        &rawtherapee_profiles,
        &args.raw,
        &developed,
        args.jpg_quality,
        args.jpeg_subsampling,
        true,
    )?;
    raw_stage.finish();

    let source = if !args.no_grain && resolved.grain.is_enabled() {
        let grain_stage = progress_stage_adaptive(
            Some(&apply_progress),
            3,
            4,
            "sampler-grain",
            "grain",
            estimate_sampler_grain_duration(args.thumbnail_long_edge),
        );
        let grained = profile_temp.join("grained-8.ppm");
        apply_grain_8bit(
            &developed,
            &grained,
            resolved.grain,
            sample_seed(base_seed, index, profile),
        )?;
        grain_stage.finish();
        remove_temp_file(&developed)?;
        grained
    } else {
        sampler_step(progress, 3, "grain skipped");
        developed
    };

    let thumbnail_stage = progress_stage_adaptive(
        Some(&apply_progress),
        4,
        5,
        "sampler-thumbnail",
        "thumbnail",
        estimate_sampler_thumbnail_duration(args.thumbnail_long_edge),
    );
    let thumb = profile_temp.join("thumb.jpg");
    finalize_output(&args.convert, &source, &thumb, export)?;
    thumbnail_stage.finish();
    remove_temp_file(&source)?;
    let (width, height) =
        image::image_dimensions(&thumb).with_context(|| format!("reading {}", thumb.display()))?;

    let relative = profile
        .strip_prefix(emulation_root)
        .unwrap_or(profile)
        .display()
        .to_string();
    let name = profile_display_name_from_relative(&relative);
    let parts = profile_name_parts(&name);
    let thumb = SampleThumb {
        image: thumb,
        name,
        filename: relative,
        parts,
        width,
        height,
    };
    sampler_step(progress, 5, "done");
    Ok(thumb)
}

fn run_structured_sheet(
    convert: &Path,
    output: &Path,
    thumbs: &[SampleThumb],
    thumbnail_long_edge: u32,
    jpg_quality: u8,
    progressive_jpeg: bool,
) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let mut trie = ProfileTrie::default();
    for thumb in thumbs {
        trie.insert(thumb.clone_for_tree());
    }
    let layout = build_sheet_layout(&trie, thumbnail_long_edge);
    let svg = render_sheet_svg(&layout);
    let svg_path = output.with_extension("mini-film-sampler.svg");
    fs::write(&svg_path, svg).with_context(|| format!("writing {}", svg_path.display()))?;

    let mut command = Command::new(convert);
    add_convert_thread_limit(&mut command);
    if let Some(font) = sheet_font_path() {
        command.arg("-font").arg(font);
    }
    command
        .arg(&svg_path)
        .arg("-quality")
        .arg(jpg_quality.clamp(1, 100).to_string());
    if progressive_jpeg {
        command.arg("-interlace").arg("Line");
    }
    command.arg(output);

    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output_result = command
        .output()
        .with_context(|| format!("running {}", convert.display()))?;
    if !output_result.status.success() {
        let stdout = String::from_utf8_lossy(&output_result.stdout);
        let stderr = String::from_utf8_lossy(&output_result.stderr);
        bail!(
            "structured sampler sheet export failed with status {}\nsvg kept at {}\nstdout:\n{}\nstderr:\n{}",
            output_result.status,
            svg_path.display(),
            stdout.trim(),
            stderr.trim()
        );
    }
    let _ = fs::remove_file(&svg_path);
    Ok(())
}

fn sampler_progress_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{spinner:.green} sampler [{elapsed_precise}] [{wide_bar:.cyan/blue}] {percent:>3}% {msg}",
    )
    .unwrap()
    .progress_chars("█▌░")
}

fn profile_progress_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{spinner:.green} profile [{wide_bar:.magenta/blue}] {percent:>3}% {msg:.40}",
    )
    .unwrap()
    .progress_chars("█▌░")
}

fn sampler_step(progress: &SamplerProgress, position: u64, step: &str) {
    set_progress(
        &progress.profile,
        progress.started,
        progress_position(position),
        step,
    );
}

fn estimate_sampler_raw_duration(thumbnail_long_edge: u32) -> Duration {
    let scale = (thumbnail_long_edge.max(128) as f64 / 512.0).sqrt();
    Duration::from_secs_f64((1.2 * scale).clamp(0.8, 5.0))
}

fn estimate_sampler_grain_duration(thumbnail_long_edge: u32) -> Duration {
    let pixels = thumbnail_long_edge.max(128) as f64;
    Duration::from_secs_f64((0.20 + pixels / 2400.0).clamp(0.25, 1.5))
}

fn estimate_sampler_thumbnail_duration(thumbnail_long_edge: u32) -> Duration {
    let pixels = thumbnail_long_edge.max(128) as f64;
    Duration::from_secs_f64((0.15 + pixels / 4000.0).clamp(0.2, 1.0))
}

fn estimate_sheet_duration(thumbs: usize) -> Duration {
    Duration::from_secs_f64((0.5 + thumbs as f64 * 0.01).clamp(1.0, 20.0))
}

fn sample_seed(base_seed: u64, index: usize, path: &Path) -> u64 {
    let path_hash = path
        .to_string_lossy()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            hash.wrapping_mul(0x0000_0100_0000_01b3) ^ byte as u64
        });
    base_seed ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ path_hash
}

impl SampleThumb {
    fn clone_for_tree(&self) -> Self {
        Self {
            image: self.image.clone(),
            name: self.name.clone(),
            filename: self.filename.clone(),
            parts: self.parts.clone(),
            width: self.width,
            height: self.height,
        }
    }
}

impl ProfileTrie {
    fn insert(&mut self, thumb: SampleThumb) {
        let mut node = self;
        for part in &thumb.parts {
            node = node.children.entry(part.clone()).or_default();
        }
        node.thumbs.push(thumb);
    }
}

fn profile_display_name_from_relative(relative: &str) -> String {
    let stem = Path::new(relative)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(relative);
    stem.trim().to_string()
}

fn profile_name_parts(name: &str) -> Vec<String> {
    let parts: Vec<_> = name
        .replace(['_', '-', '/', '.'], " ")
        .split_whitespace()
        .map(str::to_string)
        .collect();
    if parts.is_empty() {
        vec!["Profile".to_string()]
    } else {
        parts
    }
}

fn build_sheet_layout(trie: &ProfileTrie, thumbnail_long_edge: u32) -> SheetLayout {
    let mut thumb = thumbnail_long_edge.max(64);
    let columns = sampler_sheet_columns(trie_thumb_count(trie));
    loop {
        let layout = build_sheet_layout_with_thumb(trie, thumb, columns);
        if layout.height < 64_000 || thumb <= 64 {
            return layout;
        }
        thumb = ((thumb as f64) * 0.9).round().max(64.0) as u32;
    }
}

fn build_sheet_layout_with_thumb(trie: &ProfileTrie, thumb: u32, columns: u32) -> SheetLayout {
    let margin = 36u32;
    let indent = (thumb / 7).clamp(28, 96);
    let gap = (thumb / 18).clamp(12, 32);
    let columns = columns.max(1);
    let width = (thumb * columns + margin * 2 + indent * 3 + gap * columns.saturating_sub(1))
        .clamp(1200, 32_000);
    let mut ctx = LayoutContext {
        body: String::new(),
        y: margin,
        width,
        margin,
        indent,
        gap,
        thumb,
    };
    ctx.text(margin, ctx.y, "mini-film sampler", 30, 700, "#111");
    ctx.y += 34;
    ctx.text(
        margin,
        ctx.y,
        "Profiles are grouped by shared name prefixes; indentation shows trie depth.",
        15,
        400,
        "#666",
    );
    ctx.y += 36;
    for (part, child) in &trie.children {
        ctx.render_node(child, &[part.clone()], 0);
    }
    SheetLayout {
        body: ctx.body,
        width,
        height: ctx.y + margin,
    }
}

fn render_sheet_svg(layout: &SheetLayout) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
<rect width="100%" height="100%" fill="white"/>
<style>
{font_css}
.thumb {{ stroke: #d0d0d0; stroke-width: 1; fill: #f8f8f8; }}
</style>
{body}
</svg>
"#,
        width = layout.width,
        height = layout.height,
        font_css = sampler_font_css(),
        body = layout.body
    )
}

struct LayoutContext {
    body: String,
    y: u32,
    width: u32,
    margin: u32,
    indent: u32,
    gap: u32,
    thumb: u32,
}

impl LayoutContext {
    fn render_node(&mut self, node: &ProfileTrie, prefix: &[String], depth: usize) {
        let x = self.margin + self.indent * depth as u32;
        let text = prefix.join(" ");
        let size = match depth {
            0 => 27,
            1 => 22,
            2 => 18,
            _ => 15,
        };
        let weight = if depth <= 1 { 700 } else { 600 };
        self.y += if depth == 0 { 18 } else { 8 };
        self.text(x, self.y, &text, size, weight, header_color(depth));
        self.y += size + 8;

        if depth >= 1 || subtree_depth(node) <= 2 {
            let mut entries = Vec::new();
            collect_subtree_entries(node, prefix.len(), &mut entries);
            if !entries.is_empty() {
                self.render_labeled_thumbs(&entries, x + self.indent);
            }
            return;
        }

        let mut leaf_entries: Vec<_> = node
            .thumbs
            .iter()
            .map(|thumb| {
                sheet_entry(
                    prefix.last().cloned().unwrap_or_else(|| thumb.name.clone()),
                    thumb,
                )
            })
            .collect();
        leaf_entries.extend(
            node.children
                .iter()
                .filter(|(_, child)| child.children.is_empty() && !child.thumbs.is_empty())
                .flat_map(|(part, child)| {
                    child
                        .thumbs
                        .iter()
                        .map(move |thumb| sheet_entry(part.clone(), thumb))
                }),
        );
        leaf_entries.sort_by(|left, right| left.sort_key.cmp(&right.sort_key));
        if !leaf_entries.is_empty() {
            self.render_labeled_thumbs(&leaf_entries, x + self.indent);
        }

        for (part, child) in &node.children {
            if child.children.is_empty() && !child.thumbs.is_empty() {
                continue;
            }
            let mut child_prefix = prefix.to_vec();
            child_prefix.push(part.clone());
            self.render_node(child, &child_prefix, depth + 1);
        }
    }

    fn render_labeled_thumbs(&mut self, entries: &[SheetEntry<'_>], x: u32) {
        let tile = self.thumb + self.gap;
        let label_height = 48u32;
        let available = self.width.saturating_sub(x + self.margin).max(self.thumb);
        let columns = (available / tile).max(1);
        for (index, entry) in entries.iter().enumerate() {
            if index > 0 && index as u32 % columns == 0 {
                self.y += self.thumb + label_height + self.gap;
            }
            let col = index as u32 % columns;
            let tx = x + col * tile;
            let thumb = entry.thumb;
            let (display_width, display_height) = thumb_display_size(thumb, self.thumb);
            self.text(tx, self.y + 18, &entry.label, 16, 500, "#444444");
            self.text(tx, self.y + 36, &entry.full_name, 12, 400, "#777777");
            let ty = self.y + label_height + (self.thumb - display_height) / 2;
            self.rect(tx, ty, display_width, display_height);
            self.image(tx, ty, display_width, display_height, &thumb.image);
        }
        self.y += self.thumb + label_height + self.gap;
    }

    fn text(&mut self, x: u32, y: u32, text: &str, size: u32, weight: u32, color: &str) {
        self.body.push_str(&format!(
            r#"<text x="{x}" y="{y}" font-size="{size}" font-weight="{weight}" fill="{color}">{}</text>
"#,
            escape_xml(text)
        ));
    }

    fn rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        self.body.push_str(&format!(
            r#"<rect class="thumb" x="{x}" y="{y}" width="{width}" height="{height}" rx="2"/>
"#
        ));
    }

    fn image(&mut self, x: u32, y: u32, width: u32, height: u32, path: &Path) {
        self.body.push_str(&format!(
            r#"<image x="{x}" y="{y}" width="{width}" height="{height}" preserveAspectRatio="xMidYMid meet" href="{}"/>
"#,
            escape_xml(&path.to_string_lossy())
        ));
    }
}

fn collect_subtree_entries<'a>(
    node: &'a ProfileTrie,
    prefix_len: usize,
    out: &mut Vec<SheetEntry<'a>>,
) {
    for thumb in &node.thumbs {
        out.push(sheet_entry(
            thumb_label_after_prefix(thumb, prefix_len),
            thumb,
        ));
    }
    for child in node.children.values() {
        collect_subtree_entries(child, prefix_len, out);
    }
    out.sort_by(|left, right| left.sort_key.cmp(&right.sort_key));
}

fn thumb_label_after_prefix(thumb: &SampleThumb, prefix_len: usize) -> String {
    let label = thumb.parts.get(prefix_len..).unwrap_or(&[]).join(" ");
    if label.is_empty() {
        thumb
            .parts
            .last()
            .cloned()
            .unwrap_or_else(|| thumb.name.clone())
    } else {
        label
    }
}

fn sheet_entry(label: String, thumb: &SampleThumb) -> SheetEntry<'_> {
    SheetEntry {
        sort_key: variant_sort_key(&label),
        full_name: thumb.filename.clone(),
        label,
        thumb,
    }
}

fn variant_sort_key(label: &str) -> String {
    let normalized = profile_name_parts(label)
        .into_iter()
        .filter(|part| !part.eq_ignore_ascii_case("grainy"))
        .map(|part| natural_sort_part(&part))
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    let grain_rank = if label
        .split_whitespace()
        .any(|part| part.eq_ignore_ascii_case("grainy"))
    {
        "1"
    } else {
        "0"
    };
    format!(
        "{normalized}\u{0}{grain_rank}\u{0}{}",
        label.to_ascii_lowercase()
    )
}

fn natural_sort_part(part: &str) -> String {
    if let Some(version) = part.strip_prefix('v').or_else(|| part.strip_prefix('V')) {
        if let Ok(number) = version.parse::<u32>() {
            return format!("v{number:06}");
        }
    }
    if let Ok(number) = part.parse::<u32>() {
        return format!("{number:06}");
    }
    part.to_string()
}

fn trie_thumb_count(trie: &ProfileTrie) -> u32 {
    let children: u32 = trie.children.values().map(trie_thumb_count).sum();
    trie.thumbs.len() as u32 + children
}

fn subtree_depth(trie: &ProfileTrie) -> usize {
    trie.children
        .values()
        .map(subtree_depth)
        .max()
        .map_or(0, |depth| depth + 1)
}

fn sampler_sheet_columns(thumb_count: u32) -> u32 {
    thumb_count.clamp(1, 6)
}

fn header_color(depth: usize) -> &'static str {
    match depth {
        0 => "#111111",
        1 => "#333333",
        2 => "#4b4b4b",
        _ => "#666666",
    }
}

fn thumb_display_size(thumb: &SampleThumb, long_edge: u32) -> (u32, u32) {
    let width = thumb.width.max(1) as f64;
    let height = thumb.height.max(1) as f64;
    let scale = long_edge as f64 / width.max(height);
    (
        (width * scale).round().max(1.0) as u32,
        (height * scale).round().max(1.0) as u32,
    )
}

fn file_uri(path: &Path) -> String {
    let encoded = path
        .to_string_lossy()
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('#', "%23");
    format!("file://{encoded}")
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn sampler_font_css() -> String {
    if let Some(path) = pragmata_font_path() {
        format!(
            r#"@font-face {{ font-family: "Pragmata Pro"; src: url("{}"); }}
text {{ font-family: "Pragmata Pro", "DejaVu Sans", "Noto Sans", Arial, sans-serif; letter-spacing: 0; }}"#,
            escape_xml(&file_uri(&path))
        )
    } else {
        r#"text { font-family: "DejaVu Sans", "Noto Sans", Arial, sans-serif; letter-spacing: 0; }"#
            .to_string()
    }
}

fn sheet_font_path() -> Option<PathBuf> {
    pragmata_font_path().or_else(|| {
        [
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
            "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
        ]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
    })
}

fn pragmata_font_path() -> Option<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from)?;
    let fonts = home.join(".fonts");
    if !fonts.is_dir() {
        return None;
    }
    WalkDir::new(fonts)
        .follow_links(true)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .find(|path| {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            stem.contains("pragmata") && matches!(ext.as_str(), "ttf" | "otf" | "ttc")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_thumb(name: &str, width: u32, height: u32) -> SampleThumb {
        SampleThumb {
            image: PathBuf::from(format!("/tmp/{name}.jpg")),
            name: name.to_string(),
            filename: format!("{name}.xmp"),
            parts: profile_name_parts(name),
            width,
            height,
        }
    }

    #[test]
    fn profile_names_strip_extension_and_split_into_visible_levels() {
        assert_eq!(
            profile_display_name_from_relative("Kodak Portra 400 Grainy.xmp"),
            "Kodak Portra 400 Grainy"
        );
        assert_eq!(
            profile_display_name_from_relative("RNI/Kodak Portra 400.xmp"),
            "Kodak Portra 400"
        );
        assert_eq!(
            profile_name_parts("Kodak Portra 400 Grainy"),
            vec!["Kodak", "Portra", "400", "Grainy"]
        );
        assert_eq!(profile_name_parts(""), vec!["Profile"]);
    }

    #[test]
    fn structured_layout_renders_prefix_headers_and_thumbnail_refs() {
        let mut trie = ProfileTrie::default();
        let mut thumb = sample_thumb("Kodak Portra 400 Grainy", 128, 85);
        thumb.image = PathBuf::from("/tmp/kodak.jpg");
        trie.insert(thumb);

        let layout = build_sheet_layout(&trie, 128);
        let svg = render_sheet_svg(&layout);

        assert!(svg.contains(">Kodak<"));
        assert!(svg.contains(">Kodak Portra<"));
        assert!(svg.contains(">400 Grainy<"));
        assert!(svg.contains(">Kodak Portra 400 Grainy.xmp<"));
        assert!(svg.contains("/tmp/kodak.jpg"));
        assert!(svg.contains(r#"width="128" height="85""#));
        assert!(svg.contains("font-family"));
    }

    #[test]
    fn family_layout_keeps_versions_and_grainy_variants_in_one_grid() {
        let mut trie = ProfileTrie::default();
        for name in [
            "Fuji Superia 200 v2",
            "Fuji Superia 200 v2 grainy",
            "Fuji Superia 200 v3",
            "Fuji Superia 200 v3 grainy",
        ] {
            trie.insert(sample_thumb(name, 1024, 683));
        }

        let layout = build_sheet_layout(&trie, 256);
        let svg = render_sheet_svg(&layout);

        assert!(svg.contains(">Fuji<"));
        assert!(svg.contains(">Fuji Superia<"));
        assert!(svg.contains(">200 v2<"));
        assert!(svg.contains(">Fuji Superia 200 v2.xmp<"));
        assert!(svg.contains(">200 v2 grainy<"));
        assert!(svg.contains(">Fuji Superia 200 v2 grainy.xmp<"));
        assert!(svg.contains(">200 v3<"));
        assert!(svg.contains(">200 v3 grainy<"));
        assert!(svg.find(">200 v2<") < svg.find(">200 v2 grainy<"));
        assert!(svg.find(">200 v2 grainy<") < svg.find(">200 v3<"));
        assert!(svg.find(">200 v3<") < svg.find(">200 v3 grainy<"));
        assert!(!svg.contains(">Fuji Superia 200 v2 grainy<"));
    }

    #[test]
    fn large_sampler_layout_stays_below_jpeg_dimension_limit() {
        let mut trie = ProfileTrie::default();
        for film in 0..104 {
            for version in 1..=3 {
                for grainy in [false, true] {
                    let name = if grainy {
                        format!("Fuji Superia {film} v{version} grainy")
                    } else {
                        format!("Fuji Superia {film} v{version}")
                    };
                    trie.insert(sample_thumb(&name, 1024, 683));
                }
            }
        }

        let layout = build_sheet_layout(&trie, 1024);

        assert_eq!(trie_thumb_count(&trie), 624);
        assert!(layout.width < 65_000);
        assert!(layout.height < 65_000);
    }

    #[test]
    fn sampler_columns_are_capped_at_six() {
        assert_eq!(sampler_sheet_columns(414), 6);
        assert_eq!(sampler_sheet_columns(24), 6);
        assert_eq!(sampler_sheet_columns(4), 4);
    }

    #[test]
    fn thumbnail_display_size_preserves_aspect_ratio() {
        let landscape = sample_thumb("Landscape", 6000, 4000);
        let portrait = sample_thumb("Portrait", 3000, 4500);

        assert_eq!(thumb_display_size(&landscape, 512), (512, 341));
        assert_eq!(thumb_display_size(&portrait, 512), (341, 512));
    }

    #[test]
    fn sampler_font_css_prefers_pragmata_when_available() {
        let css = sampler_font_css();
        if pragmata_font_path().is_some() {
            assert!(css.contains("Pragmata Pro"));
            assert!(css.contains("@font-face"));
        } else {
            assert!(css.contains("DejaVu Sans"));
        }
    }

    #[test]
    fn sample_seed_changes_with_index_path_or_base_seed() {
        let path = Path::new("emulations/Fuji.xmp");
        let seed = sample_seed(10, 0, path);
        assert_eq!(seed, sample_seed(10, 0, path));
        assert_ne!(seed, sample_seed(11, 0, path));
        assert_ne!(seed, sample_seed(10, 1, path));
        assert_ne!(seed, sample_seed(10, 0, Path::new("emulations/Kodak.xmp")));
    }

    #[test]
    fn sampler_duration_estimates_are_clamped_and_monotonic() {
        assert_eq!(
            estimate_sampler_raw_duration(1),
            estimate_sampler_raw_duration(128)
        );
        assert!(estimate_sampler_raw_duration(1024) > estimate_sampler_raw_duration(512));
        assert!(estimate_sampler_grain_duration(4096) <= Duration::from_secs_f64(1.5));
        assert!(estimate_sheet_duration(10_000) <= Duration::from_secs(20));
    }

    #[test]
    fn resolve_sampler_jobs_defaults_and_rejects_zero() {
        assert!(resolve_sampler_jobs(None).unwrap() >= 1);
        assert_eq!(resolve_sampler_jobs(Some(4)).unwrap(), 4);
        assert!(resolve_sampler_jobs(Some(0)).is_err());
    }
}
