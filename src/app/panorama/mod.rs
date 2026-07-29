mod hugin;
mod prepare;
mod tools;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use anyhow::{Context, Result, bail};
use rayon::prelude::*;
use sha1::{Digest, Sha1};
use tempfile::Builder;

use crate::{
    app::{
        dng::DngFallbackConfig,
        util::{InputFileFilter, is_supported_input_file},
    },
    cli::{LensCorrections, PanoramaMatchingMode, PanoramaProjection},
};

use tools::HuginToolchain;
pub(crate) use tools::PanoramaCapability;

const PANORAMA_CACHE_VERSION: &str = "panorama-v3-adobe-dcp";

#[derive(Clone, Debug)]
pub(crate) struct PanoramaConfig {
    pub(crate) hugin_bin_dir: Option<PathBuf>,
    pub(crate) rawtherapee: PathBuf,
    pub(crate) dng_fallback: DngFallbackConfig,
    pub(crate) convert: PathBuf,
    pub(crate) jobs: usize,
    pub(crate) color_noise_iso_threshold: u32,
    pub(crate) lens_corrections: LensCorrections,
    pub(crate) lcp_root: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub(crate) struct PanoramaCommandArgs {
    pub(crate) input: Vec<PathBuf>,
    pub(crate) output: PathBuf,
    pub(crate) matching: PanoramaMatchingMode,
    pub(crate) projection: PanoramaProjection,
    pub(crate) config: PanoramaConfig,
    pub(crate) overwrite: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct PanoramaProgress {
    pub(crate) stage: String,
    pub(crate) completed: usize,
    pub(crate) total: usize,
    pub(crate) current: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct PanoramaPreview {
    pub(crate) projection: PanoramaProjection,
    pub(crate) path: PathBuf,
}

pub(crate) type PanoramaProgressSink = Arc<dyn Fn(PanoramaProgress) + Send + Sync>;

pub(crate) fn run_panorama(args: PanoramaCommandArgs) -> Result<()> {
    validate_sources(&args.input)?;
    validate_tiff_output(&args.output, args.overwrite)?;
    let work = Builder::new().prefix("mini-film-panorama-").tempdir()?;
    render_final(
        &args.config,
        &args.input,
        work.path(),
        args.matching,
        args.projection,
        &args.output,
        args.overwrite,
        None,
    )?;
    eprintln!("wrote panorama {}", args.output.display());
    Ok(())
}

pub(crate) fn render_preview_row(
    config: &PanoramaConfig,
    sources: &[PathBuf],
    cache_root: &Path,
    matching: PanoramaMatchingMode,
    progress: Option<PanoramaProgressSink>,
) -> Result<Vec<PanoramaPreview>> {
    validate_sources(sources)?;
    let tools = HuginToolchain::discover(config.hugin_bin_dir.as_deref())?;
    let project_root = project_cache_root(config, &tools, sources, cache_root)?;
    let prepared = prepare_sources(
        config,
        sources,
        &project_root.join("prepared-preview"),
        PreparationTier::Preview,
        progress.as_ref(),
    )?;
    emit(
        progress.as_ref(),
        "align-preview",
        0,
        1,
        Some(matching.to_string()),
    );
    let aligned = hugin::align_project(
        &tools,
        &prepared,
        &project_root
            .join("hugin-preview")
            .join(matching.to_string()),
        matching,
        config.jobs,
    )?;
    emit(
        progress.as_ref(),
        "align-preview",
        1,
        1,
        Some(matching.to_string()),
    );

    let completed = Arc::new(AtomicUsize::new(0));
    let projection_jobs = config.jobs.max(1).div_ceil(2);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2.min(config.jobs.max(1)))
        .build()
        .context("building panorama projection pool")?;
    let results = pool.install(|| {
        PanoramaProjection::ALL
            .par_iter()
            .map(|projection| {
                let output = project_root
                    .join("preview-renders")
                    .join(matching.to_string())
                    .join(format!("{projection}.jpg"));
                if let Some(parent) = output.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("creating {}", parent.display()))?;
                }
                hugin::stitch_projection(
                    &tools,
                    &config.convert,
                    hugin::StitchProjectionOptions {
                        aligned: &aligned,
                        work_dir: &project_root
                            .join("hugin-preview")
                            .join(matching.to_string()),
                        projection: *projection,
                        output: &output,
                        preview: true,
                    },
                    projection_jobs,
                )?;
                let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                emit(
                    progress.as_ref(),
                    "render-previews",
                    count,
                    PanoramaProjection::ALL.len(),
                    Some(projection.to_string()),
                );
                Ok(PanoramaPreview {
                    projection: *projection,
                    path: output,
                })
            })
            .collect::<Result<Vec<_>>>()
    })?;
    Ok(results)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_final(
    config: &PanoramaConfig,
    sources: &[PathBuf],
    cache_root: &Path,
    matching: PanoramaMatchingMode,
    projection: PanoramaProjection,
    output: &Path,
    overwrite: bool,
    progress: Option<PanoramaProgressSink>,
) -> Result<()> {
    validate_sources(sources)?;
    validate_tiff_output(output, overwrite)?;
    let tools = HuginToolchain::discover(config.hugin_bin_dir.as_deref())?;
    let project_root = project_cache_root(config, &tools, sources, cache_root)?;
    let prepared = prepare_sources(
        config,
        sources,
        &project_root.join("prepared-full"),
        PreparationTier::Full,
        progress.as_ref(),
    )?;
    emit(
        progress.as_ref(),
        "align-full",
        0,
        1,
        Some(matching.to_string()),
    );
    let hugin_root = project_root.join("hugin-full").join(matching.to_string());
    let aligned = hugin::align_project(&tools, &prepared, &hugin_root, matching, config.jobs)?;
    emit(
        progress.as_ref(),
        "align-full",
        1,
        1,
        Some(matching.to_string()),
    );
    let cached = project_root
        .join("final-renders")
        .join(matching.to_string())
        .join(format!("{projection}.tif"));
    if let Some(parent) = cached.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    emit(
        progress.as_ref(),
        "stitch-full",
        0,
        1,
        Some(projection.to_string()),
    );
    hugin::stitch_projection(
        &tools,
        &config.convert,
        hugin::StitchProjectionOptions {
            aligned: &aligned,
            work_dir: &hugin_root,
            projection,
            output: &cached,
            preview: false,
        },
        config.jobs,
    )?;
    emit(
        progress.as_ref(),
        "stitch-full",
        1,
        1,
        Some(projection.to_string()),
    );
    publish_result(&cached, &sources[0], output, overwrite)?;
    emit(progress.as_ref(), "complete", 1, 1, None);
    Ok(())
}

fn prepare_sources(
    config: &PanoramaConfig,
    sources: &[PathBuf],
    output_root: &Path,
    tier: PreparationTier,
    progress: Option<&PanoramaProgressSink>,
) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(output_root)
        .with_context(|| format!("creating {}", output_root.display()))?;
    let completed = Arc::new(AtomicUsize::new(0));
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.jobs.max(1))
        .build()
        .context("building panorama source preparation pool")?;
    pool.install(|| {
        sources
            .par_iter()
            .enumerate()
            .map(|(index, source)| {
                let extension = match tier {
                    PreparationTier::Preview => "jpg",
                    PreparationTier::Full => "tif",
                };
                let output = output_root.join(format!("{index:04}.{extension}"));
                match tier {
                    PreparationTier::Preview => {
                        prepare::prepare_preview_source(config, source, &output)?
                    }
                    PreparationTier::Full => prepare::prepare_full_source(config, source, &output)?,
                }
                let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                emit(
                    progress,
                    match tier {
                        PreparationTier::Preview => "prepare-previews",
                        PreparationTier::Full => "prepare-full",
                    },
                    count,
                    sources.len(),
                    source
                        .file_name()
                        .and_then(|name| name.to_str())
                        .map(ToString::to_string),
                );
                Ok(output)
            })
            .collect::<Result<Vec<_>>>()
    })
}

fn project_cache_root(
    config: &PanoramaConfig,
    tools: &HuginToolchain,
    sources: &[PathBuf],
    cache_root: &Path,
) -> Result<PathBuf> {
    let mut hasher = Sha1::new();
    hasher.update(PANORAMA_CACHE_VERSION.as_bytes());
    hasher.update(tools.fingerprint().as_bytes());
    hasher.update(config.color_noise_iso_threshold.to_le_bytes());
    hasher.update([
        u8::from(config.lens_corrections.distortion),
        u8::from(config.lens_corrections.ca),
        u8::from(config.lens_corrections.vignetting),
    ]);
    if let Some(lcp_root) = &config.lcp_root {
        hasher.update(lcp_root.to_string_lossy().as_bytes());
    }
    for source in sources {
        hasher.update(crate::app::dcp::dcp_cache_identity(
            source,
            &config.dng_fallback,
        ));
        let canonical = fs::canonicalize(source).unwrap_or_else(|_| source.clone());
        hasher.update(canonical.to_string_lossy().as_bytes());
        let metadata = fs::metadata(source)
            .with_context(|| format!("reading panorama source metadata {}", source.display()))?;
        hasher.update(metadata.len().to_le_bytes());
        if let Ok(modified) = metadata.modified()
            && let Ok(elapsed) = modified.duration_since(std::time::UNIX_EPOCH)
        {
            hasher.update(elapsed.as_nanos().to_le_bytes());
        }
    }
    let digest = hasher.finalize();
    let key = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(cache_root.join(PANORAMA_CACHE_VERSION).join(key))
}

fn publish_result(cached: &Path, source: &Path, output: &Path, overwrite: bool) -> Result<()> {
    let parent = output
        .parent()
        .with_context(|| format!("panorama output has no parent: {}", output.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let staged = Builder::new()
        .prefix(".mini-film-panorama-result-")
        .suffix(".tmp")
        .tempfile_in(parent)
        .with_context(|| format!("creating staged panorama in {}", parent.display()))?
        .into_temp_path();
    fs::copy(cached, &staged).with_context(|| {
        format!(
            "copying cached panorama {} to {}",
            cached.display(),
            staged.display()
        )
    })?;
    prepare::copy_panorama_result_metadata(source, &staged)?;
    if overwrite {
        staged
            .persist(output)
            .map_err(|error| error.error)
            .with_context(|| format!("publishing panorama {}", output.display()))?;
    } else {
        staged
            .persist_noclobber(output)
            .map_err(|error| error.error)
            .with_context(|| format!("publishing panorama {}", output.display()))?;
    }
    Ok(())
}

fn validate_sources(sources: &[PathBuf]) -> Result<()> {
    if sources.len() < 2 {
        bail!("panorama requires at least two source images");
    }
    for source in sources {
        if !source.is_file() {
            bail!("panorama source does not exist: {}", source.display());
        }
        if !is_supported_input_file(source, InputFileFilter::All) {
            bail!("unsupported panorama source: {}", source.display());
        }
    }
    Ok(())
}

fn validate_tiff_output(output: &Path, overwrite: bool) -> Result<()> {
    let tiff = output
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("tif") || extension.eq_ignore_ascii_case("tiff")
        });
    if !tiff {
        bail!(
            "panorama output must use .tif or .tiff: {}",
            output.display()
        );
    }
    if output.exists() && !overwrite {
        bail!(
            "panorama output already exists (use --overwrite to replace it): {}",
            output.display()
        );
    }
    Ok(())
}

fn emit(
    progress: Option<&PanoramaProgressSink>,
    stage: &str,
    completed: usize,
    total: usize,
    current: Option<String>,
) {
    if let Some(progress) = progress {
        progress(PanoramaProgress {
            stage: stage.to_string(),
            completed,
            total,
            current,
        });
    }
}

#[derive(Clone, Copy)]
enum PreparationTier {
    Preview,
    Full,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_requires_tiff_and_does_not_overwrite_by_default() {
        let temp = tempfile::tempdir().unwrap();
        assert!(validate_tiff_output(&temp.path().join("pano.jpg"), false).is_err());
        let output = temp.path().join("pano.tif");
        fs::write(&output, b"existing").unwrap();
        assert!(validate_tiff_output(&output, false).is_err());
        validate_tiff_output(&output, true).unwrap();
    }

    #[test]
    fn source_validation_accepts_mixed_supported_inputs() {
        let temp = tempfile::tempdir().unwrap();
        let raw = temp.path().join("a.NEF");
        let tiff = temp.path().join("b.TIFF");
        fs::write(&raw, b"raw").unwrap();
        fs::write(&tiff, b"tiff").unwrap();
        validate_sources(&[raw, tiff]).unwrap();
    }
}
