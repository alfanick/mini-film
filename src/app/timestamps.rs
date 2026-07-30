use std::{
    collections::HashMap,
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use exif::{Reader, Tag};
use filetime::{FileTime, set_file_atime, set_file_mtime};
use mini_film::{GrainEngine, GrainSettings, ProfileAdjustments, SharpeningSettings, ToneCurves};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::profile::ResolvedProfileMetadata;

pub(crate) struct OutputEditMetadata<'a> {
    pub(crate) comment: Option<&'a str>,
    pub(crate) profile: &'a ResolvedProfileMetadata,
    pub(crate) profile_sharpening_applied: bool,
    pub(crate) grain: GrainSettings,
    pub(crate) grain_seed: Option<u64>,
    pub(crate) grain_engine: Option<GrainEngine>,
    pub(crate) normalize_grain_mpix: Option<f64>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct GalleryFocusRegion {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
    #[serde(default)]
    pub(crate) primary: bool,
}

impl GalleryFocusRegion {
    fn normalized(self) -> Option<Self> {
        if !self.x.is_finite()
            || !self.y.is_finite()
            || !self.width.is_finite()
            || !self.height.is_finite()
            || self.width <= 0.0
            || self.height <= 0.0
        {
            return None;
        }
        let left = self.x.clamp(0.0, 1.0);
        let top = self.y.clamp(0.0, 1.0);
        let right = (self.x + self.width).clamp(0.0, 1.0);
        let bottom = (self.y + self.height).clamp(0.0, 1.0);
        (right > left && bottom > top).then_some(Self {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
            primary: self.primary,
        })
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub(crate) struct GalleryExifData {
    #[serde(default)]
    pub(crate) capture_timestamp: Option<i64>,
    #[serde(default)]
    pub(crate) rating: Option<u8>,
    #[serde(default)]
    pub(crate) file_size_bytes: Option<u64>,
    #[serde(default)]
    pub(crate) image_width: Option<u32>,
    #[serde(default)]
    pub(crate) image_height: Option<u32>,
    #[serde(default)]
    pub(crate) focus_frame_width: Option<u32>,
    #[serde(default)]
    pub(crate) focus_frame_height: Option<u32>,
    #[serde(default)]
    pub(crate) focus_regions: Vec<GalleryFocusRegion>,
    pub(crate) focal_length: Option<String>,
    pub(crate) aperture: Option<String>,
    pub(crate) shutter_speed: Option<String>,
    pub(crate) iso: Option<String>,
    #[serde(default)]
    pub(crate) auto_iso: Option<bool>,
    #[serde(default)]
    pub(crate) iso_auto_hi_limit: Option<String>,
    #[serde(default)]
    pub(crate) white_balance_mode: Option<String>,
    #[serde(default)]
    pub(crate) white_balance_temperature: Option<u32>,
    #[serde(default)]
    pub(crate) white_balance_offset: Option<i32>,
    pub(crate) camera_model: Option<String>,
    #[serde(default)]
    pub(crate) shutter_count: Option<u64>,
    #[serde(default)]
    pub(crate) shutter_mode: Option<String>,
    #[serde(default)]
    pub(crate) silent_photography: Option<bool>,
    #[serde(default)]
    pub(crate) release_mode: Option<String>,
    pub(crate) lens_model: Option<String>,
    pub(crate) shooting_mode: Option<String>,
    pub(crate) exposure_compensation: Option<String>,
    pub(crate) flash: Option<String>,
    #[serde(default)]
    pub(crate) active_d_lighting: Option<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) note: Option<String>,
}

impl GalleryExifData {
    pub(crate) fn is_empty(&self) -> bool {
        self.capture_timestamp.is_none()
            && self.rating.is_none()
            && self.focal_length.is_none()
            && self.aperture.is_none()
            && self.shutter_speed.is_none()
            && self.iso.is_none()
            && self.auto_iso.is_none()
            && self.iso_auto_hi_limit.is_none()
            && self.white_balance_mode.is_none()
            && self.white_balance_temperature.is_none()
            && self.white_balance_offset.is_none()
            && self.camera_model.is_none()
            && self.focus_regions.is_empty()
            && self.shutter_count.is_none()
            && self.shutter_mode.is_none()
            && self.silent_photography.is_none()
            && self.release_mode.is_none()
            && self.lens_model.is_none()
            && self.shooting_mode.is_none()
            && self.exposure_compensation.is_none()
            && self.flash.is_none()
            && self.active_d_lighting.is_none()
            && self.tags.is_empty()
            && self.note.is_none()
    }

    pub(crate) fn sanitize_text_fields(&mut self) {
        clean_optional_exif_text(&mut self.focal_length);
        clean_optional_exif_text(&mut self.aperture);
        clean_optional_exif_text(&mut self.shutter_speed);
        clean_optional_exif_text(&mut self.iso);
        clean_optional_exif_text(&mut self.iso_auto_hi_limit);
        clean_optional_exif_text(&mut self.white_balance_mode);
        clean_optional_exif_text(&mut self.camera_model);
        clean_optional_exif_text(&mut self.shutter_mode);
        clean_optional_exif_text(&mut self.release_mode);
        clean_optional_exif_text(&mut self.lens_model);
        clean_optional_exif_text(&mut self.shooting_mode);
        clean_optional_exif_text(&mut self.exposure_compensation);
        clean_optional_exif_text(&mut self.flash);
        clean_optional_exif_text(&mut self.active_d_lighting);
        clean_optional_exif_text(&mut self.note);
        self.tags = normalize_gallery_tags(std::mem::take(&mut self.tags));
        self.focus_regions = std::mem::take(&mut self.focus_regions)
            .into_iter()
            .filter_map(GalleryFocusRegion::normalized)
            .collect();
        if self.focus_regions.is_empty()
            || self.focus_frame_width.is_none()
            || self.focus_frame_height.is_none()
        {
            self.focus_frame_width = None;
            self.focus_frame_height = None;
        }
    }
}

pub(crate) fn sync_output_timestamps_from_exif(raw: &Path, output: &Path) -> Result<bool> {
    let capture_time = extract_capture_time(raw)?;
    let timestamp = match capture_time {
        Some(timestamp) => timestamp,
        None => match fs::metadata(raw)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
        {
            Some(timestamp) => timestamp,
            None => return Ok(false),
        },
    };

    set_file_times(output, &timestamp)?;
    Ok(true)
}

pub(crate) fn sync_output_metadata_from_raw_with_color_profile(
    raw: &Path,
    output: &Path,
    edit: OutputEditMetadata<'_>,
    color_profile_source: Option<&Path>,
) -> Result<()> {
    let is_tiff = matches!(
        output.extension().and_then(|ext| ext.to_str()),
        Some(ext) if ext.eq_ignore_ascii_case("tiff") || ext.eq_ignore_ascii_case("tif")
    );

    let status = run_exiftool_copy_all(raw, output, &edit)
        .with_context(|| format!("running exiftool on {}", output.display()))?;

    if status.success() {
        restore_output_color_profile(color_profile_source, output)?;
        return Ok(());
    }

    if !is_tiff {
        return Err(anyhow::anyhow!(
            "exiftool failed with status {status} while syncing metadata"
        ));
    }

    if !run_exiftool_fallback(raw, output, &edit)?.success()
        && !run_exiftool_minimal(output, &edit)?.success()
    {
        // TIFF metadata writing is known to be tool- and tag-dependent.
        // Never hard-fail batch/apply here because this step is cosmetic versus
        // critical output validity; capture metadata best-effort only.
        return Ok(());
    }

    restore_output_color_profile(color_profile_source, output)?;
    Ok(())
}

pub(crate) fn sync_output_metadata_from_image_with_color_profile(
    input: &Path,
    output: &Path,
    comment: Option<&str>,
    color_profile_source: Option<&Path>,
) -> Result<()> {
    let is_tiff = matches!(
        output.extension().and_then(|ext| ext.to_str()),
        Some(ext) if ext.eq_ignore_ascii_case("tiff") || ext.eq_ignore_ascii_case("tif")
    );

    let status = run_exiftool_image_copy_all(input, output, comment)
        .with_context(|| format!("running exiftool on {}", output.display()))?;
    if status.success() {
        restore_output_color_profile(color_profile_source, output)?;
        return Ok(());
    }

    if !is_tiff {
        return Err(anyhow::anyhow!(
            "exiftool failed with status {status} while syncing metadata"
        ));
    }

    if !run_exiftool_image_fallback(input, output, comment)?.success()
        && !run_exiftool_image_minimal(output, comment)?.success()
    {
        return Ok(());
    }

    restore_output_color_profile(color_profile_source, output)?;
    Ok(())
}

fn restore_output_color_profile(color_profile_source: Option<&Path>, output: &Path) -> Result<()> {
    let Some(color_profile_source) = color_profile_source else {
        return Ok(());
    };
    let is_jpeg = matches!(
        output.extension().and_then(|ext| ext.to_str()),
        Some(ext) if ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg")
    );
    if !is_jpeg {
        return Ok(());
    }

    let mut command = Command::new("exiftool");
    command
        .arg("-q")
        .arg("-quiet")
        .arg("-overwrite_original")
        .arg("-TagsFromFile")
        .arg(color_profile_source)
        .arg("-icc_profile")
        .arg(output);
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let status = command.status().with_context(|| {
        format!(
            "copying color profile from {} to {}",
            color_profile_source.display(),
            output.display()
        )
    })?;
    if !status.success() {
        bail!(
            "exiftool failed with status {status} while copying color profile from {} to {}",
            color_profile_source.display(),
            output.display()
        );
    }

    Ok(())
}

fn run_exiftool_copy_all(
    raw: &Path,
    output: &Path,
    edit: &OutputEditMetadata<'_>,
) -> Result<std::process::ExitStatus> {
    let mut command = Command::new("exiftool");
    command
        .arg("-q")
        .arg("-quiet")
        .arg("-overwrite_original")
        .arg("-m")
        .arg("-TagsFromFile")
        .arg(raw)
        .arg("-all:all")
        .arg("-icc_profile")
        .arg("-Orientation#=1");
    add_edit_metadata_args(&mut command, raw, edit);
    command.arg(output);
    command.stdout(Stdio::null()).stderr(Stdio::null());

    Ok(command.status()?)
}

fn run_exiftool_fallback(
    raw: &Path,
    output: &Path,
    edit: &OutputEditMetadata<'_>,
) -> Result<std::process::ExitStatus> {
    // Some TIFF writers reject the full `-all:all` copy even when the same tags are
    // acceptable on JPEG; keep a strict-but-smaller set for a second attempt.
    let mut command = Command::new("exiftool");
    command
        .arg("-q")
        .arg("-quiet")
        .arg("-overwrite_original")
        .arg("-m")
        .arg("-TagsFromFile")
        .arg(raw)
        .arg("-exif:all")
        .arg("-xmp:all")
        .arg("-icc_profile")
        .arg("-Orientation#=1");
    add_edit_metadata_args(&mut command, raw, edit);
    command.arg(output);
    command.stdout(Stdio::null()).stderr(Stdio::null());

    Ok(command.status()?)
}

fn run_exiftool_minimal(
    output: &Path,
    edit: &OutputEditMetadata<'_>,
) -> Result<std::process::ExitStatus> {
    // Final TIFF fallback: write only a tiny amount of metadata that is
    // widely supported (best-effort so failures still only degrade metadata).
    let mut command = Command::new("exiftool");
    command
        .arg("-q")
        .arg("-quiet")
        .arg("-overwrite_original")
        .arg("-m");
    add_edit_metadata_args(&mut command, Path::new(""), edit);
    command.arg(output);
    command.stdout(Stdio::null()).stderr(Stdio::null());

    Ok(command.status()?)
}

fn run_exiftool_image_copy_all(
    input: &Path,
    output: &Path,
    comment: Option<&str>,
) -> Result<std::process::ExitStatus> {
    let mut command = Command::new("exiftool");
    command
        .arg("-q")
        .arg("-quiet")
        .arg("-overwrite_original")
        .arg("-m")
        .arg("-TagsFromFile")
        .arg(input)
        .arg("-all:all")
        .arg("-icc_profile")
        .arg("-Orientation#=1");
    add_basic_output_metadata_args(&mut command, comment);
    command.arg(output);
    command.stdout(Stdio::null()).stderr(Stdio::null());

    Ok(command.status()?)
}

fn run_exiftool_image_fallback(
    input: &Path,
    output: &Path,
    comment: Option<&str>,
) -> Result<std::process::ExitStatus> {
    let mut command = Command::new("exiftool");
    command
        .arg("-q")
        .arg("-quiet")
        .arg("-overwrite_original")
        .arg("-m")
        .arg("-TagsFromFile")
        .arg(input)
        .arg("-exif:all")
        .arg("-xmp:all")
        .arg("-icc_profile")
        .arg("-Orientation#=1");
    add_basic_output_metadata_args(&mut command, comment);
    command.arg(output);
    command.stdout(Stdio::null()).stderr(Stdio::null());

    Ok(command.status()?)
}

fn run_exiftool_image_minimal(
    output: &Path,
    comment: Option<&str>,
) -> Result<std::process::ExitStatus> {
    let mut command = Command::new("exiftool");
    command
        .arg("-q")
        .arg("-quiet")
        .arg("-overwrite_original")
        .arg("-m");
    add_basic_output_metadata_args(&mut command, comment);
    command.arg(output);
    command.stdout(Stdio::null()).stderr(Stdio::null());

    Ok(command.status()?)
}

fn add_basic_output_metadata_args(command: &mut Command, comment: Option<&str>) {
    let agent = format!("mini-film {}", env!("CARGO_PKG_VERSION"));
    let timestamp = exiftool_timestamp(Local::now());

    if let Some(comment) = comment {
        command.arg(format!("-Comment={comment}"));
        command.arg(format!("-UserComment={comment}"));
    }

    command
        .arg(format!("-XMP-xmp:CreatorTool={agent}"))
        .arg(format!("-XMP-xmp:MetadataDate={timestamp}"))
        .arg(format!("-XMP-xmp:ModifyDate={timestamp}"));
}

fn add_edit_metadata_args(command: &mut Command, raw: &Path, edit: &OutputEditMetadata<'_>) {
    let agent = format!("mini-film {}", env!("CARGO_PKG_VERSION"));
    let timestamp = exiftool_timestamp(Local::now());
    let profile = edit.profile;

    if let Some(comment) = edit.comment {
        command.arg(format!("-Comment={comment}"));
        command.arg(format!("-UserComment={comment}"));
    }

    command
        .arg(format!("-XMP-xmp:CreatorTool={agent}"))
        .arg(format!("-XMP-xmp:MetadataDate={timestamp}"))
        .arg(format!("-XMP-xmp:ModifyDate={timestamp}"))
        .arg("-XMP-crs:HasSettings=True")
        .arg("-XMP-crs:AlreadyApplied=True")
        .arg(format!("-XMP-crs:Converter={agent}"))
        .arg("-XMP-crs:ProcessVersion=6.7")
        .arg(format!(
            "-XMP-crs:CameraProfile={}",
            metadata_text(&profile.profile_name)
        ))
        .arg(format!(
            "-XMP-crs:Name={}",
            metadata_text(&profile.profile_name)
        ))
        .arg("-XMP-xmpMM:HistoryAction=converted")
        .arg(format!("-XMP-xmpMM:HistorySoftwareAgent={agent}"))
        .arg(format!("-XMP-xmpMM:HistoryWhen={timestamp}"))
        .arg(format!(
            "-XMP-xmpMM:HistoryParameters={}",
            metadata_text(&history_parameters(raw, edit))
        ));

    if let Some(file_name) = raw.file_name().and_then(|name| name.to_str()) {
        command.arg(format!("-XMP-crs:RawFileName={}", metadata_text(file_name)));
    }
    if let Some(uuid) = &profile.profile_uuid {
        command.arg(format!("-XMP-crs:UUID={}", metadata_text(uuid)));
    }

    if profile.has_camera_raw_settings {
        let adjustments = combined_adjustments(profile);
        add_profile_adjustment_args(command, &adjustments);
        if edit.profile_sharpening_applied {
            add_sharpening_args(command, combined_sharpening(profile));
        }
    }

    command
        .arg(format!("-XMP-crs:GrainAmount={}", edit.grain.amount))
        .arg(format!("-XMP-crs:GrainSize={}", edit.grain.size))
        .arg(format!("-XMP-crs:GrainFrequency={}", edit.grain.frequency));
}

fn exiftool_timestamp(timestamp: DateTime<Local>) -> String {
    timestamp.format("%Y:%m:%d %H:%M:%S%:z").to_string()
}

fn metadata_text(value: &str) -> String {
    value.replace(['\n', '\r'], " ").trim().to_string()
}

fn history_parameters(raw: &Path, edit: &OutputEditMetadata<'_>) -> String {
    let profile = edit.profile;
    let mut parts = vec![format!("profile={}", profile.profile_name)];
    if let Some(source) = &profile.source_profile_name {
        parts.push(format!("source_profile={source}"));
    }
    if let Some(uuid) = &profile.source_profile_uuid {
        parts.push(format!("source_uuid={uuid}"));
    }
    if let Some(look) = &profile.look_name {
        parts.push(format!("look={look}"));
    }
    if let Some(uuid) = &profile.look_uuid {
        parts.push(format!("look_uuid={uuid}"));
    }
    if let Some(hald) = &profile.hald_path {
        parts.push(format!("hald={}", hald.display()));
    }
    if let Some(pp3) = &profile.pp3_path {
        parts.push(format!("pp3={}", pp3.display()));
    }
    if edit.grain.is_enabled() {
        parts.push(format!(
            "grain={},{},{}",
            edit.grain.amount, edit.grain.size, edit.grain.frequency
        ));
        if let Some(engine) = edit.grain_engine {
            parts.push(format!("grain_engine={engine}"));
        }
        parts.push(edit.normalize_grain_mpix.map_or_else(
            || "grain_normalize_mpix=off".to_string(),
            |mpix| format!("grain_normalize_mpix={mpix}"),
        ));
    } else {
        parts.push("grain=off".to_string());
    }
    if let Some(seed) = edit.grain_seed {
        parts.push(format!("grain_seed={seed}"));
    }
    if let Some(name) = raw.file_name().and_then(|name| name.to_str()) {
        parts.push(format!("raw={name}"));
    }
    parts.join("; ")
}

fn combined_adjustments(profile: &ResolvedProfileMetadata) -> ProfileAdjustments {
    let mut combined = profile.source_adjustments.clone();
    add_adjustments(&mut combined, &profile.emulation_adjustments);
    combined
}

fn add_adjustments(target: &mut ProfileAdjustments, source: &ProfileAdjustments) {
    target.exposure += source.exposure;
    target.contrast += source.contrast;
    target.highlights += source.highlights;
    target.shadows += source.shadows;
    target.whites += source.whites;
    target.blacks += source.blacks;
    target.saturation += source.saturation;
    target.vibrance += source.vibrance;
    target.clarity += source.clarity;
    target.parametric.shadows += source.parametric.shadows;
    target.parametric.darks += source.parametric.darks;
    target.parametric.lights += source.parametric.lights;
    target.parametric.highlights += source.parametric.highlights;
    target.parametric.shadow_split = source.parametric.shadow_split;
    target.parametric.midtone_split = source.parametric.midtone_split;
    target.parametric.highlight_split = source.parametric.highlight_split;
    add_array(&mut target.hsl.hue, &source.hsl.hue);
    add_array(&mut target.hsl.saturation, &source.hsl.saturation);
    add_array(&mut target.hsl.luminance, &source.hsl.luminance);
    target.calibration.red_hue += source.calibration.red_hue;
    target.calibration.red_saturation += source.calibration.red_saturation;
    target.calibration.green_hue += source.calibration.green_hue;
    target.calibration.green_saturation += source.calibration.green_saturation;
    target.calibration.blue_hue += source.calibration.blue_hue;
    target.calibration.blue_saturation += source.calibration.blue_saturation;
    append_curves(&mut target.tone_curve, &source.tone_curve);
}

fn add_array<const N: usize>(target: &mut [f32; N], source: &[f32; N]) {
    for (target, source) in target.iter_mut().zip(source) {
        *target += *source;
    }
}

fn append_curves(target: &mut ToneCurves, source: &ToneCurves) {
    if !curve_is_identity(&source.composite) {
        target.composite = source.composite.clone();
    }
    if !curve_is_identity(&source.red) {
        target.red = source.red.clone();
    }
    if !curve_is_identity(&source.green) {
        target.green = source.green.clone();
    }
    if !curve_is_identity(&source.blue) {
        target.blue = source.blue.clone();
    }
}

fn combined_sharpening(profile: &ResolvedProfileMetadata) -> SharpeningSettings {
    if profile.emulation_sharpening.present {
        profile.emulation_sharpening
    } else {
        profile.source_sharpening
    }
}

fn add_profile_adjustment_args(command: &mut Command, adjustments: &ProfileAdjustments) {
    command
        .arg(format!(
            "-XMP-crs:Exposure2012={}",
            fmt_real(adjustments.exposure)
        ))
        .arg(format!(
            "-XMP-crs:Contrast2012={}",
            fmt_integer(adjustments.contrast)
        ))
        .arg(format!(
            "-XMP-crs:Highlights2012={}",
            fmt_integer(adjustments.highlights)
        ))
        .arg(format!(
            "-XMP-crs:Shadows2012={}",
            fmt_integer(adjustments.shadows)
        ))
        .arg(format!(
            "-XMP-crs:Whites2012={}",
            fmt_integer(adjustments.whites)
        ))
        .arg(format!(
            "-XMP-crs:Blacks2012={}",
            fmt_integer(adjustments.blacks)
        ))
        .arg(format!(
            "-XMP-crs:Saturation={}",
            fmt_integer(adjustments.saturation)
        ))
        .arg(format!(
            "-XMP-crs:Vibrance={}",
            fmt_integer(adjustments.vibrance)
        ))
        .arg(format!(
            "-XMP-crs:Clarity2012={}",
            fmt_integer(adjustments.clarity)
        ))
        .arg(format!(
            "-XMP-crs:ParametricShadows={}",
            fmt_integer(adjustments.parametric.shadows)
        ))
        .arg(format!(
            "-XMP-crs:ParametricDarks={}",
            fmt_integer(adjustments.parametric.darks)
        ))
        .arg(format!(
            "-XMP-crs:ParametricLights={}",
            fmt_integer(adjustments.parametric.lights)
        ))
        .arg(format!(
            "-XMP-crs:ParametricHighlights={}",
            fmt_integer(adjustments.parametric.highlights)
        ))
        .arg(format!(
            "-XMP-crs:ParametricShadowSplit={}",
            fmt_integer(adjustments.parametric.shadow_split)
        ))
        .arg(format!(
            "-XMP-crs:ParametricMidtoneSplit={}",
            fmt_integer(adjustments.parametric.midtone_split)
        ))
        .arg(format!(
            "-XMP-crs:ParametricHighlightSplit={}",
            fmt_integer(adjustments.parametric.highlight_split)
        ))
        .arg(format!(
            "-XMP-crs:RedHue={}",
            fmt_integer(adjustments.calibration.red_hue)
        ))
        .arg(format!(
            "-XMP-crs:RedSaturation={}",
            fmt_integer(adjustments.calibration.red_saturation)
        ))
        .arg(format!(
            "-XMP-crs:GreenHue={}",
            fmt_integer(adjustments.calibration.green_hue)
        ))
        .arg(format!(
            "-XMP-crs:GreenSaturation={}",
            fmt_integer(adjustments.calibration.green_saturation)
        ))
        .arg(format!(
            "-XMP-crs:BlueHue={}",
            fmt_integer(adjustments.calibration.blue_hue)
        ))
        .arg(format!(
            "-XMP-crs:BlueSaturation={}",
            fmt_integer(adjustments.calibration.blue_saturation)
        ));

    add_hsl_args(command, "HueAdjustment", &adjustments.hsl.hue);
    add_hsl_args(command, "SaturationAdjustment", &adjustments.hsl.saturation);
    add_hsl_args(command, "LuminanceAdjustment", &adjustments.hsl.luminance);
    add_curve_args(
        command,
        "ToneCurvePV2012",
        &adjustments.tone_curve.composite,
    );
    add_curve_args(command, "ToneCurvePV2012Red", &adjustments.tone_curve.red);
    add_curve_args(
        command,
        "ToneCurvePV2012Green",
        &adjustments.tone_curve.green,
    );
    add_curve_args(command, "ToneCurvePV2012Blue", &adjustments.tone_curve.blue);
    if !all_curves_identity(&adjustments.tone_curve) {
        command.arg("-XMP-crs:ToneCurveName2012=Custom");
    }
}

fn add_sharpening_args(command: &mut Command, sharpening: SharpeningSettings) {
    if !sharpening.present {
        return;
    }
    command
        .arg(format!(
            "-XMP-crs:Sharpness={}",
            fmt_integer(sharpening.amount)
        ))
        .arg(format!(
            "-XMP-crs:SharpenRadius={}",
            fmt_real(sharpening.radius)
        ))
        .arg(format!(
            "-XMP-crs:SharpenDetail={}",
            fmt_integer(sharpening.detail)
        ))
        .arg(format!(
            "-XMP-crs:SharpenEdgeMasking={}",
            fmt_integer(sharpening.masking)
        ));
}

fn add_hsl_args(command: &mut Command, prefix: &str, values: &[f32; 8]) {
    let names = [
        "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta",
    ];
    for (name, value) in names.iter().zip(values) {
        command.arg(format!("-XMP-crs:{prefix}{name}={}", fmt_integer(*value)));
    }
}

fn add_curve_args(command: &mut Command, name: &str, points: &[(f32, f32)]) {
    if curve_is_identity(points) {
        return;
    }
    for (index, (x, y)) in points.iter().enumerate() {
        let operator = if index == 0 { "=" } else { "+=" };
        command.arg(format!(
            "-XMP-crs:{name}{operator}{}, {}",
            fmt_integer(*x),
            fmt_integer(*y)
        ));
    }
}

fn all_curves_identity(curves: &ToneCurves) -> bool {
    curve_is_identity(&curves.composite)
        && curve_is_identity(&curves.red)
        && curve_is_identity(&curves.green)
        && curve_is_identity(&curves.blue)
}

fn curve_is_identity(points: &[(f32, f32)]) -> bool {
    points.is_empty() || points.iter().all(|(x, y)| (*x - *y).abs() < f32::EPSILON)
}

fn fmt_integer(value: f32) -> String {
    format!("{:.0}", value.round())
}

fn fmt_real(value: f32) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded == 0.0 {
        "0".to_string()
    } else {
        format!("{rounded:.2}")
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct GalleryExifCacheKey {
    path: PathBuf,
    file_size: u64,
    modified_nanos: Option<u128>,
}

static GALLERY_EXIF_CACHE: OnceLock<
    Mutex<HashMap<PathBuf, (GalleryExifCacheKey, GalleryExifData)>>,
> = OnceLock::new();

pub(crate) fn prefetch_gallery_exif(files: &[PathBuf]) {
    files.par_iter().for_each(|file| {
        let _ = extract_gallery_exif(file);
    });
}

pub(crate) fn extract_gallery_exif(file: &Path) -> Result<GalleryExifData> {
    let cache_key = gallery_exif_cache_key(file);
    if let Some(cached) = cache_key.as_ref().and_then(cached_gallery_exif) {
        return Ok(cached);
    }

    let data = extract_gallery_exif_uncached(file)?;
    if let Some(cache_key) = cache_key {
        cache_gallery_exif(cache_key, data.clone());
    }
    Ok(data)
}

fn extract_gallery_exif_uncached(file: &Path) -> Result<GalleryExifData> {
    let file_size_bytes = fs::metadata(file).ok().map(|metadata| metadata.len());
    let direct_dimensions = crate::util::is_jpeg_input_file(file)
        .then(|| image::image_dimensions(file).ok())
        .flatten();
    let opened = File::open(file).with_context(|| format!("opening file {}", file.display()))?;
    let mut reader = BufReader::new(opened);
    let exif = match Reader::new().read_from_container(&mut reader) {
        Ok(exif) => exif,
        Err(_) => {
            let mut data = GalleryExifData {
                file_size_bytes,
                image_width: direct_dimensions.map(|(width, _)| width),
                image_height: direct_dimensions.map(|(_, height)| height),
                ..GalleryExifData::default()
            };
            if let Some(metadata) = extract_gallery_metadata_with_exiftool(file) {
                data.tags = metadata.tags;
                data.note = metadata.note;
                data.rating = metadata.rating;
                data.exposure_compensation = metadata.exposure_compensation;
                data.flash = metadata.flash;
                data.active_d_lighting = metadata.active_d_lighting;
                data.auto_iso = metadata.auto_iso;
                data.iso_auto_hi_limit = metadata.iso_auto_hi_limit;
                data.white_balance_mode = metadata.white_balance_mode;
                data.white_balance_temperature = metadata.white_balance_temperature;
                data.white_balance_offset = metadata.white_balance_offset;
                data.shutter_count = metadata.shutter_count;
                data.shutter_mode = metadata.shutter_mode;
                data.silent_photography = metadata.silent_photography;
                data.release_mode = metadata.release_mode;
                data.image_width = metadata.image_width.or(data.image_width);
                data.image_height = metadata.image_height.or(data.image_height);
                data.focus_frame_width = metadata.focus_frame_width;
                data.focus_frame_height = metadata.focus_frame_height;
                data.focus_regions = metadata.focus_regions;
            }
            data.sanitize_text_fields();
            return Ok(data);
        }
    };

    let focal_length = exif_field_value(&exif, Tag::FocalLength);
    let aperture = exif_field_value(&exif, Tag::FNumber)
        .or_else(|| exif_field_value(&exif, Tag::MaxApertureValue));
    let shutter_speed = exif_field_value(&exif, Tag::ExposureTime);
    let exif_exposure_compensation = exif_field_value(&exif, Tag::ExposureBiasValue);
    let iso = extract_capture_iso_from_exif(&exif).map(|iso| iso.to_string());

    let camera_model = exif_field_value(&exif, Tag::Model);
    let lens_model = exif_field_value(&exif, Tag::LensModel)
        .or_else(|| exif_field_value(&exif, Tag::LensSpecification));
    let shooting_mode = exif_exposure_program(&exif);
    let mut note = exif_field_value(&exif, Tag::ImageDescription);
    let mut tags = Vec::new();
    let mut rating = None;
    let mut exposure_compensation = exif_exposure_compensation
        .as_ref()
        .and_then(|value| parse_exposure_compensation_text(value));
    let mut flash = extract_firing_flash_details(&exif);
    let mut active_d_lighting = None;
    let mut auto_iso = None;
    let mut iso_auto_hi_limit = None;
    let mut white_balance_mode = None;
    let mut white_balance_temperature = None;
    let mut white_balance_offset = None;
    let mut shutter_count = None;
    let mut shutter_mode = None;
    let mut silent_photography = None;
    let mut release_mode = None;
    let mut image_width = direct_dimensions.map(|(width, _)| width);
    let mut image_height = direct_dimensions.map(|(_, height)| height);
    let mut focus_frame_width = None;
    let mut focus_frame_height = None;
    let mut focus_regions = Vec::new();
    if let Some(metadata) = extract_gallery_metadata_with_exiftool(file) {
        tags = metadata.tags;
        rating = metadata.rating;
        exposure_compensation = exposure_compensation.or(metadata.exposure_compensation);
        active_d_lighting = metadata.active_d_lighting;
        auto_iso = metadata.auto_iso;
        iso_auto_hi_limit = metadata.iso_auto_hi_limit;
        white_balance_mode = metadata.white_balance_mode;
        white_balance_temperature = metadata.white_balance_temperature;
        white_balance_offset = metadata.white_balance_offset;
        shutter_count = metadata.shutter_count;
        shutter_mode = metadata.shutter_mode;
        silent_photography = metadata.silent_photography;
        release_mode = metadata.release_mode;
        image_width = metadata.image_width.or(image_width);
        image_height = metadata.image_height.or(image_height);
        focus_frame_width = metadata.focus_frame_width;
        focus_frame_height = metadata.focus_frame_height;
        focus_regions = metadata.focus_regions;
        if flash.is_none() {
            flash = metadata
                .flash
                .and_then(|value| filter_fired_flash_detail(&value).map(|value| value.to_string()));
        }
        if note.is_none() {
            note = metadata.note;
        }
    }
    let capture_timestamp =
        extract_capture_time_from_exif(&exif).and_then(system_time_to_unix_seconds);

    let mut data = GalleryExifData {
        capture_timestamp,
        rating,
        file_size_bytes,
        image_width,
        image_height,
        focus_frame_width,
        focus_frame_height,
        focus_regions,
        focal_length,
        aperture: aperture.map(format_exif_aperture),
        shutter_speed,
        iso,
        auto_iso,
        iso_auto_hi_limit,
        white_balance_mode,
        white_balance_temperature,
        white_balance_offset,
        camera_model,
        shutter_count,
        shutter_mode,
        silent_photography,
        release_mode,
        lens_model,
        shooting_mode,
        exposure_compensation,
        flash,
        active_d_lighting,
        tags,
        note,
    };
    data.sanitize_text_fields();
    Ok(data)
}

fn gallery_exif_cache_key(file: &Path) -> Option<GalleryExifCacheKey> {
    let metadata = fs::metadata(file).ok()?;
    let modified_nanos = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos());
    Some(GalleryExifCacheKey {
        path: file.to_path_buf(),
        file_size: metadata.len(),
        modified_nanos,
    })
}

fn cached_gallery_exif(cache_key: &GalleryExifCacheKey) -> Option<GalleryExifData> {
    GALLERY_EXIF_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()
        .and_then(|cache| {
            cache
                .get(&cache_key.path)
                .filter(|(cached_key, _)| cached_key == cache_key)
                .map(|(_, data)| data.clone())
        })
}

fn cache_gallery_exif(cache_key: GalleryExifCacheKey, data: GalleryExifData) {
    if let Ok(mut cache) = GALLERY_EXIF_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        cache.insert(cache_key.path.clone(), (cache_key, data));
    }
}

#[derive(Debug, Default)]
struct GalleryMetadata {
    tags: Vec<String>,
    note: Option<String>,
    rating: Option<u8>,
    exposure_compensation: Option<String>,
    flash: Option<String>,
    active_d_lighting: Option<String>,
    auto_iso: Option<bool>,
    iso_auto_hi_limit: Option<String>,
    white_balance_mode: Option<String>,
    white_balance_temperature: Option<u32>,
    white_balance_offset: Option<i32>,
    shutter_count: Option<u64>,
    shutter_mode: Option<String>,
    silent_photography: Option<bool>,
    release_mode: Option<String>,
    image_width: Option<u32>,
    image_height: Option<u32>,
    focus_frame_width: Option<u32>,
    focus_frame_height: Option<u32>,
    focus_regions: Vec<GalleryFocusRegion>,
}

fn extract_gallery_metadata_with_exiftool(file: &Path) -> Option<GalleryMetadata> {
    let output = Command::new("exiftool")
        .arg("-q")
        .arg("-q")
        .arg("-j")
        .arg("-Subject")
        .arg("-Keywords")
        .arg("-Description")
        .arg("-ImageDescription")
        .arg("-Flash")
        .arg("-ExposureBiasValue")
        .arg("-Nikon:ActiveD-Lighting")
        .arg("-Nikon:ShootingMode")
        .arg("-NikonSettings:ISOAutoHiLimit")
        .arg("-Nikon:WhiteBalance")
        .arg("-Nikon:ColorTemperatureAuto#")
        .arg("-Nikon:WhiteBalanceFineTune#")
        .arg("-ShutterCount#")
        .arg("-Nikon:ShutterMode")
        .arg("-Nikon:SilentPhotography#")
        .arg("-Nikon:ReleaseMode")
        .arg("-ImageWidth#")
        .arg("-ImageHeight#")
        .arg("-Orientation#")
        .arg("-Nikon:AFImageWidth#")
        .arg("-Nikon:AFImageHeight#")
        .arg("-Nikon:AFAreaXPosition#")
        .arg("-Nikon:AFAreaYPosition#")
        .arg("-Nikon:AFAreaWidth#")
        .arg("-Nikon:AFAreaHeight#")
        .arg("-Nikon:PrimaryAFPoint")
        .arg("-Nikon:AFPointsUsed")
        .arg("-Rating")
        .arg("-XMP-xmp:Rating")
        .arg("-XMP-nine:Rating")
        .arg("-EXIF:Rating")
        .arg(file)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut values = serde_json::from_slice::<Vec<Value>>(&output.stdout).ok()?;
    let object = values.pop()?;
    let tags = normalize_gallery_tags(
        json_string_values(object.get("Subject"))
            .into_iter()
            .chain(json_string_values(object.get("Keywords")))
            .collect(),
    );
    let note = json_first_string(object.get("Description"))
        .or_else(|| json_first_string(object.get("ImageDescription")))
        .map(clean_exif_display_text)
        .filter(|value| !value.is_empty());
    let flash = json_first_string(object.get("Flash"))
        .map(clean_exif_display_text)
        .filter(|value| !value.is_empty());
    let active_d_lighting = json_first_string(object.get("ActiveD-Lighting"))
        .or_else(|| json_first_string(object.get("Nikon:ActiveD-Lighting")))
        .map(clean_exif_display_text)
        .filter(|value| !value.is_empty());
    let auto_iso = json_first_string(object.get("ShootingMode"))
        .or_else(|| json_first_string(object.get("Nikon:ShootingMode")))
        .map(|value| nikon_shooting_mode_uses_auto_iso(&value));
    let iso_auto_hi_limit = json_first_string(object.get("ISOAutoHiLimit"))
        .or_else(|| json_first_string(object.get("NikonSettings:ISOAutoHiLimit")))
        .map(clean_exif_display_text)
        .filter(|value| !value.is_empty());
    let white_balance_mode = json_first_string(object.get("WhiteBalance"))
        .or_else(|| json_first_string(object.get("Nikon:WhiteBalance")))
        .map(clean_exif_display_text)
        .filter(|value| !value.is_empty());
    let white_balance_temperature = json_u32_value(object.get("ColorTemperatureAuto"))
        .or_else(|| json_u32_value(object.get("Nikon:ColorTemperatureAuto")));
    let white_balance_offset = json_nikon_white_balance_offset(
        object
            .get("WhiteBalanceFineTune")
            .or_else(|| object.get("Nikon:WhiteBalanceFineTune")),
    );
    let shutter_count = json_u64_value(object.get("ShutterCount"))
        .or_else(|| json_u64_value(object.get("Nikon:ShutterCount")));
    let shutter_mode = json_first_string(object.get("ShutterMode"))
        .or_else(|| json_first_string(object.get("Nikon:ShutterMode")))
        .map(clean_exif_display_text)
        .filter(|value| !value.is_empty());
    let silent_photography = json_bool_value(object.get("SilentPhotography"))
        .or_else(|| json_bool_value(object.get("Nikon:SilentPhotography")));
    let release_mode = json_first_string(object.get("ReleaseMode"))
        .or_else(|| json_first_string(object.get("Nikon:ReleaseMode")))
        .map(clean_exif_display_text)
        .filter(|value| !value.is_empty());
    let exposure_compensation = json_exposure_compensation_value(object.get("ExposureBiasValue"))
        .or_else(|| json_exposure_compensation_value(object.get("XMP-exif:ExposureBiasValue")))
        .or_else(|| json_exposure_compensation_value(object.get("XMP:ExposureBiasValue")))
        .or_else(|| json_exposure_compensation_value(object.get("ExposureCompensation")))
        .or_else(|| json_exposure_compensation_value(object.get("XMP:ExposureCompensation")));
    let rating = json_rating_value(object.get("Rating"))
        .or_else(|| json_rating_value(object.get("XMP:Rating")))
        .or_else(|| json_rating_value(object.get("XMP-xmp:Rating")))
        .or_else(|| json_rating_value(object.get("XMP-nine:Rating")))
        .or_else(|| json_rating_value(object.get("EXIF:Rating")));
    let image_width = json_u32_value(object.get("ImageWidth"));
    let image_height = json_u32_value(object.get("ImageHeight"));
    let (focus_frame_width, focus_frame_height, focus_regions) = json_nikon_focus_regions(&object);
    Some(GalleryMetadata {
        tags,
        note,
        rating,
        exposure_compensation,
        flash,
        active_d_lighting,
        auto_iso,
        iso_auto_hi_limit,
        white_balance_mode,
        white_balance_temperature,
        white_balance_offset,
        shutter_count,
        shutter_mode,
        silent_photography,
        release_mode,
        image_width,
        image_height,
        focus_frame_width,
        focus_frame_height,
        focus_regions,
    })
}

fn nikon_shooting_mode_uses_auto_iso(value: &str) -> bool {
    value
        .split(',')
        .any(|mode| mode.trim().eq_ignore_ascii_case("Auto ISO"))
}

fn json_bool_value(value: Option<&Value>) -> Option<bool> {
    match value {
        Some(Value::Bool(value)) => Some(*value),
        Some(Value::Number(value)) => value.as_u64().map(|value| value != 0),
        Some(Value::String(value)) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "on" | "yes" | "enabled" => Some(true),
            "0" | "false" | "off" | "no" | "disabled" => Some(false),
            _ => None,
        },
        Some(Value::Array(values)) => values.iter().find_map(|value| json_bool_value(Some(value))),
        _ => None,
    }
}

fn json_u64_value(value: Option<&Value>) -> Option<u64> {
    match value {
        Some(Value::Number(value)) => value.as_u64(),
        Some(Value::String(value)) => value.trim().parse::<u64>().ok(),
        Some(Value::Array(values)) => values.iter().find_map(|value| json_u64_value(Some(value))),
        _ => None,
    }
}

fn json_u32_value(value: Option<&Value>) -> Option<u32> {
    match value {
        Some(Value::Number(value)) => value.as_u64().and_then(|value| u32::try_from(value).ok()),
        Some(Value::String(value)) => value.trim().parse::<u32>().ok(),
        Some(Value::Array(values)) => values.iter().find_map(|value| json_u32_value(Some(value))),
        _ => None,
    }
    .filter(|value| *value > 0)
}

fn json_nonnegative_u32_value(value: Option<&Value>) -> Option<u32> {
    match value {
        Some(Value::Number(value)) => value.as_u64().and_then(|value| u32::try_from(value).ok()),
        Some(Value::String(value)) => value.trim().parse::<u32>().ok(),
        Some(Value::Array(values)) => values
            .iter()
            .find_map(|value| json_nonnegative_u32_value(Some(value))),
        _ => None,
    }
}

fn json_nikon_focus_regions(object: &Value) -> (Option<u32>, Option<u32>, Vec<GalleryFocusRegion>) {
    let orientation = json_u32_value(object.get("Orientation"))
        .filter(|orientation| (1..=8).contains(orientation))
        .unwrap_or(1);
    let frame_width = json_u32_value(object.get("AFImageWidth"))
        .or_else(|| json_u32_value(object.get("ImageWidth")));
    let frame_height = json_u32_value(object.get("AFImageHeight"))
        .or_else(|| json_u32_value(object.get("ImageHeight")));
    let (Some(frame_width), Some(frame_height)) = (frame_width, frame_height) else {
        return (None, None, Vec::new());
    };

    let mut regions = match (
        json_nonnegative_u32_value(object.get("AFAreaXPosition")),
        json_nonnegative_u32_value(object.get("AFAreaYPosition")),
        json_u32_value(object.get("AFAreaWidth")),
        json_u32_value(object.get("AFAreaHeight")),
    ) {
        (Some(center_x), Some(center_y), Some(width), Some(height)) => {
            let left = (f64::from(center_x) - f64::from(width) / 2.0) / f64::from(frame_width);
            let top = (f64::from(center_y) - f64::from(height) / 2.0) / f64::from(frame_height);
            let right = (f64::from(center_x) + f64::from(width) / 2.0) / f64::from(frame_width);
            let bottom = (f64::from(center_y) + f64::from(height) / 2.0) / f64::from(frame_height);
            oriented_focus_region(left, top, right, bottom, orientation, true)
                .into_iter()
                .collect()
        }
        _ => phase_detect_focus_regions(object, orientation),
    };
    regions = regions
        .into_iter()
        .filter_map(GalleryFocusRegion::normalized)
        .collect();
    if regions.is_empty() {
        return (None, None, regions);
    }

    let (width, height) = oriented_dimensions(frame_width, frame_height, orientation);
    (Some(width), Some(height), regions)
}

fn phase_detect_focus_regions(object: &Value, orientation: u32) -> Vec<GalleryFocusRegion> {
    let primary = json_first_string(object.get("PrimaryAFPoint"))
        .and_then(|value| parse_nikon_focus_point(&value));
    let mut points = json_first_string(object.get("AFPointsUsed"))
        .map(|value| {
            value
                .split(|character: char| character == ',' || character.is_whitespace())
                .filter_map(parse_nikon_focus_point)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if let Some(primary) = primary
        && !points.contains(&primary)
    {
        points.push(primary);
    }
    points.sort_unstable();
    points.dedup();

    points
        .into_iter()
        .filter_map(|point| {
            let row = u32::from(point[0] - b'A');
            let column = u32::from(point[1] - b'1');
            let center_x = (f64::from(column) + 0.5) / 9.0;
            let center_y = (f64::from(row) + 0.5) / 9.0;
            let half_width = 0.5 / 29.0;
            let half_height = 0.5 / 17.0;
            oriented_focus_region(
                center_x - half_width,
                center_y - half_height,
                center_x + half_width,
                center_y + half_height,
                orientation,
                Some(point) == primary,
            )
        })
        .collect()
}

fn parse_nikon_focus_point(value: &str) -> Option<[u8; 2]> {
    let point = value.split_whitespace().next()?.as_bytes();
    if point.len() != 2 {
        return None;
    }
    let row = point[0].to_ascii_uppercase();
    let column = point[1];
    ((b'A'..=b'I').contains(&row) && (b'1'..=b'9').contains(&column)).then_some([row, column])
}

fn oriented_focus_region(
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
    orientation: u32,
    primary: bool,
) -> Option<GalleryFocusRegion> {
    let corners = [
        oriented_focus_point(left, top, orientation),
        oriented_focus_point(right, top, orientation),
        oriented_focus_point(right, bottom, orientation),
        oriented_focus_point(left, bottom, orientation),
    ];
    let min_x = corners
        .iter()
        .map(|(x, _)| *x)
        .fold(f64::INFINITY, f64::min);
    let min_y = corners
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::INFINITY, f64::min);
    let max_x = corners
        .iter()
        .map(|(x, _)| *x)
        .fold(f64::NEG_INFINITY, f64::max);
    let max_y = corners
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::NEG_INFINITY, f64::max);
    GalleryFocusRegion {
        x: min_x as f32,
        y: min_y as f32,
        width: (max_x - min_x) as f32,
        height: (max_y - min_y) as f32,
        primary,
    }
    .normalized()
}

fn oriented_focus_point(x: f64, y: f64, orientation: u32) -> (f64, f64) {
    match orientation {
        2 => (1.0 - x, y),
        3 => (1.0 - x, 1.0 - y),
        4 => (x, 1.0 - y),
        5 => (y, x),
        6 => (1.0 - y, x),
        7 => (1.0 - y, 1.0 - x),
        8 => (y, 1.0 - x),
        _ => (x, y),
    }
}

fn oriented_dimensions(width: u32, height: u32, orientation: u32) -> (u32, u32) {
    if (5..=8).contains(&orientation) {
        (height, width)
    } else {
        (width, height)
    }
}

fn json_nikon_white_balance_offset(value: Option<&Value>) -> Option<i32> {
    let value = json_first_string(value)?;
    let values = value
        .split(|character: char| character.is_whitespace() || character == ',')
        .filter_map(|value| value.parse::<f32>().ok())
        .collect::<Vec<_>>();
    let offset = *values.get(1)?;
    offset.is_finite().then(|| offset.round() as i32)
}

fn json_string_values(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(value)) => split_gallery_tag_text(value),
        Some(Value::Array(values)) => values
            .iter()
            .flat_map(|value| json_string_values(Some(value)))
            .collect(),
        _ => Vec::new(),
    }
}

fn json_first_string(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(value)) => Some(value.clone()),
        Some(Value::Array(values)) => values
            .iter()
            .find_map(|value| json_first_string(Some(value))),
        _ => None,
    }
}

fn json_exposure_compensation_value(value: Option<&Value>) -> Option<String> {
    let value = match value {
        Some(Value::Number(value)) => json_to_exposure_compensation_value(value.as_f64()?),
        Some(Value::String(value)) => parse_exposure_compensation_text(value),
        Some(Value::Array(values)) => values
            .iter()
            .find_map(|value| json_exposure_compensation_value(Some(value))),
        _ => None,
    };
    let value = value?;
    if value == "0" || value == "0.0" || value == "-0.0" {
        return None;
    }
    Some(value)
}

fn json_to_exposure_compensation_value(value: f64) -> Option<String> {
    normalize_numeric_exposure_compensation(value)
}

fn parse_exposure_compensation_text(value: &str) -> Option<String> {
    for token in
        value.split(|c: char| c.is_whitespace() || c == ';' || c == ',' || c == '(' || c == ')')
    {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let token = token
            .trim_end_matches("EV")
            .trim_end_matches("ev")
            .trim()
            .trim();
        if token.is_empty() {
            continue;
        }
        if token.contains('/') {
            if let Some(value) = parse_rational_exposure_compensation(token) {
                return Some(value);
            }
        } else if let Ok(value) = token.parse::<f64>()
            && let Some(value) = normalize_numeric_exposure_compensation(value)
        {
            return Some(value);
        }
    }

    None
}

fn parse_rational_exposure_compensation(value: &str) -> Option<String> {
    let mut parts = value.split('/');
    let numerator = parts.next()?.trim().parse::<f64>().ok()?;
    let denominator = parts.next()?.trim().parse::<f64>().ok()?;
    if !denominator.is_normal() || denominator == 0.0 {
        return None;
    }
    normalize_numeric_exposure_compensation(numerator / denominator)
}

fn normalize_numeric_exposure_compensation(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    let normalized = (value * 10.0).round() / 10.0;
    if normalized == 0.0 {
        return None;
    }
    Some(format!("{normalized:.1}"))
}

fn json_rating_value(value: Option<&Value>) -> Option<u8> {
    match value {
        Some(Value::Number(value)) => value.as_f64().and_then(normalize_rating_value),
        Some(Value::String(value)) => value
            .trim()
            .parse::<f64>()
            .ok()
            .and_then(normalize_rating_value),
        Some(Value::Array(values)) => values
            .iter()
            .find_map(|value| json_rating_value(Some(value))),
        _ => None,
    }
}

fn normalize_rating_value(value: f64) -> Option<u8> {
    if value.is_finite() && value > 0.0 {
        Some(value.round().clamp(1.0, 5.0) as u8)
    } else {
        None
    }
}

fn split_gallery_tag_text(value: &str) -> Vec<String> {
    value
        .split([',', ';'])
        .map(|value| clean_exif_display_text(value.to_string()))
        .filter(|value| !value.is_empty())
        .collect()
}

fn normalize_gallery_tags(tags: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for tag in tags {
        let tag = clean_exif_display_text(tag);
        let key = tag.to_ascii_lowercase();
        if tag.is_empty() || !seen.insert(key) {
            continue;
        }
        out.push(tag);
    }
    out
}

fn extract_firing_flash_details(exif: &exif::Exif) -> Option<String> {
    let fired = exif_uint_value(exif, Tag::Flash).map(|value| value & 1 != 0)?;
    if !fired {
        return None;
    }
    exif_field_value(exif, Tag::Flash)
}

fn filter_fired_flash_detail(value: &str) -> Option<&str> {
    let normalized = value.to_ascii_lowercase();
    if normalized.contains("did not fire") || normalized.contains("not fired") {
        return None;
    }
    if normalized.contains("fired") {
        Some(value)
    } else {
        None
    }
}

fn clean_optional_exif_text(value: &mut Option<String>) {
    *value = value
        .take()
        .map(clean_exif_display_text)
        .filter(|value| !value.is_empty());
}

fn clean_exif_display_text(value: String) -> String {
    let trimmed = value.trim_matches('\0').trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if trimmed.contains('\0')
        && let Some(value) = trimmed
            .split('\0')
            .map(clean_exif_scalar_text)
            .find(|value| !value.is_empty())
    {
        return value;
    }

    first_quoted_exif_list_value(trimmed).unwrap_or_else(|| clean_exif_scalar_text(trimmed))
}

fn clean_exif_scalar_text(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('\0')
        .trim()
        .to_string()
}

fn first_quoted_exif_list_value(value: &str) -> Option<String> {
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' || ch == '\'' {
            let quote = ch;
            let mut item = String::new();
            for next in chars.by_ref() {
                if next == quote {
                    break;
                }
                item.push(next);
            }
            let cleaned = clean_exif_scalar_text(&item);
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
            continue;
        }

        if ch == ',' || ch.is_whitespace() {
            continue;
        }

        return None;
    }
    None
}

fn format_exif_aperture(raw: String) -> String {
    let mut value = raw.trim().trim_start_matches('f').trim_start_matches('F');
    value = value.trim_start_matches('ƒ');
    let value = value.trim_start_matches('/');
    if value.is_empty() {
        return String::new();
    }
    format!("ƒ/{}", value)
}

fn extract_capture_iso_from_exif(exif: &exif::Exif) -> Option<u32> {
    let tags = [
        Tag::PhotographicSensitivity,
        Tag::ISOSpeed,
        Tag::RecommendedExposureIndex,
        Tag::StandardOutputSensitivity,
        Tag::ExposureIndex,
    ];
    for tag in tags {
        if let Some(value) = exif_field_value(exif, tag)
            && let Some(iso) = parse_iso_value(&value)
        {
            return Some(iso);
        }
    }
    None
}

fn exif_exposure_program(exif: &exif::Exif) -> Option<String> {
    match exif_uint_value(exif, Tag::ExposureProgram) {
        Some(1) => Some("M".to_string()),
        Some(2) => Some("P".to_string()),
        Some(3) => Some("A".to_string()),
        Some(4) => Some("S".to_string()),
        Some(0) | None => None,
        Some(_) => exif_field_value(exif, Tag::ExposureProgram),
    }
}

fn set_file_times(path: &Path, timestamp: &SystemTime) -> Result<()> {
    let file_time = FileTime::from_system_time(*timestamp);
    set_file_atime(path, file_time)
        .with_context(|| format!("setting access time for {}", path.display()))?;
    set_file_mtime(path, file_time)
        .with_context(|| format!("setting modification time for {}", path.display()))?;
    Ok(())
}

pub(crate) fn extract_capture_time(raw: &Path) -> Result<Option<SystemTime>> {
    let raw_file =
        File::open(raw).with_context(|| format!("opening raw file {}", raw.display()))?;
    let mut reader = BufReader::new(raw_file);
    let exif = match Reader::new().read_from_container(&mut reader) {
        Ok(exif) => exif,
        Err(_) => return Ok(None),
    };

    Ok(extract_capture_time_from_exif(&exif))
}

fn extract_capture_time_from_exif(exif: &exif::Exif) -> Option<SystemTime> {
    let datetime_candidates = [
        (Tag::DateTimeOriginal, Some(Tag::OffsetTimeOriginal)),
        (Tag::DateTimeDigitized, Some(Tag::OffsetTimeDigitized)),
        (Tag::DateTime, Some(Tag::OffsetTime)),
    ];

    for (datetime_tag, offset_tag) in datetime_candidates {
        let Some(datetime_value) = exif_field_value(exif, datetime_tag) else {
            continue;
        };
        let offset_value = offset_tag.and_then(|tag| exif_field_value(exif, tag));
        if let Some(capture_time) =
            parse_exif_datetime_with_offset(&datetime_value, offset_value.as_deref())
        {
            return Some(capture_time);
        }
    }

    None
}

pub(crate) fn extract_capture_iso(raw: &Path) -> Result<Option<u32>> {
    let raw_file =
        File::open(raw).with_context(|| format!("opening raw file {}", raw.display()))?;
    let mut reader = BufReader::new(raw_file);
    let exif = match Reader::new().read_from_container(&mut reader) {
        Ok(exif) => exif,
        Err(_) => return Ok(None),
    };

    let tags = [
        Tag::PhotographicSensitivity,
        Tag::ISOSpeed,
        Tag::RecommendedExposureIndex,
        Tag::StandardOutputSensitivity,
        Tag::ExposureIndex,
    ];
    for tag in tags {
        if let Some(value) = exif_field_value(&exif, tag)
            && let Some(iso) = parse_iso_value(&value)
        {
            return Ok(Some(iso));
        }
    }

    Ok(None)
}

fn parse_iso_value(raw: &str) -> Option<u32> {
    for token in raw.split_whitespace() {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }

        if let Some((num, den)) = token.split_once('/') {
            let num: f64 = num.trim().parse().ok()?;
            let den: f64 = den.trim().parse().ok()?;
            if den == 0.0 {
                continue;
            }
            if num < 0.0 || den < 0.0 {
                continue;
            }
            let value = (num / den).round();
            if value.is_finite() && value >= 0.0 && value <= u32::MAX as f64 {
                return Some(value as u32);
            }
        }

        if let Ok(value) = token.parse::<f64>()
            && value.is_sign_positive()
            && value.is_finite()
            && value >= 0.0
            && value <= u32::MAX as f64
        {
            return Some(value.round() as u32);
        }
        continue;
    }
    None
}

fn exif_field_value(exif: &exif::Exif, tag: Tag) -> Option<String> {
    exif.fields()
        .find(|field| field.tag == tag)
        .map(|field| field.display_value().with_unit(exif).to_string())
        .map(clean_exif_display_text)
        .filter(|value| !value.is_empty())
}

fn exif_uint_value(exif: &exif::Exif, tag: Tag) -> Option<u32> {
    exif.fields()
        .find(|field| field.tag == tag)
        .and_then(|field| field.value.get_uint(0))
}

fn parse_exif_datetime_with_offset(datetime: &str, offset: Option<&str>) -> Option<SystemTime> {
    if let Some(offset) = offset {
        let timestamp = format!("{datetime}{offset}");
        if let Some(capture_time) = parse_exif_datetime(&timestamp) {
            return Some(capture_time);
        }
    }

    parse_exif_datetime(datetime)
}

fn parse_exif_datetime(value: &str) -> Option<SystemTime> {
    let value = value.trim();
    let value = value
        .split_once('.')
        .map_or(value, |(seconds, _)| seconds)
        .trim();

    let formats_with_tz = ["%Y:%m:%d %H:%M:%S%:z", "%Y:%m:%d %H:%M:%S%z"];
    let formats_with_tz_alt = ["%Y-%m-%d %H:%M:%S%:z", "%Y-%m-%d %H:%M:%S%z"];
    if let Some(with_tz) = formats_with_tz.iter().find_map(|format| {
        DateTime::parse_from_str(value, format)
            .ok()
            .map(|value| value.to_utc().timestamp())
    }) {
        return unix_timestamp_to_system_time(with_tz);
    }
    if let Some(with_tz) = formats_with_tz_alt.iter().find_map(|format| {
        DateTime::parse_from_str(value, format)
            .ok()
            .map(|value| value.to_utc().timestamp())
    }) {
        return unix_timestamp_to_system_time(with_tz);
    }

    let naive_with_tz_formats = ["%Y:%m:%d %H:%M:%S", "%Y-%m-%d %H:%M:%S"];
    let naive = naive_with_tz_formats
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(value, format).ok())?;
    let local = Local
        .from_local_datetime(&naive)
        .single()
        .or_else(|| Local.from_local_datetime(&naive).earliest())
        .or_else(|| Local.from_local_datetime(&naive).latest())?;

    unix_timestamp_to_system_time(local.with_timezone(&Utc).timestamp())
}

fn unix_timestamp_to_system_time(timestamp: i64) -> Option<SystemTime> {
    if timestamp < 0 {
        return None;
    }
    UNIX_EPOCH
        .checked_add(Duration::new(timestamp as u64, 0))
        .filter(|candidate| *candidate >= UNIX_EPOCH)
}

fn system_time_to_unix_seconds(timestamp: SystemTime) -> Option<i64> {
    timestamp
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
}

#[cfg(test)]
mod tests {
    use super::{
        OutputEditMetadata, add_edit_metadata_args, clean_exif_display_text, extract_gallery_exif,
        format_exif_aperture, gallery_exif_cache_key, json_bool_value, json_nikon_focus_regions,
        json_nikon_white_balance_offset, json_u32_value, json_u64_value,
        nikon_shooting_mode_uses_auto_iso, parse_exif_datetime, parse_exif_datetime_with_offset,
        parse_iso_value, sync_output_timestamps_from_exif,
    };
    use crate::app::profile::ResolvedProfileMetadata;
    use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
    use filetime::{FileTime, set_file_atime, set_file_mtime};
    use mini_film::{GrainSettings, SharpeningSettings};
    use std::time::{Duration, UNIX_EPOCH};
    use std::{fs, path::Path, process::Command};
    use tempfile::tempdir;

    #[test]
    fn gallery_exif_cache_key_tracks_source_identity() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("source.nef");
        fs::write(&source, b"raw").unwrap();

        let initial = gallery_exif_cache_key(&source).unwrap();
        assert_eq!(gallery_exif_cache_key(&source), Some(initial.clone()));

        fs::write(&source, b"changed raw").unwrap();
        assert_ne!(gallery_exif_cache_key(&source), Some(initial));
    }

    #[test]
    fn edit_metadata_omits_profile_sharpening_when_pixels_were_not_sharpened() {
        let profile = ResolvedProfileMetadata {
            profile_name: "Profile".to_string(),
            profile_uuid: None,
            look_name: None,
            look_uuid: None,
            source_profile_name: None,
            source_profile_uuid: None,
            hald_path: None,
            pp3_path: None,
            pp3_adjustments: Vec::new(),
            grain: GrainSettings::default(),
            source_adjustments: Default::default(),
            source_sharpening: SharpeningSettings {
                present: true,
                amount: 42.0,
                radius: 0.8,
                detail: 25.0,
                masking: 10.0,
            },
            emulation_adjustments: Default::default(),
            emulation_sharpening: Default::default(),
            has_camera_raw_settings: true,
        };
        let metadata_args = |profile_sharpening_applied| {
            let edit = OutputEditMetadata {
                comment: None,
                profile: &profile,
                profile_sharpening_applied,
                grain: GrainSettings::default(),
                grain_seed: None,
                grain_engine: None,
                normalize_grain_mpix: None,
            };
            let mut command = Command::new("exiftool");
            add_edit_metadata_args(&mut command, Path::new("frame.jpg"), &edit);
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };

        let compressed_args = metadata_args(false);
        assert!(
            compressed_args
                .iter()
                .all(|arg| !arg.starts_with("-XMP-crs:Sharpen"))
        );

        let raw_args = metadata_args(true);
        assert!(raw_args.iter().any(|arg| arg == "-XMP-crs:Sharpness=42"));
        assert!(
            raw_args
                .iter()
                .any(|arg| arg == "-XMP-crs:SharpenRadius=0.80")
        );
    }

    #[test]
    fn json_u32_value_accepts_positive_numeric_dimensions() {
        assert_eq!(json_u32_value(Some(&serde_json::json!(8288))), Some(8288));
        assert_eq!(json_u32_value(Some(&serde_json::json!("5520"))), Some(5520));
        assert_eq!(json_u32_value(Some(&serde_json::json!(0))), None);
        assert_eq!(json_u32_value(Some(&serde_json::json!(-1))), None);
    }

    #[test]
    fn nikon_focus_rectangle_is_normalized_in_display_orientation() {
        let metadata = serde_json::json!({
            "Orientation": 8,
            "AFImageWidth": 8256,
            "AFImageHeight": 5504,
            "AFAreaXPosition": 5076,
            "AFAreaYPosition": 1738,
            "AFAreaWidth": 884,
            "AFAreaHeight": 884,
        });

        let (width, height, regions) = json_nikon_focus_regions(&metadata);

        assert_eq!((width, height), (Some(5504), Some(8256)));
        assert_eq!(regions.len(), 1);
        let region = regions[0];
        assert!(region.primary);
        assert!((region.x - 1296.0 / 5504.0).abs() < 0.000_01);
        assert!((region.y - (1.0 - 5518.0 / 8256.0)).abs() < 0.000_01);
        assert!((region.width - 884.0 / 5504.0).abs() < 0.000_01);
        assert!((region.height - 884.0 / 8256.0).abs() < 0.000_01);
    }

    #[test]
    fn nikon_phase_detect_points_use_the_81_point_grid() {
        let metadata = serde_json::json!({
            "Orientation": 1,
            "ImageWidth": 8256,
            "ImageHeight": 5504,
            "PrimaryAFPoint": "E5 (Center)",
            "AFPointsUsed": "E4,E5,F5",
        });

        let (width, height, regions) = json_nikon_focus_regions(&metadata);

        assert_eq!((width, height), (Some(8256), Some(5504)));
        assert_eq!(regions.len(), 3);
        let primary = regions.iter().find(|region| region.primary).unwrap();
        assert!((primary.x + primary.width / 2.0 - 0.5).abs() < 0.000_01);
        assert!((primary.y + primary.height / 2.0 - 0.5).abs() < 0.000_01);
        assert!((primary.width - 1.0 / 29.0).abs() < 0.000_01);
        assert!((primary.height - 1.0 / 17.0).abs() < 0.000_01);
    }

    #[test]
    fn nikon_empty_af_block_does_not_invent_a_focus_region() {
        let metadata = serde_json::json!({
            "Orientation": 1,
            "ImageWidth": 8256,
            "ImageHeight": 5504,
            "AFAreaXPosition": 0,
            "AFAreaYPosition": 0,
            "PrimaryAFPoint": "(none)",
            "AFPointsUsed": "(none)",
        });

        assert_eq!(
            json_nikon_focus_regions(&metadata),
            (None, None, Vec::new())
        );
    }

    #[test]
    fn json_u64_value_accepts_numeric_shutter_counts() {
        assert_eq!(
            json_u64_value(Some(&serde_json::json!(66_278))),
            Some(66_278)
        );
        assert_eq!(
            json_u64_value(Some(&serde_json::json!("66278"))),
            Some(66_278)
        );
        assert_eq!(json_u64_value(Some(&serde_json::json!(0))), Some(0));
        assert_eq!(json_u64_value(Some(&serde_json::json!(-1))), None);
    }

    #[test]
    fn json_bool_value_accepts_raw_exiftool_flags() {
        assert_eq!(json_bool_value(Some(&serde_json::json!(1))), Some(true));
        assert_eq!(json_bool_value(Some(&serde_json::json!(0))), Some(false));
        assert_eq!(json_bool_value(Some(&serde_json::json!("On"))), Some(true));
        assert_eq!(
            json_bool_value(Some(&serde_json::json!("Off"))),
            Some(false)
        );
        assert_eq!(json_bool_value(Some(&serde_json::json!("unknown"))), None);
    }

    #[test]
    fn nikon_white_balance_offset_uses_green_magenta_component() {
        assert_eq!(
            json_nikon_white_balance_offset(Some(&serde_json::json!("2 -3"))),
            Some(-3)
        );
        assert_eq!(
            json_nikon_white_balance_offset(Some(&serde_json::json!("2"))),
            None
        );
    }

    #[test]
    fn nikon_shooting_mode_identifies_auto_iso_token() {
        assert!(nikon_shooting_mode_uses_auto_iso("Single-Frame, Auto ISO"));
        assert!(nikon_shooting_mode_uses_auto_iso("auto iso"));
        assert!(!nikon_shooting_mode_uses_auto_iso("Single-Frame"));
    }

    #[test]
    fn extract_gallery_exif_reports_jpeg_source_file_info() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("source.jpg");
        image::RgbImage::new(12, 8).save(&source).unwrap();

        let metadata = extract_gallery_exif(&source).unwrap();

        assert_eq!(
            metadata.file_size_bytes,
            Some(fs::metadata(&source).unwrap().len())
        );
        assert_eq!(metadata.image_width, Some(12));
        assert_eq!(metadata.image_height, Some(8));
    }

    #[test]
    fn parse_exif_datetime_parses_standard_format() {
        let time = parse_exif_datetime("2026:06:07 12:34:56").unwrap();
        let expected = NaiveDateTime::parse_from_str("2026:06:07 12:34:56", "%Y:%m:%d %H:%M:%S")
            .expect("valid exif datetime");
        let local = chrono::Local
            .from_local_datetime(&expected)
            .single()
            .or_else(|| chrono::Local.from_local_datetime(&expected).earliest())
            .unwrap();
        let expected = local.with_timezone(&Utc).timestamp() as u64;
        assert_eq!(
            time.duration_since(UNIX_EPOCH).unwrap(),
            std::time::Duration::from_secs(expected)
        );
    }

    #[test]
    fn parse_exif_datetime_parses_timezone_offset_and_normalizes_to_utc() {
        let time = parse_exif_datetime("2026:06:07 12:34:56+01:00").unwrap();
        let expected = DateTime::parse_from_str("2026:06:07 12:34:56+01:00", "%Y:%m:%d %H:%M:%S%:z")
            .unwrap()
            .to_utc()
            .timestamp() as u64;
        assert_eq!(
            time.duration_since(UNIX_EPOCH).unwrap(),
            std::time::Duration::from_secs(expected)
        );
    }

    #[test]
    fn parse_exif_datetime_parses_display_value_with_hyphens() {
        let time = parse_exif_datetime("2026-06-07 12:34:56").unwrap();
        let local = chrono::Local
            .from_local_datetime(
                &NaiveDateTime::parse_from_str("2026-06-07 12:34:56", "%Y-%m-%d %H:%M:%S")
                    .expect("valid exif datetime"),
            )
            .single()
            .or_else(|| {
                chrono::Local
                    .from_local_datetime(
                        &NaiveDateTime::parse_from_str("2026-06-07 12:34:56", "%Y-%m-%d %H:%M:%S")
                            .expect("valid exif datetime"),
                    )
                    .earliest()
            })
            .unwrap();
        let expected = local.with_timezone(&Utc).timestamp() as u64;
        assert_eq!(
            time.duration_since(UNIX_EPOCH).unwrap(),
            std::time::Duration::from_secs(expected)
        );
    }

    #[test]
    fn parse_exif_datetime_with_separate_offset_field() {
        let time = parse_exif_datetime_with_offset("2026:06:07 12:34:56", Some("+01:00"))
            .expect("offset datetime should parse");
        let expected = DateTime::parse_from_str("2026:06:07 12:34:56+01:00", "%Y:%m:%d %H:%M:%S%:z")
            .unwrap()
            .to_utc()
            .timestamp() as u64;
        assert_eq!(
            time.duration_since(UNIX_EPOCH).unwrap(),
            std::time::Duration::from_secs(expected)
        );
    }

    #[test]
    fn parse_exif_datetime_rejects_invalid_input() {
        assert!(parse_exif_datetime("not-a-date").is_none());
    }

    #[test]
    fn parse_iso_value_prefers_first_parsable() {
        assert_eq!(parse_iso_value("1600/1"), Some(1600));
        assert_eq!(parse_iso_value("6400"), Some(6400));
        assert_eq!(parse_iso_value("ISO 3200"), Some(3200));
    }

    #[test]
    fn parse_iso_value_handles_fractional_ratios() {
        assert_eq!(parse_iso_value("800/2"), Some(400));
        assert_eq!(parse_iso_value("0/0"), None);
    }

    #[test]
    fn sync_output_timestamps_from_exif_falls_back_to_raw_modified_time() {
        let dir = tempdir().unwrap();
        let raw = dir.path().join("source.raw");
        let output = dir.path().join("output.jpg");
        fs::write(&raw, b"raw").unwrap();
        fs::write(&output, b"out").unwrap();

        let past_time = UNIX_EPOCH + Duration::from_secs(1_650_000_000);
        let file_time = FileTime::from_system_time(past_time);
        set_file_atime(&raw, file_time).unwrap();
        set_file_mtime(&raw, file_time).unwrap();

        sync_output_timestamps_from_exif(&raw, &output).unwrap();

        let output_time = fs::metadata(&output).unwrap().modified().unwrap();
        assert_eq!(
            output_time.duration_since(UNIX_EPOCH).unwrap().as_secs(),
            past_time.duration_since(UNIX_EPOCH).unwrap().as_secs(),
        );
    }

    #[test]
    fn format_exif_aperture_normalizes_common_notations() {
        assert_eq!(format_exif_aperture("4".to_string()), "ƒ/4");
        assert_eq!(format_exif_aperture("f/4".to_string()), "ƒ/4");
        assert_eq!(format_exif_aperture("ƒ/4".to_string()), "ƒ/4");
        assert_eq!(format_exif_aperture("ƒ4".to_string()), "ƒ/4");
        assert_eq!(format_exif_aperture("F2.8".to_string()), "ƒ/2.8");
    }

    #[test]
    fn clean_exif_display_text_removes_camera_wrappers() {
        assert_eq!(
            clean_exif_display_text("\"NIKON Z 6\"".to_string()),
            "NIKON Z 6"
        );
        assert_eq!(
            clean_exif_display_text("'NIKON Z 6'".to_string()),
            "NIKON Z 6"
        );
        assert_eq!(
            clean_exif_display_text("NIKON Z 6".to_string()),
            "NIKON Z 6"
        );
    }

    #[test]
    fn clean_exif_display_text_uses_first_non_empty_ascii_item() {
        assert_eq!(
            clean_exif_display_text("\"NIKKOR Z 28mm f/2.8\", \"\", \"\", \"\"".to_string()),
            "NIKKOR Z 28mm f/2.8"
        );
        assert_eq!(
            clean_exif_display_text("\"\", \"\", \"NIKKOR\"".to_string()),
            "NIKKOR"
        );
        assert_eq!(
            clean_exif_display_text("Lens, Inc 28mm".to_string()),
            "Lens, Inc 28mm"
        );
        assert_eq!(
            clean_exif_display_text("NIKON Z 7_2\0\0".to_string()),
            "NIKON Z 7_2"
        );
    }
}
