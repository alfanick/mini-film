use std::{fs, path::Path, process::Command};

use anyhow::{Context, Result, bail};
use tempfile::Builder;

use super::PanoramaConfig;
use crate::app::{
    apply::{RawTherapeeProfileOptions, rawtherapee_profiles_for_input},
    dcp::resolve_dcp_profile,
    export::add_convert_thread_limit_with_count,
    pp3::write_rawtherapee_disable_sharpening_profile,
    profile::neutral_profile,
    raw::run_raw_develop,
    retouch::{BwFilter, RetouchWhiteBalance},
    util::{is_heic_input_file, is_raw_input_file},
};

pub(crate) const PANORAMA_PREVIEW_LONG_EDGE: u32 = 2048;

pub(crate) fn prepare_preview_source(
    config: &PanoramaConfig,
    source: &Path,
    output: &Path,
) -> Result<()> {
    if output.is_file() {
        return Ok(());
    }
    let parent = output
        .parent()
        .with_context(|| format!("panorama preview has no parent: {}", output.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let temp = Builder::new()
        .prefix(".mini-film-panorama-preview-")
        .suffix(".jpg")
        .tempfile_in(parent)
        .with_context(|| format!("creating panorama preview in {}", parent.display()))?
        .into_temp_path();
    fs::remove_file(&temp).with_context(|| format!("preparing {}", temp.display()))?;

    let extracted = if is_raw_input_file(source) {
        Some(extract_raw_preview(source, parent)?)
    } else {
        None
    };
    let conversion_input = extracted.as_deref().unwrap_or(source);
    let mut command = Command::new(&config.convert);
    add_convert_thread_limit_with_count(&mut command, &config.convert, config.jobs);
    let result = command
        .arg("-define")
        .arg(format!(
            "jpeg:size={PANORAMA_PREVIEW_LONG_EDGE}x{PANORAMA_PREVIEW_LONG_EDGE}"
        ))
        .arg(conversion_input)
        .arg("-auto-orient")
        .arg("-filter")
        .arg("Triangle")
        .arg("-resize")
        .arg(format!(
            "{PANORAMA_PREVIEW_LONG_EDGE}x{PANORAMA_PREVIEW_LONG_EDGE}>"
        ))
        .arg("-interlace")
        .arg("Line")
        .arg("-depth")
        .arg("8")
        .arg("-sampling-factor")
        .arg("2x2,1x1,1x1")
        .arg("-quality")
        .arg("85")
        .arg(&temp)
        .output()
        .with_context(|| {
            format!(
                "preparing panorama preview with {}",
                config.convert.display()
            )
        })?;
    if !result.status.success() {
        bail!(
            "panorama preview conversion failed with status {}\nstderr:\n{}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        );
    }
    copy_alignment_metadata(source, &temp)?;
    temp.persist(output)
        .map_err(|error| error.error)
        .with_context(|| format!("publishing panorama preview {}", output.display()))?;
    Ok(())
}

pub(crate) fn prepare_full_source(
    config: &PanoramaConfig,
    source: &Path,
    output: &Path,
) -> Result<()> {
    if output.is_file() {
        return Ok(());
    }
    let parent = output
        .parent()
        .with_context(|| format!("panorama TIFF has no parent: {}", output.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let source_work = parent.join(format!(
        ".{}-work",
        output
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("source")
    ));
    fs::create_dir_all(&source_work)
        .with_context(|| format!("creating {}", source_work.display()))?;

    let converted_heic = if is_heic_input_file(source) {
        let prepared = source_work.join("heic-input.tif");
        if !prepared.is_file() {
            convert_to_tiff(config, source, &prepared)?;
            copy_alignment_metadata(source, &prepared)?;
        }
        Some(prepared)
    } else {
        None
    };
    let prepared_source = match converted_heic.as_deref() {
        Some(develop_input) => crate::app::dng::PreparedRawSource::unchanged(develop_input),
        None => config.dng_fallback.prepare_known(source)?,
    };
    let develop_input = prepared_source.active();
    let dcp_profile = is_raw_input_file(develop_input)
        .then(|| resolve_dcp_profile(develop_input, &config.dng_fallback))
        .flatten();
    let neutral = neutral_profile();
    let mut profiles = rawtherapee_profiles_for_input(
        RawTherapeeProfileOptions {
            input: develop_input,
            retouch: None,
            retouch_white_balance: RetouchWhiteBalance::default(),
            bw_filter: BwFilter::None,
            color_noise_iso_threshold: config.color_noise_iso_threshold,
            lens_corrections: config.lens_corrections,
            dcp_profile: dcp_profile.as_ref(),
        },
        &neutral,
        &source_work,
    )?;
    let color_profile = source_work.join("panorama-color.pp3");
    fs::write(&color_profile, panorama_color_profile_text())
        .with_context(|| format!("writing {}", color_profile.display()))?;
    profiles.push(color_profile);
    profiles.push(write_rawtherapee_disable_sharpening_profile(
        &source_work.join("panorama-no-sharpening.pp3"),
    )?);

    let temp = Builder::new()
        .prefix(".mini-film-panorama-source-")
        .suffix(".tif")
        .tempfile_in(parent)
        .with_context(|| format!("creating panorama source in {}", parent.display()))?
        .into_temp_path();
    fs::remove_file(&temp).with_context(|| format!("preparing {}", temp.display()))?;
    let outcome = run_raw_develop(
        &config.rawtherapee,
        &profiles,
        prepared_source,
        &temp,
        is_raw_input_file(source)
            .then_some(config.lcp_root.as_deref())
            .flatten(),
        true,
        &config.dng_fallback,
    )?;
    copy_alignment_metadata(outcome.source.active(), &temp)?;
    temp.persist(output)
        .map_err(|error| error.error)
        .with_context(|| format!("publishing panorama source {}", output.display()))?;
    config
        .dng_fallback
        .finish_successful_development(&outcome.source)?;
    let _ = fs::remove_dir_all(source_work);
    Ok(())
}

pub(crate) fn copy_panorama_result_metadata(source: &Path, output: &Path) -> Result<()> {
    let status = Command::new("exiftool")
        .args(["-q", "-q", "-overwrite_original", "-TagsFromFile"])
        .arg(source)
        .args([
            "-Make",
            "-Model",
            "-LensModel",
            "-LensID",
            "-FocalLength",
            "-FocalLengthIn35mmFormat",
            "-DateTimeOriginal",
            "-SubSecTimeOriginal",
            "-GPS:all",
        ])
        .arg("-Orientation#=1")
        .arg(format!("-Software=mini-film {}", env!("CARGO_PKG_VERSION")))
        .arg(output)
        .status()
        .with_context(|| format!("copying panorama metadata to {}", output.display()))?;
    if !status.success() {
        bail!("exiftool failed while writing panorama result metadata");
    }
    Ok(())
}

fn convert_to_tiff(config: &PanoramaConfig, source: &Path, output: &Path) -> Result<()> {
    let mut command = Command::new(&config.convert);
    add_convert_thread_limit_with_count(&mut command, &config.convert, config.jobs);
    let result = command
        .arg(source)
        .arg("-auto-orient")
        .arg("-depth")
        .arg("16")
        .arg("-compress")
        .arg("Zip")
        .arg(output)
        .output()
        .with_context(|| format!("converting {} to TIFF", source.display()))?;
    if !result.status.success() {
        bail!(
            "TIFF preparation failed with status {}\nstderr:\n{}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        );
    }
    Ok(())
}

fn extract_raw_preview(raw: &Path, parent: &Path) -> Result<tempfile::TempPath> {
    for tag in ["PreviewImage", "JpgFromRaw", "OtherImage", "ThumbnailImage"] {
        let result = Command::new("exiftool")
            .arg("-b")
            .arg(format!("-{tag}"))
            .arg(raw)
            .output()
            .with_context(|| format!("extracting {tag} from {}", raw.display()))?;
        if !result.status.success() || !looks_like_jpeg(&result.stdout) {
            continue;
        }
        let preview = Builder::new()
            .prefix(".mini-film-panorama-embedded-")
            .suffix(".jpg")
            .tempfile_in(parent)
            .with_context(|| format!("creating embedded preview in {}", parent.display()))?
            .into_temp_path();
        fs::write(&preview, result.stdout)
            .with_context(|| format!("writing {}", preview.display()))?;
        copy_source_orientation(raw, &preview)?;
        return Ok(preview);
    }
    bail!("no embedded JPEG preview found in {}", raw.display())
}

fn copy_alignment_metadata(source: &Path, output: &Path) -> Result<()> {
    let mut command = Command::new("exiftool");
    command
        .args(["-q", "-q", "-overwrite_original", "-TagsFromFile"])
        .arg(source)
        .args([
            "-Make",
            "-Model",
            "-LensModel",
            "-LensID",
            "-FocalLength",
            "-FocalLengthIn35mmFormat",
            "-ScaleFactor35efl",
            "-DateTimeOriginal",
            "-SubSecTimeOriginal",
        ]);
    let status = command
        .arg("-Orientation#=1")
        .arg(output)
        .status()
        .with_context(|| format!("copying alignment metadata to {}", output.display()))?;
    if !status.success() {
        bail!("exiftool failed while copying panorama alignment metadata");
    }
    Ok(())
}

fn copy_source_orientation(source: &Path, output: &Path) -> Result<()> {
    let status = Command::new("exiftool")
        .args(["-q", "-q", "-overwrite_original", "-TagsFromFile"])
        .arg(source)
        .arg("-Orientation")
        .arg(output)
        .status()
        .with_context(|| format!("copying source orientation to {}", output.display()))?;
    if !status.success() {
        bail!("exiftool failed while copying panorama source orientation");
    }
    Ok(())
}

fn panorama_color_profile_text() -> &'static str {
    "[Color Management]\n\
WorkingProfile=ProPhoto\n\
WorkingTRC=none\n\
OutputProfile=RTv4_sRGB\n\
OutputProfileIntent=Relative\n\
OutputBPC=true\n"
}

fn looks_like_jpeg(bytes: &[u8]) -> bool {
    bytes.len() > 3 && bytes[0] == 0xff && bytes[1] == 0xd8 && bytes[2] == 0xff
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panorama_color_profile_is_explicitly_srgb() {
        let profile = panorama_color_profile_text();
        assert!(profile.contains("WorkingProfile=ProPhoto"));
        assert!(profile.contains("OutputProfile=RTv4_sRGB"));
        assert!(profile.contains("OutputBPC=true"));
    }
}
