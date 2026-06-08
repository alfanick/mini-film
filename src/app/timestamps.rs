use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use exif::{Reader, Tag};
use filetime::{FileTime, set_file_atime, set_file_mtime};

#[derive(Clone, Debug, Default)]
pub(crate) struct GalleryExifData {
    pub(crate) focal_length: Option<String>,
    pub(crate) aperture: Option<String>,
    pub(crate) shutter_speed: Option<String>,
    pub(crate) iso: Option<String>,
    pub(crate) camera_model: Option<String>,
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
    exif_comment: Option<&str>,
) -> Result<()> {
    let is_tiff = matches!(
        output.extension().and_then(|ext| ext.to_str()),
        Some(ext) if ext.eq_ignore_ascii_case("tiff") || ext.eq_ignore_ascii_case("tif")
    );

    let status = run_exiftool_copy_all(raw, output, exif_comment)
        .with_context(|| format!("running exiftool on {}", output.display()))?;

    if status.success() {
        return Ok(());
    }

    if !is_tiff {
        return Err(anyhow::anyhow!(
            "exiftool failed with status {status} while syncing metadata"
        ));
    }

    if !run_exiftool_fallback(raw, output, exif_comment)?.success()
        && !run_exiftool_minimal(output, exif_comment)?.success()
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
    exif_comment: Option<&str>,
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
    if let Some(comment) = exif_comment {
        command.arg(format!("-Comment={comment}"));
    }
    command.arg(output);
    command.stdout(Stdio::null()).stderr(Stdio::null());

    Ok(command.status()?)
}

fn run_exiftool_fallback(
    raw: &Path,
    output: &Path,
    exif_comment: Option<&str>,
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
    if let Some(comment) = exif_comment {
        command.arg(format!("-Comment={comment}"));
    }
    command.arg(output);
    command.stdout(Stdio::null()).stderr(Stdio::null());

    Ok(command.status()?)
}

fn run_exiftool_minimal(
    output: &Path,
    exif_comment: Option<&str>,
) -> Result<std::process::ExitStatus> {
    // Final TIFF fallback: write only a tiny amount of metadata that is
    // widely supported (best-effort so failures still only degrade metadata).
    let mut command = Command::new("exiftool");
    command
        .arg("-q")
        .arg("-quiet")
        .arg("-overwrite_original")
        .arg("-m");
    if let Some(comment) = exif_comment {
        command.arg(format!("-Comment={comment}"));
    }
    command.arg(output);
    command.stdout(Stdio::null()).stderr(Stdio::null());

    Ok(command.status()?)
}

pub(crate) fn extract_gallery_exif(file: &Path) -> Result<GalleryExifData> {
    let file = File::open(file).with_context(|| format!("opening file {}", file.display()))?;
    let mut reader = BufReader::new(file);
    let exif = match Reader::new().read_from_container(&mut reader) {
        Ok(exif) => exif,
        Err(_) => return Ok(GalleryExifData::default()),
    };

    let focal_length = exif_field_value(&exif, Tag::FocalLength);
    let aperture = exif_field_value(&exif, Tag::FNumber)
        .or_else(|| exif_field_value(&exif, Tag::MaxApertureValue));
    let shutter_speed = exif_field_value(&exif, Tag::ExposureTime);
    let iso = extract_capture_iso_from_exif(&exif).map(|iso| iso.to_string());

    let camera_model = exif_field_value(&exif, Tag::Model).map(strip_surrounding_quotes);

    Ok(GalleryExifData {
        focal_length,
        aperture: aperture.map(format_exif_aperture),
        shutter_speed,
        iso,
        camera_model,
    })
}

fn strip_surrounding_quotes(value: String) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
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

    let datetime_candidates = [
        (Tag::DateTimeOriginal, Some(Tag::OffsetTimeOriginal)),
        (Tag::DateTimeDigitized, Some(Tag::OffsetTimeDigitized)),
        (Tag::DateTime, Some(Tag::OffsetTime)),
    ];

    for (datetime_tag, offset_tag) in datetime_candidates {
        let Some(datetime_value) = exif_field_value(&exif, datetime_tag) else {
            continue;
        };
        let offset_value = offset_tag.and_then(|tag| exif_field_value(&exif, tag));
        if let Some(capture_time) =
            parse_exif_datetime_with_offset(&datetime_value, offset_value.as_deref())
        {
            return Ok(Some(capture_time));
        }
    }

    Ok(None)
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
        .map(|value| value.trim_matches('\0').trim().to_string())
        .filter(|value| !value.is_empty())
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

#[cfg(test)]
mod tests {
    use super::{
        format_exif_aperture, parse_exif_datetime, parse_exif_datetime_with_offset,
        parse_iso_value, strip_surrounding_quotes, sync_output_timestamps_from_exif,
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
    fn strip_surrounding_quotes_removes_camera_wrappers() {
        assert_eq!(
            strip_surrounding_quotes("\"NIKON Z 6\"".to_string()),
            "NIKON Z 6"
        );
        assert_eq!(
            strip_surrounding_quotes("'NIKON Z 6'".to_string()),
            "NIKON Z 6"
        );
        assert_eq!(
            strip_surrounding_quotes("NIKON Z 6".to_string()),
            "NIKON Z 6"
        );
    }
}
