//! Verify typed wire adapters against real review state and the historical JSON semantics.
//! These checks complement generated schema validation rather than replacing it with mock types.

use super::*;
use crate::review_contract as wire;

/// The typed snapshot retains the old root shape and required nullable output fields.
#[test]
fn wire_snapshot_and_patch_preserve_null_omission_and_diffusion_replacements() {
    let directory = tempfile::tempdir().unwrap();
    let mut handle = test_handle(
        directory.path().join("input"),
        directory.path().join("output"),
        vec![profile(0, "test")],
    );
    handle.invocation = Some("mini-film batch".to_owned());
    let previous = handle.api_state_snapshot().unwrap();
    let unchanged = wire::ReviewStatePatch::between(&previous, &previous);
    assert_eq!(
        serde_json::to_value(unchanged).unwrap(),
        json!({"type":"patch","version":env!("CARGO_PKG_VERSION")})
    );
    handle.invocation = None;
    handle.diffusion.softness = 31;
    let mut current = handle.api_state_snapshot().unwrap();
    current
        .profile_diffusion_settings
        .push(wire::ReviewProfileDiffusionSetting {
            profile_index: 0,
            settings: (&handle.diffusion).into(),
        });
    let patch = serde_json::to_value(wire::ReviewStatePatch::between(&previous, &current)).unwrap();
    assert_eq!(patch["invocation"], serde_json::Value::Null);
    assert_eq!(patch["diffusion_default"]["softness"], 31);
    assert_eq!(patch["profile_diffusion_settings"][0]["profile_index"], 0);
    assert!(patch.get("images").is_none());
    let snapshot = serde_json::to_value(current).unwrap();
    assert!(snapshot.get("type").is_none());
    assert!(snapshot.get("invocation").is_some());
    assert!(snapshot["profiles"][0].get("hald_path").is_none());
    assert_eq!(
        snapshot["publish_defaults"]["resize"],
        serde_json::Value::Null
    );
}

/// Shared primitive adapters serialize exactly like the existing renderer and metadata values.
#[test]
fn wire_value_adapters_preserve_serialized_defaults_and_metadata() {
    let settings = mini_film::DiffusionSettings::default();
    assert_eq!(
        serde_json::to_value(wire::DiffusionSettings::from(&settings)).unwrap(),
        serde_json::to_value(settings).unwrap()
    );
    let metadata = fully_populated_review_store().profiles[0]
        .metadata
        .clone()
        .unwrap();
    assert_eq!(
        serde_json::to_value(wire::ReviewProfileMetadata::from(&metadata)).unwrap(),
        serde_json::to_value(metadata).unwrap()
    );
    let mut exif = GalleryExifData {
        camera_serial: Some("private serial".to_owned()),
        nikon_burst_key: Some("private burst".to_owned()),
        nikon_burst_shot_number: Some(7),
        ..GalleryExifData::default()
    };
    exif.tags = vec!["one".to_owned(), "two".to_owned()];
    assert_eq!(
        serde_json::to_value(wire::GalleryExifData::from(&exif)).unwrap(),
        serde_json::to_value(exif).unwrap()
    );
}

/// Input adapters leave legacy precedence, label normalization, and empty publish bodies untouched.
#[test]
fn wire_request_adapters_keep_partial_defaults_and_parallel_profile_fields() {
    let input: wire::ReviewUpdateRequest = serde_json::from_value(json!({"image_id":7,"rating":255,"tags":[],"label":"blue","labels":["red"],"enabled_profile_indexes":[0],"publish_profile_indexes":[1],"retouch":{"crop":{}}})).unwrap();
    let request: ReviewUpdateRequest = input.into();
    assert_eq!(request.rating, 255);
    assert_eq!(request.label, ReviewLabel::Blue);
    assert_eq!(request.labels, vec![ReviewLabel::Red]);
    assert_eq!(request.enabled_profile_indexes, Some(vec![0]));
    assert_eq!(request.publish_profile_indexes, Some(vec![1]));
    assert_eq!(request.retouch.unwrap().crop.unwrap().width, 1.0);
    let input: wire::ReviewUiUpdateRequest =
        serde_json::from_value(json!({"labels":["red","red"],"future":true})).unwrap();
    let request: ReviewUiUpdateRequest = input.into();
    assert_eq!(
        request.labels.into_iter().collect::<Vec<_>>(),
        vec![ReviewLabel::Red]
    );
    assert!(parse_publish_request(&[]).unwrap().album.is_none());
}

/// Typed image replacement/removal retains the server's existing ordering and full-record semantics.
#[test]
fn wire_image_patch_keeps_complete_images_and_explicit_ordering() {
    let directory = tempfile::tempdir().unwrap();
    let store = fully_populated_review_store();
    let handle = test_handle(
        directory.path().join("input"),
        directory.path().join("output"),
        store.profiles.clone(),
    );
    handle.state.store(Arc::new(store));
    let previous = handle.api_state_snapshot().unwrap();
    assert!(previous.images.len() > 1);
    let mut current = previous.clone();
    let removed = current.images.pop().unwrap().id;
    current.images[0].notes = "new manual note".to_owned();
    let patch = serde_json::to_value(wire::ReviewStatePatch::between(&previous, &current)).unwrap();
    assert_eq!(patch["removed_image_ids"], json!([removed]));
    assert_eq!(
        patch["image_ids"],
        json!(
            current
                .images
                .iter()
                .map(|image| image.id)
                .collect::<Vec<_>>()
        )
    );
    assert_eq!(patch["images"], json!([current.images[0]]));
    let render = &patch["images"][0]["profiles"][0];
    assert_eq!(
        render["diffusion"]["settings"],
        render["diffusion_settings"]
    );
    assert_eq!(render["diffusion"]["source"], render["diffusion_source"]);
}

/// Polling-job adapters preserve each public field and omit private paths and unproduced aliases.
#[test]
fn wire_polling_snapshots_match_existing_job_serializers() {
    let job = ReviewDiffusionJob {
        id: 7,
        status: ReviewDiffusionJobStatus::Done,
        image_id: 1,
        profile_index: 0,
        settings: DiffusionSettings::default(),
        before_url: Some("before".to_owned()),
        after_url: Some("after".to_owned()),
        preview_width: Some(512),
        preview_height: Some(341),
        focus_source: Some(ReviewDiffusionFocusSource::CenterFallback),
        detail_areas: vec![ReviewDiffusionDetailArea {
            kind: ReviewDiffusionDetailAreaKind::Focus,
            x: 1,
            y: 2,
            width: 30,
            height: 40,
        }],
        error: None,
        before_path: Some(PathBuf::from("private-before")),
        after_path: Some(PathBuf::from("private-after")),
    };
    assert_eq!(
        serde_json::to_value(wire::ReviewDiffusionJob::from(&job)).unwrap(),
        serde_json::to_value(job).unwrap()
    );
    let job = ReviewSamplerJobSnapshot {
        id: 8,
        image_id: 1,
        file_name: "fixture.nef".to_owned(),
        status: ReviewSamplerJobStatus::Done,
        source_url: Some("source".to_owned()),
        source_width: Some(512),
        source_height: Some(341),
        completed: 1,
        total: 1,
        failed: 0,
        workers: 1,
        error: None,
        entries: vec![ReviewSamplerEntrySnapshot {
            key: "key".to_owned(),
            name: "name".to_owned(),
            filename: "profile.xmp".to_owned(),
            parts: vec!["group".to_owned()],
            status: ReviewSamplerEntryStatus::Done,
            thumbnail_url: Some("thumbnail".to_owned()),
            duration_ms: Some(20),
            error: None,
            current_enabled: true,
            all_enabled: false,
            configured_from_cli: true,
            selected: true,
        }],
    };
    assert_eq!(
        serde_json::to_value(wire::ReviewSamplerJobSnapshot::from(&job)).unwrap(),
        serde_json::to_value(job).unwrap()
    );
}
