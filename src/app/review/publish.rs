use super::{
    db::*,
    handle::{retouch_white_balance_for_image, retouch_without_adjustments},
    model::*,
    prelude::*,
    store::*,
};
use mini_film::GrainEngine;

pub(super) fn parse_batch_output_format(raw: &str) -> Result<BatchOutputFormat> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Ok(BatchOutputFormat::Jpg),
        "tif" | "tiff" => Ok(BatchOutputFormat::Tiff),
        other => bail!("unsupported output format {other:?}; expected jpg or tiff"),
    }
}

pub(super) fn parse_gallery_template(raw: &str) -> Result<Option<GalleryTemplate>> {
    let raw = raw.trim().to_ascii_lowercase();
    if raw.is_empty() || raw == "none" {
        return Ok(None);
    }
    let template = match raw.as_str() {
        "modern" => GalleryTemplate::Modern,
        "soft" => GalleryTemplate::Soft,
        "compact" => GalleryTemplate::Compact,
        "hero" => GalleryTemplate::Hero,
        "phone" => GalleryTemplate::Phone,
        "all" => GalleryTemplate::All,
        other => bail!("unsupported gallery template {other:?}"),
    };
    Ok(Some(template))
}

pub(super) fn parse_jpeg_subsampling(raw: &str) -> Result<crate::cli::JpegSubsampling> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "s444" | "444" | "4:4:4" => Ok(crate::cli::JpegSubsampling::S444),
        "s422" | "422" | "4:2:2" => Ok(crate::cli::JpegSubsampling::S422),
        "s420" | "420" | "4:2:0" => Ok(crate::cli::JpegSubsampling::S420),
        other => bail!("unsupported JPEG subsampling {other:?}"),
    }
}

pub(super) fn parse_grain_engine(raw: &str) -> Result<GrainEngine> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "legacy" => Ok(GrainEngine::Legacy),
        "rfgr" => Ok(GrainEngine::Rfgr),
        "rfgrfast" | "rfgr-fast" => Ok(GrainEngine::RfgrFast),
        other => bail!("unsupported grain engine {other:?}; expected legacy, rfgr, or rfgrfast"),
    }
}

pub(super) fn lens_corrections_arg(corrections: LensCorrections) -> String {
    let mut parts = Vec::new();
    if corrections.distortion {
        parts.push("distortion");
    }
    if corrections.ca {
        parts.push("ca");
    }
    if corrections.vignetting {
        parts.push("vignetting");
    }
    parts.join(",")
}

pub(super) fn parse_review_label(raw: &str) -> Result<ReviewLabel> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "none" => Ok(ReviewLabel::None),
        "red" => Ok(ReviewLabel::Red),
        "yellow" => Ok(ReviewLabel::Yellow),
        "green" => Ok(ReviewLabel::Green),
        "blue" => Ok(ReviewLabel::Blue),
        "purple" => Ok(ReviewLabel::Purple),
        other => bail!("unsupported review label {other:?}"),
    }
}

pub(super) fn normalize_tag_filter(tags: &[String]) -> HashSet<String> {
    tags.iter()
        .map(|tag| tag.trim().to_ascii_lowercase())
        .filter(|tag| !tag.is_empty())
        .collect()
}

pub(super) fn validate_relative_publish_album(raw: &str) -> Result<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("publish output directory must not be empty");
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        bail!("publish output directory must be relative to the daemon output directory");
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("publish output directory cannot leave the daemon output directory");
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        bail!("publish output directory must contain a folder name");
    }
    Ok(normalized)
}

pub(crate) fn run_review_publish(args: ReviewPublishCommandArgs) -> Result<()> {
    let report = if args.progress_events {
        let emit = |progress: ReviewPublishProgress| {
            if let Ok(line) = serde_json::to_string(&ReviewPublishEvent::Progress { progress }) {
                println!("{line}");
            }
        };
        let report = publish_review_state(&args, Some(&emit))?;
        println!(
            "{}",
            serde_json::to_string(&ReviewPublishEvent::Report {
                report: report.clone()
            })
            .context("serializing review publish report event")?
        );
        report
    } else {
        publish_review_state(&args, None)?
    };
    if !args.progress_events {
        println!(
            "{}",
            serde_json::to_string(&report).context("serializing review publish report")?
        );
    }
    Ok(())
}

pub(super) fn spawn_review_publish_command<F>(
    args: &ReviewPublishCommandArgs,
    mut on_progress: F,
) -> Result<PublishReport>
where
    F: FnMut(ReviewPublishProgress) -> Result<()>,
{
    let exe = env::current_exe().context("resolving current mini-film executable")?;
    let mut command = Command::new(exe);
    command
        .arg("review-publish")
        .arg("--progress-events")
        .arg("--state")
        .arg(&args.state)
        .arg("--input-root")
        .arg(&args.input_root)
        .arg("--output-root")
        .arg(&args.output_root)
        .arg("--album")
        .arg(&args.album)
        .arg("--min-rating")
        .arg(args.min_rating.to_string())
        .arg("--output-format")
        .arg(args.output_format.to_string())
        .arg("--hald-dir")
        .arg(&args.hald_dir)
        .arg("--profiles-root")
        .arg(&args.profiles_root)
        .arg("--hald-level")
        .arg(args.hald_level.to_string())
        .arg("--rawtherapee")
        .arg(&args.rawtherapee)
        .arg("--convert")
        .arg(&args.convert)
        .arg("--jobs")
        .arg(args.jobs.to_string())
        .arg("--gallery-thumbnail-long-edge")
        .arg(args.gallery_thumbnail_long_edge.to_string())
        .arg("--gallery-columns")
        .arg(args.gallery_columns.to_string())
        .arg("--jpg-quality")
        .arg(args.export.jpg_quality.to_string())
        .arg("--jpeg-subsampling")
        .arg(args.export.jpeg_subsampling.to_string());
    args.dng_fallback.append_cli_args(&mut command);
    if let Some(lcp_root) = args.lcp_root.as_ref() {
        command.arg("--lcp-root").arg(lcp_root);
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(gallery) = args.gallery {
        command.arg("--gallery").arg(gallery.to_string());
    }
    for label in &args.labels {
        command.arg("--label").arg(label);
    }
    for tag in &args.tags {
        command.arg("--tag").arg(tag);
    }
    if let Some(resize) = &args.export.resize {
        command.arg("--resize").arg(resize);
    }
    if let Some(long_edge) = args.export.long_edge {
        command.arg("--long-edge").arg(long_edge.to_string());
    }
    if let Some(max_width) = args.export.max_width {
        command.arg("--max-width").arg(max_width.to_string());
    }
    if let Some(max_height) = args.export.max_height {
        command.arg("--max-height").arg(max_height.to_string());
    }
    if args.export.strip_metadata {
        command.arg("--strip-metadata");
    }
    if args.export.progressive_jpeg {
        command.arg("--progressive");
    }
    if args.rerender_raw {
        command.arg("--rerender-raw");
    }
    if args.no_grain {
        command.arg("--no-grain");
    }
    if args.lens_corrections.is_enabled() {
        command
            .arg("--lens-corrections")
            .arg(lens_corrections_arg(args.lens_corrections));
    }
    command
        .arg("--color-noise-iso-threshold")
        .arg(args.color_noise_iso_threshold.to_string());
    if let Some(grain) = &args.grain {
        command.arg("--grain").arg(grain);
    }
    if let Some(grain_preset) = &args.grain_preset {
        command.arg("--grain-preset").arg(grain_preset);
    }
    if let Some(grain_seed) = args.grain_seed {
        command.arg("--grain-seed").arg(grain_seed.to_string());
    }
    command
        .arg("--grain-engine")
        .arg(args.grain_engine.to_string());

    let mut child = command
        .spawn()
        .context("starting mini-film review-publish")?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("review-publish stdout pipe was not available"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("review-publish stderr pipe was not available"))?;
    let stderr_reader = thread::Builder::new()
        .name("mini-film-review-publish-stderr".to_string())
        .spawn(move || {
            let mut stderr_text = String::new();
            let _ = BufReader::new(stderr).read_to_string(&mut stderr_text);
            stderr_text
        })
        .context("starting review-publish stderr reader")?;

    let mut report = None;
    for line in BufReader::new(stdout).lines() {
        let line = line.context("reading review-publish progress")?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<ReviewPublishEvent>(&line) {
            Ok(ReviewPublishEvent::Progress { progress }) => on_progress(progress)?,
            Ok(ReviewPublishEvent::Report {
                report: final_report,
            }) => report = Some(final_report),
            Err(error) => {
                bail!("parsing review-publish event {line:?}: {error}");
            }
        }
    }

    let status = child.wait().context("waiting for review-publish")?;
    let stderr = stderr_reader
        .join()
        .unwrap_or_else(|_| "stderr reader thread panicked".to_string());
    if !status.success() {
        bail!(
            "review-publish failed with status {}\nstderr:\n{}",
            status,
            stderr.trim()
        );
    }
    report.ok_or_else(|| anyhow!("review-publish completed without a report event"))
}

pub(super) fn publish_review_state(
    args: &ReviewPublishCommandArgs,
    progress: Option<ReviewPublishProgressSink<'_>>,
) -> Result<PublishReport> {
    validate_export_options(&args.export)?;
    if args.jobs == 0 {
        bail!("--jobs must be at least 1");
    }
    let input_root = canonical_existing_dir(&args.input_root)?;
    let output_root = canonical_existing_dir(&args.output_root)?;
    let state = resolve_review_state_for_publish(&args.state, &input_root, &output_root)?;
    let store = load_store_for_publish(&state, &input_root, &output_root)?
        .ok_or_else(|| anyhow!("review state is empty"))?;
    let album = validate_relative_publish_album(&args.album)?;
    ensure_safe_dir_all(&output_root, &album)?;

    let labels = args
        .labels
        .iter()
        .map(|label| parse_review_label(label))
        .collect::<Result<HashSet<_>>>()?
        .into_iter()
        .filter(|label| *label != ReviewLabel::None)
        .collect::<HashSet<_>>();
    let tags = normalize_tag_filter(&args.tags);
    let options = ReviewPublishOptions {
        album,
        min_rating: args.min_rating.min(5),
        labels,
        tags,
        output_format: args.output_format,
        hald_dir: args.hald_dir.clone(),
        profiles_root: args.profiles_root.clone(),
        hald_level: args.hald_level,
        rawtherapee: args.rawtherapee.clone(),
        dng_fallback: args.dng_fallback.clone(),
        lcp_root: args.lcp_root.clone(),
        convert: args.convert.clone(),
        jobs: args.jobs,
        export: args.export.clone(),
        rerender_raw: args.rerender_raw,
        no_grain: args.no_grain,
        color_noise_iso_threshold: args.color_noise_iso_threshold,
        lens_corrections: args.lens_corrections,
        grain: args.grain.clone(),
        grain_preset: args.grain_preset.clone(),
        grain_seed: args.grain_seed,
        grain_engine: args.grain_engine,
        write_metadata: true,
    };
    let mut report = publish_store_inner(&store, &input_root, &output_root, &options, progress)?;

    if let Some(template) = args.gallery {
        let mut rendered = 0u64;
        for root in &report.gallery_roots {
            emit_publish_progress(
                progress,
                ReviewPublishProgress {
                    processed: report.linked,
                    total: report.linked,
                    linked: report.linked,
                    skipped: report.skipped,
                    galleries: rendered,
                    step: "gallery".to_string(),
                    current: root
                        .file_name()
                        .and_then(|name| name.to_str())
                        .map(ToString::to_string),
                },
            );
            render_gallery_for_folder(
                root,
                &FolderGalleryOptions {
                    convert: &args.convert,
                    template,
                    columns: args.gallery_columns,
                    thumbnail_long_edge: args.gallery_thumbnail_long_edge,
                    jobs: args.jobs,
                    export: &args.export,
                    profile_stem: root
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("review"),
                },
            )?;
            rendered += 1;
            emit_publish_progress(
                progress,
                ReviewPublishProgress {
                    processed: report.linked,
                    total: report.linked,
                    linked: report.linked,
                    skipped: report.skipped,
                    galleries: rendered,
                    step: "gallery".to_string(),
                    current: None,
                },
            );
        }
        report.galleries = rendered;
    }

    Ok(report)
}

pub(super) fn publish_store_inner(
    store: &ReviewStore,
    input_root: &Path,
    output_root: &Path,
    options: &ReviewPublishOptions,
    progress: Option<ReviewPublishProgressSink<'_>>,
) -> Result<PublishReport> {
    let mut report = PublishReport {
        min_rating: options.min_rating,
        ..PublishReport::default()
    };
    let publish_root = ensure_safe_dir_all(output_root, &options.album)?;
    let mut tasks = Vec::new();
    for image in &store.images {
        if !image_passes_publish_filters(image, options) {
            report.skipped += 1;
            continue;
        }

        if image_is_direct_compressed(image) {
            if image.preview.status != ReviewRenderStatus::Done {
                report.skipped += 1;
                continue;
            }
            let Some(source) = &image.preview.path else {
                report.skipped += 1;
                continue;
            };
            let source = safe_existing_managed_output_source(
                source,
                &image.raw_path,
                input_root,
                output_root,
            )?;
            let raw_stem = Path::new(&image.relative_path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| {
                    anyhow!("review image has no valid stem: {}", image.relative_path)
                })?;
            let file_name = review_publish_source_file_name(raw_stem, options.output_format)?;
            let destination_relative = options.album.join(file_name);
            let destination = safe_child_path(output_root, &destination_relative)?;
            tasks.push(ReviewPublishTask {
                source,
                destination,
                image: image.clone(),
                render: None,
                profile: None,
                current: image.file_name.clone(),
            });
            continue;
        }

        let publish_indexes = effective_publish_profile_indexes(image);
        if publish_indexes.is_empty() {
            report.skipped += 1;
            continue;
        }

        let raw_stem = Path::new(&image.relative_path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| anyhow!("review image has no valid stem: {}", image.relative_path))?;
        let default_profile_index = store.profiles.first().map(|profile| profile.index);
        for profile_index in publish_indexes {
            let Some(render) = image
                .profiles
                .iter()
                .find(|render| render.profile_index == profile_index)
            else {
                report.skipped += 1;
                continue;
            };
            if render.status != ReviewRenderStatus::Done {
                report.skipped += 1;
                continue;
            }
            let Some(source) = &render.output_path else {
                report.skipped += 1;
                continue;
            };
            let source = if is_sooc_render(render) {
                let original = image_sooc_source(image).ok_or_else(|| {
                    anyhow!("review image has no SOOC source: {}", image.relative_path)
                })?;
                safe_existing_managed_output_source(source, original, input_root, output_root)?
            } else {
                safe_existing_output_source(source, output_root)?
            };
            let file_name = review_publish_file_name(
                raw_stem,
                render,
                default_profile_index,
                options.output_format,
            )?;
            let destination_relative = options.album.join(file_name);
            let destination = safe_child_path(output_root, &destination_relative)?;
            let profile = if profile_index == SOOC_PROFILE_INDEX {
                None
            } else {
                Some(
                    store
                        .profiles
                        .iter()
                        .find(|profile| profile.index == profile_index)
                        .ok_or_else(|| {
                            anyhow!("review profile index {profile_index} is not configured")
                        })?,
                )
            };
            tasks.push(ReviewPublishTask {
                source,
                destination,
                image: image.clone(),
                render: Some(render.clone()),
                profile: profile.cloned(),
                current: format!(
                    "{} / {}",
                    image.file_name,
                    render
                        .display_name
                        .as_deref()
                        .unwrap_or(render.profile_stem.as_str())
                ),
            });
        }
    }

    let expected_output_file_names = tasks
        .iter()
        .filter_map(|task| {
            task.destination
                .file_name()
                .and_then(|file_name| file_name.to_str())
                .map(|file_name| file_name.to_owned())
        })
        .collect::<HashSet<_>>();

    let total = tasks.len() as u64;
    emit_publish_progress(
        progress,
        ReviewPublishProgress {
            processed: 0,
            total,
            linked: 0,
            skipped: report.skipped,
            galleries: 0,
            step: "publish".to_string(),
            current: None,
        },
    );

    if total > 0 {
        let skipped = report.skipped;
        let processed = AtomicU64::new(0);
        let linked = AtomicU64::new(0);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(options.jobs)
            .build()
            .context("building review publish thread pool")?;
        pool.install(|| {
            tasks.par_iter().try_for_each(|task| {
                let step = if options.rerender_raw {
                    "rerender"
                } else {
                    "link"
                };
                publish_review_output(ReviewPublishOutput {
                    input_root,
                    source: &task.source,
                    destination: &task.destination,
                    image: &task.image,
                    render: task.render.as_ref(),
                    profile: task.profile.as_ref(),
                    options,
                })?;
                let linked_now = linked.fetch_add(1, Ordering::Relaxed) + 1;
                let processed_now = processed.fetch_add(1, Ordering::Relaxed) + 1;
                emit_publish_progress(
                    progress,
                    ReviewPublishProgress {
                        processed: processed_now,
                        total,
                        linked: linked_now,
                        skipped,
                        galleries: 0,
                        step: step.to_string(),
                        current: Some(task.current.clone()),
                    },
                );
                Ok::<_, anyhow::Error>(())
            })
        })?;
        report.linked = linked.load(Ordering::Relaxed);
        if report.linked > 0 {
            report.gallery_roots.push(publish_root.clone());
        }
    }
    cleanup_stale_publish_outputs(&publish_root, &expected_output_file_names)?;
    emit_publish_progress(
        progress,
        ReviewPublishProgress {
            processed: report.linked,
            total,
            linked: report.linked,
            skipped: report.skipped,
            galleries: report.galleries,
            step: "publish".to_string(),
            current: None,
        },
    );
    Ok(report)
}

fn cleanup_stale_publish_outputs(
    publish_root: &Path,
    expected_file_names: &HashSet<String>,
) -> Result<()> {
    for entry in fs::read_dir(publish_root)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !(file_type.is_file() || file_type.is_symlink()) {
            continue;
        }
        let file_name_os = entry.file_name();
        let Some(file_name) = file_name_os.to_str() else {
            continue;
        };
        if expected_file_names.contains(file_name) {
            continue;
        }
        let entry_path = entry.path();
        let Some(extension) = entry_path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if !extension.eq_ignore_ascii_case("jpg")
            && !extension.eq_ignore_ascii_case("jpeg")
            && !extension.eq_ignore_ascii_case("tif")
            && !extension.eq_ignore_ascii_case("tiff")
        {
            continue;
        }
        fs::remove_file(&entry_path)
            .with_context(|| format!("removing stale publish output {}", entry_path.display()))?;
    }
    Ok(())
}

pub(super) fn emit_publish_progress(
    progress: Option<ReviewPublishProgressSink<'_>>,
    event: ReviewPublishProgress,
) {
    if let Some(progress) = progress {
        progress(event);
    }
}

pub(super) fn image_passes_publish_filters(
    image: &ReviewImage,
    options: &ReviewPublishOptions,
) -> bool {
    if image.rating < options.min_rating {
        return false;
    }
    if !options.labels.is_empty()
        && image_review_labels(image)
            .iter()
            .all(|label| !options.labels.contains(label))
    {
        return false;
    }
    if !options.tags.is_empty()
        && !image
            .tags
            .iter()
            .map(|tag| tag.to_ascii_lowercase())
            .any(|tag| options.tags.contains(&tag))
    {
        return false;
    }
    true
}

pub(super) fn publish_review_output(item: ReviewPublishOutput<'_>) -> Result<()> {
    if item.destination.exists() {
        fs::remove_file(item.destination)
            .with_context(|| format!("removing {}", item.destination.display()))?;
    }
    if item.options.rerender_raw {
        if item.render.is_some_and(is_sooc_render) {
            rerender_sooc_review_output(
                item.input_root,
                item.destination,
                item.image,
                item.options,
            )?;
        } else if let Some(profile) = item.profile {
            rerender_review_output(
                item.input_root,
                item.destination,
                item.image,
                profile,
                item.options,
            )?;
        } else {
            rerender_compressed_review_output(
                item.input_root,
                item.destination,
                item.image,
                item.options,
            )?;
        }
    } else if item.options.write_metadata && item.source.starts_with(item.input_root) {
        fs::copy(item.source, item.destination).with_context(|| {
            format!(
                "copying original {} before writing publish metadata",
                item.source.display()
            )
        })?;
    } else if fs::hard_link(item.source, item.destination).is_err() {
        symlink_file(item.source, item.destination).with_context(|| {
            format!(
                "symlinking {} to {} after hardlink failed",
                item.source.display(),
                item.destination.display()
            )
        })?;
    }
    if item.options.write_metadata {
        let profile_stem = item
            .render
            .map(|render| render.profile_stem.as_str())
            .unwrap_or("source");
        write_review_metadata(item.destination, item.image, profile_stem)?;
    }
    Ok(())
}

pub(super) fn is_sooc_render(render: &ReviewProfileRender) -> bool {
    render.profile_index == SOOC_PROFILE_INDEX || render.profile_stem == SOOC_PROFILE_STEM
}

pub(super) fn rerender_review_output(
    input_root: &Path,
    destination: &Path,
    image: &ReviewImage,
    profile: &ReviewProfile,
    options: &ReviewPublishOptions,
) -> Result<()> {
    let raw = safe_existing_raw_source(&image.raw_path, input_root)?;
    run_apply(ApplyArgs {
        raw,
        output: destination.to_path_buf(),
        profile: optional_profile_selector(&profile.selector),
        hald_dir: options.hald_dir.clone(),
        profiles_root: options.profiles_root.clone(),
        hald_level: options.hald_level,
        rawtherapee: options.rawtherapee.clone(),
        dng_fallback: options.dng_fallback.clone(),
        convert: options.convert.clone(),
        keep_intermediate: None,
        no_grain: options.no_grain,
        color_noise_iso_threshold: options.color_noise_iso_threshold,
        lens_corrections: options.lens_corrections,
        grain: options.grain.clone(),
        grain_preset: options.grain_preset.clone(),
        lcp_root: options.lcp_root.clone(),
        grain_seed: options
            .grain_seed
            .map(|seed| review_publish_seed(seed, &image.raw_path, profile.index)),
        grain_engine: options.grain_engine,
        export: options.export.clone(),
        retouch: Some(image.retouch.clone()),
        retouch_white_balance: retouch_white_balance_for_image(image),
        bw_filter: effective_bw_filter_for_profile(image, profile),
    })
}

pub(super) fn rerender_compressed_review_output(
    input_root: &Path,
    destination: &Path,
    image: &ReviewImage,
    options: &ReviewPublishOptions,
) -> Result<()> {
    let input = safe_existing_raw_source(&image.raw_path, input_root)?;
    apply_compressed(
        CompressedApplyJob {
            input: &input,
            output: destination,
            convert: &options.convert,
            export: &options.export,
            exif_comment: Some(format!(
                "mini-film {} usage=review-publish compressed-input {}",
                env!("CARGO_PKG_VERSION"),
                image.retouch.summary()
            )),
            retouch: Some(&image.retouch),
        },
        None,
    )
}

pub(super) fn rerender_sooc_review_output(
    input_root: &Path,
    destination: &Path,
    image: &ReviewImage,
    options: &ReviewPublishOptions,
) -> Result<()> {
    let sidecar = image_sooc_source(image)
        .ok_or_else(|| anyhow!("review image has no SOOC source: {}", image.relative_path))?;
    let sidecar = safe_existing_raw_source(sidecar, input_root)?;
    let retouch = retouch_without_adjustments(&image.retouch);
    apply_compressed(
        CompressedApplyJob {
            input: &sidecar,
            output: destination,
            convert: &options.convert,
            export: &options.export,
            exif_comment: Some(format!(
                "mini-film {} usage=review-publish sooc-sidecar {}",
                env!("CARGO_PKG_VERSION"),
                retouch.summary()
            )),
            retouch: Some(&retouch),
        },
        None,
    )
}

pub(super) fn optional_profile_selector(selector: &str) -> Option<String> {
    let selector = selector.trim();
    (!selector.is_empty()).then(|| selector.to_string())
}

pub(super) fn canonical_existing_dir(path: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path)
        .with_context(|| format!("canonicalizing directory {}", path.display()))?;
    if !canonical.is_dir() {
        bail!("not a directory: {}", canonical.display());
    }
    Ok(canonical)
}

pub(super) fn ensure_path_within(path: &Path, root: &Path) -> Result<()> {
    if path.starts_with(root) {
        return Ok(());
    }
    bail!(
        "path {} is outside of configured root {}",
        path.display(),
        root.display()
    )
}

pub(super) fn safe_relative_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        bail!("expected relative path, got {}", path.display());
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("unsafe relative path component in {}", path.display());
            }
        }
    }
    Ok(normalized)
}

pub(super) fn safe_child_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    Ok(root.join(safe_relative_path(relative)?))
}

pub(super) fn ensure_safe_dir_all(root: &Path, relative: &Path) -> Result<PathBuf> {
    let relative = safe_relative_path(relative)?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            continue;
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    bail!(
                        "refusing to write through symlink directory {}",
                        current.display()
                    );
                }
                if !metadata.is_dir() {
                    bail!(
                        "publish path component is not a directory: {}",
                        current.display()
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)
                    .with_context(|| format!("creating {}", current.display()))?;
            }
            Err(error) => {
                return Err(error).with_context(|| format!("checking {}", current.display()));
            }
        }
    }
    Ok(current)
}

pub(super) fn safe_existing_output_source(source: &Path, output_root: &Path) -> Result<PathBuf> {
    let source = fs::canonicalize(source)
        .with_context(|| format!("canonicalizing review output {}", source.display()))?;
    ensure_path_within(&source, output_root)?;
    if !source.is_file() {
        bail!("review output is not a file: {}", source.display());
    }
    Ok(source)
}

pub(super) fn safe_existing_managed_output_source(
    source: &Path,
    original: &Path,
    input_root: &Path,
    output_root: &Path,
) -> Result<PathBuf> {
    let source = fs::canonicalize(source)
        .with_context(|| format!("canonicalizing managed review output {}", source.display()))?;
    if source.starts_with(output_root) {
        if !source.is_file() {
            bail!("managed review output is not a file: {}", source.display());
        }
        return Ok(source);
    }

    let original = fs::canonicalize(original)
        .with_context(|| format!("canonicalizing original source {}", original.display()))?;
    ensure_path_within(&original, input_root)?;
    if source != original {
        bail!(
            "managed review output {} does not point to expected original {}",
            source.display(),
            original.display()
        );
    }
    if !source.is_file() {
        bail!("managed review output is not a file: {}", source.display());
    }
    Ok(source)
}

pub(super) fn safe_existing_raw_source(raw: &Path, input_root: &Path) -> Result<PathBuf> {
    if raw.starts_with(input_root) && raw.is_file() {
        return Ok(raw.to_path_buf());
    }
    let canonical = fs::canonicalize(raw)
        .with_context(|| format!("canonicalizing RAW source {}", raw.display()))?;
    ensure_path_within(&canonical, input_root)?;
    if !canonical.is_file() {
        bail!("review RAW source is not a file: {}", raw.display());
    }
    Ok(raw.to_path_buf())
}

pub(super) fn review_publish_seed(base_seed: u64, raw: &Path, profile_index: usize) -> u64 {
    let mut hasher = Sha1::new();
    hasher.update(base_seed.to_le_bytes());
    hasher.update(raw.to_string_lossy().as_bytes());
    hasher.update((profile_index as u64).to_le_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    u64::from_le_bytes(bytes)
}

#[cfg(unix)]
pub(super) fn symlink_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, destination)
}

#[cfg(windows)]
pub(super) fn symlink_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(source, destination)
}

pub(super) fn review_publish_file_name(
    raw_stem: &str,
    render: &ReviewProfileRender,
    default_profile_index: Option<usize>,
    output_format: BatchOutputFormat,
) -> Result<String> {
    let stem = sanitize_filename::sanitize(raw_stem).into_owned();
    let stem = if stem.trim().is_empty() {
        "image".to_string()
    } else {
        stem
    };
    let suffix = if Some(render.profile_index) == default_profile_index {
        String::new()
    } else {
        format!("-{}", review_profile_folder_name(&render.profile_stem))
    };
    Ok(format!("{stem}{suffix}.{}", output_format.extension()))
}

pub(super) fn review_publish_source_file_name(
    raw_stem: &str,
    output_format: BatchOutputFormat,
) -> Result<String> {
    let stem = sanitize_filename::sanitize(raw_stem).into_owned();
    let stem = if stem.trim().is_empty() {
        "image".to_string()
    } else {
        stem
    };
    Ok(format!("{stem}.{}", output_format.extension()))
}

pub(super) fn review_profile_folder_name(profile_stem: &str) -> String {
    let folder = sanitize_filename::sanitize(profile_stem).into_owned();
    if folder.trim().is_empty() {
        "profile".to_string()
    } else {
        folder
    }
}

pub(super) fn write_review_metadata(
    path: &Path,
    image: &ReviewImage,
    profile_stem: &str,
) -> Result<()> {
    let labels = image_review_labels(image);
    let labels_text = review_labels_text(&labels);
    let mut command = Command::new("exiftool");
    command
        .arg("-overwrite_original")
        .arg("-P")
        .arg("-q")
        .arg("-q")
        .arg(format!("-Rating={}", image.rating))
        .arg(format!("-XMP:Rating={}", image.rating))
        .arg(format!("-Label={labels_text}"))
        .arg(format!("-XMP:Label={labels_text}"))
        .arg(format!("-XMP:PreservedFileName={}", image.file_name))
        .arg(format!("-XMP:Nickname={}", image.relative_path))
        .arg(format!(
            "-UserComment=mini-film {} review profile={} rating={} labels={} {} notes={}",
            env!("CARGO_PKG_VERSION"),
            if profile_stem.trim().is_empty() {
                "none"
            } else {
                profile_stem
            },
            image.rating,
            labels_text,
            image.retouch.summary(),
            image.notes
        ));
    if !image.notes.trim().is_empty() {
        command.arg(format!("-Description={}", image.notes.trim()));
        command.arg(format!("-ImageDescription={}", image.notes.trim()));
    }
    for tag in &image.tags {
        command.arg(format!("-Subject+={tag}"));
    }
    command.arg(path);
    let output = command
        .output()
        .with_context(|| format!("running exiftool for {}", path.display()))?;
    if !output.status.success() {
        bail!(
            "exiftool failed for {} with status {}\nstderr:\n{}",
            path.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}
