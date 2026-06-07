use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDateTime};
use exif::{In, Reader, Tag};
use filetime::{FileTime, set_file_atime, set_file_mtime};

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

    let datetime_fields = [
        (Tag::DateTimeOriginal, In::PRIMARY),
        (Tag::DateTimeDigitized, In::PRIMARY),
        (Tag::DateTime, In::PRIMARY),
    ];

    for (tag, ifd) in datetime_fields {
        let Some(field) = exif.get_field(tag, ifd) else {
            continue;
        };

        let raw_value = field.display_value().with_unit(&exif).to_string();
        let value = raw_value.trim_matches('\0').trim();
        if let Some(capture_time) = parse_exif_datetime(value) {
            return Ok(Some(capture_time));
        }
    }

    Ok(None)
}

fn parse_exif_datetime(value: &str) -> Option<SystemTime> {
    let formats_with_tz = ["%Y:%m:%d %H:%M:%S%:z", "%Y:%m:%d %H:%M:%S%z"];
    if let Some(with_tz) = formats_with_tz.iter().find_map(|format| {
        DateTime::parse_from_str(value, format)
            .ok()
            .map(|value| value.to_utc().timestamp())
    }) {
        return unix_timestamp_to_system_time(with_tz);
    }

    let naive = NaiveDateTime::parse_from_str(value, "%Y:%m:%d %H:%M:%S").ok()?;
    unix_timestamp_to_system_time(naive.and_utc().timestamp())
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
    use super::parse_exif_datetime;
    use super::sync_output_timestamps_from_exif;
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
        let expected = Utc.from_utc_datetime(&expected).timestamp() as u64;
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
    fn parse_exif_datetime_rejects_invalid_input() {
        assert!(parse_exif_datetime("not-a-date").is_none());
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
}
