use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use exif::{Reader, Tag};
use filetime::{FileTime, set_file_atime, set_file_mtime};
use mini_film::{GrainSettings, ProfileAdjustments, SharpeningSettings, ToneCurves};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::profile::ResolvedProfileMetadata;

pub(crate) struct OutputEditMetadata<'a> {
    pub(crate) comment: Option<&'a str>,
    pub(crate) profile: &'a ResolvedProfileMetadata,
    pub(crate) grain: GrainSettings,
    pub(crate) grain_seed: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct GalleryExifData {
    #[serde(default)]
    pub(crate) capture_timestamp: Option<i64>,
    pub(crate) focal_length: Option<String>,
    pub(crate) aperture: Option<String>,
    pub(crate) shutter_speed: Option<String>,
    pub(crate) iso: Option<String>,
    pub(crate) camera_model: Option<String>,
    pub(crate) lens_model: Option<String>,
    pub(crate) shooting_mode: Option<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) note: Option<String>,
}

impl GalleryExifData {
    pub(crate) fn is_empty(&self) -> bool {
        self.capture_timestamp.is_none()
            && self.focal_length.is_none()
            && self.aperture.is_none()
            && self.shutter_speed.is_none()
            && self.iso.is_none()
            && self.camera_model.is_none()
            && self.lens_model.is_none()
            && self.shooting_mode.is_none()
            && self.tags.is_empty()
            && self.note.is_none()
    }

    pub(crate) fn sanitize_text_fields(&mut self) {
        clean_optional_exif_text(&mut self.focal_length);
        clean_optional_exif_text(&mut self.aperture);
        clean_optional_exif_text(&mut self.shutter_speed);
        clean_optional_exif_text(&mut self.iso);
        clean_optional_exif_text(&mut self.camera_model);
        clean_optional_exif_text(&mut self.lens_model);
        clean_optional_exif_text(&mut self.shooting_mode);
        clean_optional_exif_text(&mut self.note);
        self.tags = normalize_gallery_tags(std::mem::take(&mut self.tags));
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

pub(crate) fn sync_output_metadata_from_raw(
    raw: &Path,
    output: &Path,
    edit: OutputEditMetadata<'_>,
) -> Result<()> {
    let is_tiff = matches!(
        output.extension().and_then(|ext| ext.to_str()),
        Some(ext) if ext.eq_ignore_ascii_case("tiff") || ext.eq_ignore_ascii_case("tif")
    );

    let status = run_exiftool_copy_all(raw, output, &edit)
        .with_context(|| format!("running exiftool on {}", output.display()))?;

    if status.success() {
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
        let sharpening = combined_sharpening(profile);
        add_profile_adjustment_args(command, &adjustments);
        add_sharpening_args(command, sharpening);
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

pub(crate) fn extract_gallery_exif(file: &Path) -> Result<GalleryExifData> {
    let opened = File::open(file).with_context(|| format!("opening file {}", file.display()))?;
    let mut reader = BufReader::new(opened);
    let exif = match Reader::new().read_from_container(&mut reader) {
        Ok(exif) => exif,
        Err(_) => return Ok(GalleryExifData::default()),
    };

    let focal_length = exif_field_value(&exif, Tag::FocalLength);
    let aperture = exif_field_value(&exif, Tag::FNumber)
        .or_else(|| exif_field_value(&exif, Tag::MaxApertureValue));
    let shutter_speed = exif_field_value(&exif, Tag::ExposureTime);
    let iso = extract_capture_iso_from_exif(&exif).map(|iso| iso.to_string());

    let camera_model = exif_field_value(&exif, Tag::Model);
    let lens_model = exif_field_value(&exif, Tag::LensModel)
        .or_else(|| exif_field_value(&exif, Tag::LensSpecification));
    let shooting_mode = exif_exposure_program(&exif);
    let mut note = exif_field_value(&exif, Tag::ImageDescription);
    let mut tags = Vec::new();
    if let Some(metadata) = extract_gallery_text_metadata_with_exiftool(file) {
        tags = metadata.tags;
        if note.is_none() {
            note = metadata.note;
        }
    }
    let capture_timestamp =
        extract_capture_time_from_exif(&exif).and_then(system_time_to_unix_seconds);

    let mut data = GalleryExifData {
        capture_timestamp,
        focal_length,
        aperture: aperture.map(format_exif_aperture),
        shutter_speed,
        iso,
        camera_model,
        lens_model,
        shooting_mode,
        tags,
        note,
    };
    data.sanitize_text_fields();
    Ok(data)
}

#[derive(Debug, Default)]
struct GalleryTextMetadata {
    tags: Vec<String>,
    note: Option<String>,
}

fn extract_gallery_text_metadata_with_exiftool(file: &Path) -> Option<GalleryTextMetadata> {
    let output = Command::new("exiftool")
        .arg("-q")
        .arg("-q")
        .arg("-j")
        .arg("-Subject")
        .arg("-Keywords")
        .arg("-Description")
        .arg("-ImageDescription")
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
    Some(GalleryTextMetadata { tags, note })
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
        clean_exif_display_text, format_exif_aperture, parse_exif_datetime,
        parse_exif_datetime_with_offset, parse_iso_value, sync_output_timestamps_from_exif,
    };
    use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
    use filetime::{FileTime, set_file_atime, set_file_mtime};
    use std::fs;
    use std::time::{Duration, UNIX_EPOCH};
    use tempfile::tempdir;

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
