use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use exif::{In, Reader, Tag};
use filetime::{FileTime, set_file_atime, set_file_mtime};

pub(crate) fn sync_output_timestamps_from_exif(raw: &Path, output: &Path) -> Result<bool> {
    let capture_time = extract_capture_time(raw)?;
    let Some(timestamp) = capture_time else {
        return Ok(false);
    };

    let file_time = FileTime::from_system_time(timestamp);
    set_file_atime(output, file_time)
        .with_context(|| format!("setting access time for {}", output.display()))?;
    set_file_mtime(output, file_time)
        .with_context(|| format!("setting modification time for {}", output.display()))?;
    Ok(true)
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
    let naive = NaiveDateTime::parse_from_str(value, "%Y:%m:%d %H:%M:%S").ok()?;

    let timestamp = naive.and_utc().timestamp();
    let nanos = naive.and_utc().timestamp_subsec_nanos();
    if timestamp < 0 {
        return None;
    }
    UNIX_EPOCH
        .checked_add(Duration::new(timestamp as u64, nanos))
        .filter(|candidate| *candidate >= UNIX_EPOCH)
}

#[cfg(test)]
mod tests {
    use super::parse_exif_datetime;
    use chrono::{NaiveDateTime, TimeZone, Utc};
    use std::time::UNIX_EPOCH;

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
    fn parse_exif_datetime_rejects_invalid_input() {
        assert!(parse_exif_datetime("not-a-date").is_none());
    }
}
