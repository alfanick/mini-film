use std::{path::Path, process::Command};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::app::review::AutoImportIdentity;

pub(super) fn read_identity(exiftool: &Path, source: &Path) -> Result<AutoImportIdentity> {
    let output = Command::new(exiftool)
        .args([
            "-j",
            "-ImageUniqueID",
            "-DateTimeOriginal",
            "-SubSecTimeOriginal",
            "-OffsetTimeOriginal",
            "-SerialNumber",
            "-InternalSerialNumber",
            "-ShutterCount",
            "-Make",
            "-Model",
            "-OriginalRawFileName",
        ])
        .arg(source)
        .output()
        .with_context(|| format!("reading auto-import identity from {}", source.display()))?;
    if !output.status.success() {
        bail!(
            "ExifTool could not read auto-import identity from {} with status {}",
            source.display(),
            output.status
        );
    }
    let rows: Vec<Value> = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("parsing auto-import identity from {}", source.display()))?;
    let row = rows
        .first()
        .and_then(Value::as_object)
        .with_context(|| format!("ExifTool returned no identity for {}", source.display()))?;
    Ok(AutoImportIdentity {
        image_unique_id: value_text(row.get("ImageUniqueID")),
        capture_timestamp: value_text(row.get("DateTimeOriginal")),
        capture_subsecond: value_text(row.get("SubSecTimeOriginal")),
        capture_offset: value_text(row.get("OffsetTimeOriginal")),
        camera_serial: value_text(row.get("SerialNumber"))
            .or_else(|| value_text(row.get("InternalSerialNumber"))),
        shutter_count: value_i64(row.get("ShutterCount")),
        camera_make: value_text(row.get("Make")),
        camera_model: value_text(row.get("Model")),
        original_raw_filename: value_text(row.get("OriginalRawFileName")),
    })
}

pub(super) fn strong_identity_match(left: &AutoImportIdentity, right: &AutoImportIdentity) -> bool {
    if let (Some(left_id), Some(right_id)) = (
        left.image_unique_id.as_deref(),
        right.image_unique_id.as_deref(),
    ) {
        return left_id.eq_ignore_ascii_case(right_id);
    }

    let (Some(left_serial), Some(right_serial)) = (
        left.camera_serial.as_deref(),
        right.camera_serial.as_deref(),
    ) else {
        return false;
    };
    let (Some(left_timestamp), Some(right_timestamp)) = (
        left.capture_timestamp.as_deref(),
        right.capture_timestamp.as_deref(),
    ) else {
        return false;
    };
    let (Some(left_subsecond), Some(right_subsecond)) = (
        left.capture_subsecond.as_deref(),
        right.capture_subsecond.as_deref(),
    ) else {
        return false;
    };
    let (Some(left_offset), Some(right_offset)) = (
        left.capture_offset.as_deref(),
        right.capture_offset.as_deref(),
    ) else {
        return false;
    };
    if !left_serial.eq_ignore_ascii_case(right_serial)
        || left_timestamp != right_timestamp
        || left_subsecond != right_subsecond
        || left_offset != right_offset
    {
        return false;
    }
    match (left.shutter_count, right.shutter_count) {
        (Some(left), Some(right)) => left == right,
        _ => true,
    }
}

fn value_text(value: Option<&Value>) -> Option<String> {
    let value = match value? {
        Value::String(value) => value.trim().to_string(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        _ => return None,
    };
    (!value.is_empty() && value != "-").then_some(value)
}

fn value_i64(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(value) => value.as_i64(),
        Value::String(value) => value.trim().parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timestamp_identity(serial: &str, timestamp: &str) -> AutoImportIdentity {
        AutoImportIdentity {
            camera_serial: Some(serial.to_string()),
            capture_timestamp: Some(timestamp.to_string()),
            capture_subsecond: Some("42".to_string()),
            capture_offset: Some("+02:00".to_string()),
            ..AutoImportIdentity::default()
        }
    }

    #[test]
    fn image_unique_id_is_a_strong_match() {
        let left = AutoImportIdentity {
            image_unique_id: Some("ABC123".to_string()),
            ..AutoImportIdentity::default()
        };
        let right = AutoImportIdentity {
            image_unique_id: Some("abc123".to_string()),
            ..AutoImportIdentity::default()
        };
        assert!(strong_identity_match(&left, &right));
    }

    #[test]
    fn timestamp_identity_requires_serial_subseconds_and_offset() {
        let left = timestamp_identity("camera-1", "2026:07:27 12:34:56");
        let mut right = left.clone();
        assert!(strong_identity_match(&left, &right));

        right.capture_subsecond = None;
        assert!(!strong_identity_match(&left, &right));
        right = left.clone();
        right.camera_serial = Some("camera-2".to_string());
        assert!(!strong_identity_match(&left, &right));

        let mut no_subseconds_or_offsets = left.clone();
        no_subseconds_or_offsets.capture_subsecond = None;
        no_subseconds_or_offsets.capture_offset = None;
        assert!(!strong_identity_match(
            &no_subseconds_or_offsets,
            &no_subseconds_or_offsets
        ));
    }

    #[test]
    fn missing_strong_fields_never_matches() {
        let left = AutoImportIdentity {
            capture_timestamp: Some("2026:07:27 12:34:56".to_string()),
            ..AutoImportIdentity::default()
        };
        assert!(!strong_identity_match(
            &left,
            &AutoImportIdentity::default()
        ));
    }
}
