use super::prelude::*;
use super::{handle::*, model::*, preview::*, publish::*, scheduler::*, server::*, store::*};

fn profile(index: usize, stem: &str) -> ReviewProfile {
    ReviewProfile {
        index,
        selector: stem.to_string(),
        stem: stem.to_string(),
        retouch_base: BasicRetouchAdjustments::default(),
    }
}

fn test_export_options() -> ExportOptions {
    ExportOptions {
        jpg_quality: 90,
        resize: None,
        long_edge: None,
        max_width: None,
        max_height: None,
        jpeg_subsampling: crate::cli::JpegSubsampling::S444,
        strip_metadata: false,
        progressive_jpeg: false,
    }
}

fn test_handle(input: PathBuf, output: PathBuf, profiles: Vec<ReviewProfile>) -> ReviewHandle {
    let export = test_export_options();
    let (subscribers, _) = broadcast::channel(256);
    ReviewHandle {
        state: Arc::new(Mutex::new(ReviewStore::new(profiles))),
        subscribers: Arc::new(subscribers),
        state_path: output.join("mini-film-review.json"),
        input_root: input.clone(),
        output_root: output.clone(),
        hald_dir: output.join("hald"),
        profiles_root: input.clone(),
        hald_level: 16,
        rawtherapee: PathBuf::from("rawtherapee-cli"),
        output_format: BatchOutputFormat::Jpg,
        gallery: None,
        convert: PathBuf::from("convert"),
        export: export.clone(),
        jobs: 1,
        no_grain: false,
        color_noise_iso_threshold: 1600,
        lens_corrections: LensCorrections::default(),
        grain: None,
        grain_preset: None,
        grain_seed: Some(1),
        publish_defaults: ReviewPublishDefaults::new(
            "published".to_string(),
            BatchOutputFormat::Jpg,
            &export,
            ReviewGalleryDefaults {
                template: None,
                thumbnail_long_edge: 1024,
                columns: 4,
            },
        ),
        publish_jobs: Arc::new(Mutex::new(Vec::new())),
        next_publish_job_id: Arc::new(Mutex::new(1)),
        retouch_scheduler: Arc::new(ReviewRetouchScheduler::default()),
    }
}

fn test_publish_options(album: &str) -> ReviewPublishOptions {
    ReviewPublishOptions {
        album: PathBuf::from(album),
        min_rating: 2,
        labels: HashSet::new(),
        tags: HashSet::new(),
        output_format: BatchOutputFormat::Jpg,
        hald_dir: PathBuf::from("hald"),
        profiles_root: PathBuf::from("profiles"),
        hald_level: 16,
        rawtherapee: PathBuf::from("rawtherapee-cli"),
        convert: PathBuf::from("convert"),
        jobs: 2,
        export: test_export_options(),
        rerender_raw: false,
        no_grain: false,
        color_noise_iso_threshold: 1600,
        lens_corrections: LensCorrections::default(),
        grain: None,
        grain_preset: None,
        grain_seed: Some(1),
        write_metadata: false,
    }
}

fn profile_render(index: usize, stem: &str) -> ReviewProfileRender {
    ReviewProfileRender {
        profile_index: index,
        profile_stem: stem.to_string(),
        status: ReviewRenderStatus::Done,
        output_path: None,
        error: None,
        duration_ms: Some(1),
        render_key: None,
        updated_at: now_string(),
    }
}

#[test]
fn preferred_preview_profile_prefers_checked_profile_when_visible_is_unchecked() {
    let image = ReviewImage {
        id: 1,
        raw_path: PathBuf::from("/in/frame.NEF"),
        relative_path: "frame.NEF".to_string(),
        file_name: "frame.NEF".to_string(),
        exif: GalleryExifData::default(),
        selected_profile_index: 2,
        rating: 0,
        label: ReviewLabel::None,
        labels: Vec::new(),
        tags: Vec::new(),
        notes: String::new(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![1]),
        preview: ReviewPreview::default(),
        profiles: vec![
            profile_render(0, "A"),
            profile_render(1, "B"),
            profile_render(2, "C"),
        ],
        updated_at: now_string(),
    };

    let publish_indexes = effective_publish_profile_indexes(&image);
    assert_eq!(
        preferred_preview_profile_index(&image, &publish_indexes),
        Some(1)
    );

    let mut no_checked = image;
    no_checked.publish_profile_indexes = Some(Vec::new());
    let publish_indexes = effective_publish_profile_indexes(&no_checked);
    assert_eq!(
        preferred_preview_profile_index(&no_checked, &publish_indexes),
        Some(2)
    );
}

#[test]
fn review_state_defaults_to_first_profile_and_records_outputs() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(input.join("day")).unwrap();
    fs::create_dir_all(&output).unwrap();
    let raw = input.join("day").join("frame.NEF");
    fs::write(&raw, b"raw").unwrap();
    let rendered = output.join("day").join("Classic").join("frame.jpg");
    fs::create_dir_all(rendered.parent().unwrap()).unwrap();
    fs::write(&rendered, b"jpg").unwrap();

    let handle = test_handle(
        input,
        output,
        vec![profile(0, "Classic"), profile(1, "Fade")],
    );

    handle.record_discovered_raw(&raw).unwrap();
    handle
        .record_profile_done(&raw, 0, &rendered, Duration::from_millis(42))
        .unwrap();
    let text = handle.api_state_json().unwrap();
    assert!(text.contains("\"selected_profile_index\":0"));
    assert!(text.contains("\"publish_profile_indexes\":[0,1]"));
    assert!(text.contains("\"status\":\"done\""));
    assert!(text.contains("media/1/0"));
}

#[test]
fn review_visible_order_uses_exif_capture_time_before_path() {
    let mut store = ReviewStore::new(vec![profile(0, "Classic")]);
    store.images.push(ReviewImage {
        id: 1,
        raw_path: PathBuf::from("/in/camera-a/late.NEF"),
        relative_path: "camera-a/late.NEF".to_string(),
        file_name: "late.NEF".to_string(),
        exif: GalleryExifData {
            capture_timestamp: Some(300),
            ..GalleryExifData::default()
        },
        preview: ReviewPreview::default(),
        selected_profile_index: 0,
        rating: 1,
        label: ReviewLabel::None,
        labels: Vec::new(),
        tags: Vec::new(),
        notes: String::new(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![0]),
        profiles: vec![profile_render(0, "Classic")],
        updated_at: now_string(),
    });
    store.images.push(ReviewImage {
        id: 2,
        raw_path: PathBuf::from("/in/camera-b/early.NEF"),
        relative_path: "camera-b/early.NEF".to_string(),
        file_name: "early.NEF".to_string(),
        exif: GalleryExifData {
            capture_timestamp: Some(100),
            ..GalleryExifData::default()
        },
        preview: ReviewPreview::default(),
        selected_profile_index: 0,
        rating: 1,
        label: ReviewLabel::None,
        labels: Vec::new(),
        tags: Vec::new(),
        notes: String::new(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![0]),
        profiles: vec![profile_render(0, "Classic")],
        updated_at: now_string(),
    });
    store.images.push(ReviewImage {
        id: 3,
        raw_path: PathBuf::from("/in/camera-c/no-exif.NEF"),
        relative_path: "camera-c/no-exif.NEF".to_string(),
        file_name: "no-exif.NEF".to_string(),
        exif: GalleryExifData::default(),
        preview: ReviewPreview::default(),
        selected_profile_index: 0,
        rating: 1,
        label: ReviewLabel::None,
        labels: Vec::new(),
        tags: Vec::new(),
        notes: String::new(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![0]),
        profiles: vec![profile_render(0, "Classic")],
        updated_at: now_string(),
    });

    assert_eq!(store.visible_image_ids_at(1), vec![2, 1, 3]);
    let mut images = store.images.clone();
    sort_review_images(&mut images);
    assert_eq!(
        images.iter().map(|image| image.id).collect::<Vec<_>>(),
        vec![2, 1, 3]
    );
}

#[test]
fn sync_profiles_drops_stale_renders_when_wizard_profile_changes() {
    let mut store = ReviewStore::new(vec![profile(0, "Old")]);
    store.images.push(ReviewImage {
        id: 1,
        raw_path: PathBuf::from("/in/frame.NEF"),
        relative_path: "frame.NEF".to_string(),
        file_name: "frame.NEF".to_string(),
        exif: GalleryExifData::default(),
        preview: ReviewPreview::default(),
        selected_profile_index: 0,
        rating: 3,
        label: ReviewLabel::None,
        labels: Vec::new(),
        tags: vec!["keep".to_string()],
        notes: "preserve review metadata".to_string(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![0]),
        profiles: vec![ReviewProfileRender {
            output_path: Some(PathBuf::from("/out/Old/frame.jpg")),
            ..profile_render(0, "Old")
        }],
        updated_at: now_string(),
    });

    store.sync_profiles(vec![profile(0, "New")]);

    assert_eq!(store.profiles, vec![profile(0, "New")]);
    assert_eq!(store.images[0].rating, 3);
    assert_eq!(store.images[0].tags, vec!["keep"]);
    assert_eq!(store.images[0].notes, "preserve review metadata");
    assert_eq!(store.images[0].selected_profile_index, 0);
    assert_eq!(store.images[0].publish_profile_indexes, Some(vec![0]));
    let render = &store.images[0].profiles[0];
    assert_eq!(render.profile_stem, "New");
    assert_eq!(render.status, ReviewRenderStatus::Missing);
    assert_eq!(render.output_path, None);
    assert_eq!(render.duration_ms, None);
}

#[test]
fn sync_profiles_selects_all_wizard_profiles_when_profile_set_changes() {
    let mut store = ReviewStore::new(vec![profile(0, "Classic")]);
    store.images.push(ReviewImage {
        id: 1,
        raw_path: PathBuf::from("/in/frame.NEF"),
        relative_path: "frame.NEF".to_string(),
        file_name: "frame.NEF".to_string(),
        exif: GalleryExifData::default(),
        preview: ReviewPreview::default(),
        selected_profile_index: 0,
        rating: 0,
        label: ReviewLabel::None,
        labels: Vec::new(),
        tags: Vec::new(),
        notes: String::new(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![0]),
        profiles: vec![ReviewProfileRender {
            output_path: Some(PathBuf::from("/out/Classic/frame.jpg")),
            ..profile_render(0, "Classic")
        }],
        updated_at: now_string(),
    });

    store.sync_profiles(vec![profile(0, "Classic"), profile(1, "Fade")]);

    assert_eq!(store.images[0].selected_profile_index, 0);
    assert_eq!(store.images[0].publish_profile_indexes, Some(vec![0, 1]));
    assert_eq!(store.images[0].profiles[0].profile_stem, "Classic");
    assert_eq!(store.images[0].profiles[0].status, ReviewRenderStatus::Done);
    assert_eq!(store.images[0].profiles[1].profile_stem, "Fade");
    assert_eq!(
        store.images[0].profiles[1].status,
        ReviewRenderStatus::Missing
    );
}

#[test]
fn sync_profiles_drops_same_stem_render_when_profile_identity_changes() {
    let old_profile = profile(0, "Classic");
    let mut new_profile = profile(0, "Classic");
    new_profile.retouch_base.exposure = 0.25;
    let mut store = ReviewStore::new(vec![old_profile]);
    store.images.push(ReviewImage {
        id: 1,
        raw_path: PathBuf::from("/in/frame.NEF"),
        relative_path: "frame.NEF".to_string(),
        file_name: "frame.NEF".to_string(),
        exif: GalleryExifData::default(),
        preview: ReviewPreview::default(),
        selected_profile_index: 0,
        rating: 0,
        label: ReviewLabel::None,
        labels: Vec::new(),
        tags: Vec::new(),
        notes: String::new(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![0]),
        profiles: vec![ReviewProfileRender {
            output_path: Some(PathBuf::from("/out/Classic/frame.jpg")),
            ..profile_render(0, "Classic")
        }],
        updated_at: now_string(),
    });

    store.sync_profiles(vec![new_profile]);

    assert_eq!(store.images[0].profiles[0].profile_stem, "Classic");
    assert_eq!(
        store.images[0].profiles[0].status,
        ReviewRenderStatus::Missing
    );
    assert_eq!(store.images[0].profiles[0].output_path, None);
}

#[test]
fn sync_profiles_preserves_publish_selection_when_profiles_are_unchanged() {
    let profiles = vec![profile(0, "Classic"), profile(1, "Fade")];
    let mut store = ReviewStore::new(profiles.clone());
    store.images.push(ReviewImage {
        id: 1,
        raw_path: PathBuf::from("/in/frame.NEF"),
        relative_path: "frame.NEF".to_string(),
        file_name: "frame.NEF".to_string(),
        exif: GalleryExifData::default(),
        preview: ReviewPreview::default(),
        selected_profile_index: 1,
        rating: 0,
        label: ReviewLabel::None,
        labels: Vec::new(),
        tags: Vec::new(),
        notes: String::new(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![1]),
        profiles: vec![profile_render(0, "Classic"), profile_render(1, "Fade")],
        updated_at: now_string(),
    });

    store.sync_profiles(profiles);

    assert_eq!(store.images[0].selected_profile_index, 1);
    assert_eq!(store.images[0].publish_profile_indexes, Some(vec![1]));
    assert_eq!(store.images[0].profiles[1].status, ReviewRenderStatus::Done);
}

#[test]
fn base_render_done_triggers_pending_retouch_without_marking_done() {
    let output = PathBuf::from("frame.jpg");
    let mut render = ReviewProfileRender {
        profile_index: 0,
        profile_stem: "Classic".to_string(),
        status: ReviewRenderStatus::Queued,
        output_path: None,
        error: Some("old".to_string()),
        duration_ms: None,
        render_key: Some("retouch-key".to_string()),
        updated_at: now_string(),
    };

    let key = apply_base_render_done(&mut render, &output, Duration::from_millis(42));

    assert_eq!(key.as_deref(), Some("retouch-key"));
    assert_eq!(render.status, ReviewRenderStatus::Queued);
    assert_eq!(render.output_path.as_deref(), Some(output.as_path()));
    assert_eq!(render.error, None);
    assert_eq!(render.duration_ms, Some(42));

    render.render_key = None;
    let key = apply_base_render_done(&mut render, &output, Duration::from_millis(7));
    assert_eq!(key, None);
    assert_eq!(render.status, ReviewRenderStatus::Done);
    assert_eq!(render.duration_ms, Some(7));
}

#[test]
fn queued_missing_output_reuses_saved_retouch_settings() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let raw = input.join("frame.NEF");
    let rendered = output.join("Classic").join("frame.jpg");
    fs::write(&raw, b"raw").unwrap();

    let handle = test_handle(input, output, vec![profile(0, "Classic")]);
    handle.record_discovered_raw(&raw).unwrap();
    let saved_retouch = RetouchSettings {
        adjustments: BasicRetouchAdjustments {
            exposure: 0.35,
            clarity: 12.0,
            ..BasicRetouchAdjustments::default()
        },
        ..RetouchSettings::default()
    }
    .normalized();
    let expected_key = saved_retouch.render_key();
    {
        let mut store = handle.lock_store().unwrap();
        let image = store
            .images
            .iter_mut()
            .find(|image| image.raw_path == raw)
            .unwrap();
        image.retouch = saved_retouch;
    }

    handle.record_profile_queued(&raw, 0, &rendered).unwrap();
    {
        let store = handle.lock_store().unwrap();
        let render = &store.images[0].profiles[0];
        assert_eq!(render.status, ReviewRenderStatus::Queued);
        assert_eq!(render.output_path.as_deref(), Some(rendered.as_path()));
        assert_eq!(render.render_key.as_deref(), Some(expected_key.as_str()));
    }

    handle
        .record_profile_done(&raw, 0, &rendered, Duration::from_millis(42))
        .unwrap();
    let job = handle.retouch_scheduler.next_job();
    assert_eq!(job.raw, raw);
    assert_eq!(job.profile_index, 0);
    assert_eq!(job.output, rendered);
    assert_eq!(job.render_key, expected_key);
}

#[test]
fn retouch_scheduler_coalesces_same_raw_profile_to_latest_job() {
    let scheduler = ReviewRetouchScheduler::default();
    scheduler.schedule_after(
        PathBuf::from("frame.NEF"),
        1,
        PathBuf::from("old.jpg"),
        "old".to_string(),
        Duration::ZERO,
    );
    scheduler.schedule_after(
        PathBuf::from("frame.NEF"),
        1,
        PathBuf::from("new.jpg"),
        "new".to_string(),
        Duration::ZERO,
    );

    let job = scheduler.next_job();

    assert_eq!(job.raw, PathBuf::from("frame.NEF"));
    assert_eq!(job.profile_index, 1);
    assert_eq!(job.output, PathBuf::from("new.jpg"));
    assert_eq!(job.render_key, "new");
}

#[test]
fn normalize_review_labels_removes_none_and_keeps_display_order() {
    assert_eq!(
        normalize_review_labels([
            ReviewLabel::Purple,
            ReviewLabel::None,
            ReviewLabel::Red,
            ReviewLabel::Purple,
        ]),
        vec![ReviewLabel::Red, ReviewLabel::Purple]
    );
}

#[test]
fn review_update_advances_shared_server_ui_state() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(input.join("day")).unwrap();
    fs::create_dir_all(&output).unwrap();
    let first = input.join("day").join("frame-1.NEF");
    let second = input.join("day").join("frame-2.NEF");
    fs::write(&first, b"raw").unwrap();
    fs::write(&second, b"raw").unwrap();

    let handle = test_handle(
        input,
        output,
        vec![profile(0, "Classic"), profile(1, "Fade")],
    );

    handle.record_discovered_raw(&first).unwrap();
    handle.record_discovered_raw(&second).unwrap();
    handle
        .apply_review_update(ReviewUpdateRequest {
            image_id: 1,
            rating: 1,
            label: ReviewLabel::Green,
            labels: vec![ReviewLabel::Green],
            tags: vec!["keep".to_string()],
            notes: String::new(),
            retouch: None,
            selected_profile_index: 0,
            publish_profile_indexes: Some(vec![0, 1]),
            advance_after_update: true,
        })
        .unwrap();

    let state =
        serde_json::from_str::<serde_json::Value>(&handle.api_state_json().unwrap()).unwrap();
    assert_eq!(state["ui"]["current_image_id"], 2);
    assert_eq!(state["ui"]["min_rating"], 0);

    handle
        .apply_review_update(ReviewUpdateRequest {
            image_id: 2,
            rating: 0,
            label: ReviewLabel::None,
            labels: Vec::new(),
            tags: Vec::new(),
            notes: String::new(),
            retouch: None,
            selected_profile_index: 0,
            publish_profile_indexes: Some(vec![0, 1]),
            advance_after_update: true,
        })
        .unwrap();

    let state =
        serde_json::from_str::<serde_json::Value>(&handle.api_state_json().unwrap()).unwrap();
    assert_eq!(state["ui"]["current_image_id"], 1);
    assert_eq!(state["ui"]["min_rating"], 1);
}

#[test]
fn review_history_records_review_and_publish_state_changes() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(input.join("day")).unwrap();
    fs::create_dir_all(&output).unwrap();
    let raw = input.join("day").join("frame-1.NEF");
    fs::write(&raw, b"raw").unwrap();

    let handle = test_handle(
        input,
        output.clone(),
        vec![profile(0, "Classic"), profile(1, "Fade")],
    );

    handle.record_discovered_raw(&raw).unwrap();
    handle
        .apply_review_update(ReviewUpdateRequest {
            image_id: 1,
            rating: 4,
            label: ReviewLabel::Red,
            labels: vec![ReviewLabel::Red],
            tags: vec!["keep".to_string()],
            notes: "publish candidate".to_string(),
            retouch: None,
            selected_profile_index: 1,
            publish_profile_indexes: Some(vec![1]),
            advance_after_update: false,
        })
        .unwrap();
    handle
        .apply_ui_update(ReviewUiUpdateRequest {
            current_image_id: Some(1),
            min_rating: 3,
        })
        .unwrap();

    handle.publish_jobs.lock().unwrap().push(ReviewPublishJob {
        id: 1,
        album: "finals".to_string(),
        status: ReviewPublishJobStatus::Running,
        started_at: now_string(),
        finished_at: None,
        processed: 0,
        total: 0,
        step: "starting".to_string(),
        current: None,
        linked: 0,
        skipped: 0,
        galleries: 0,
        error: None,
    });
    handle
        .record_publish_job_progress(
            1,
            &ReviewPublishProgress {
                processed: 1,
                total: 2,
                linked: 1,
                skipped: 0,
                galleries: 0,
                step: "link".to_string(),
                current: Some("frame-1.jpg".to_string()),
            },
        )
        .unwrap();
    handle
        .record_publish_job_done(
            1,
            &PublishReport {
                linked: 1,
                skipped: 1,
                min_rating: 3,
                galleries: 0,
                gallery_roots: Vec::new(),
            },
        )
        .unwrap();

    let history = fs::read_to_string(output.join("history.txt")).unwrap();
    assert!(history.contains("review image day/frame-1.NEF #1"));
    assert!(history.contains("review metadata changed day/frame-1.NEF #1"));
    assert!(history.contains("rating: 0 -> 4"));
    assert!(history.contains("labels: none -> red"));
    assert!(history.contains("tags: none -> keep"));
    assert!(history.contains("selected profile: 0:Classic -> 1:Fade"));
    assert!(history.contains("review UI changed"));
    assert!(history.contains("minimum rating: 0 -> 3"));
    assert!(history.contains("review publish job #1 changed"));
    assert!(history.contains("processed: 0 -> 1"));
    assert!(history.contains("status: running -> done"));
}

#[test]
fn review_state_reports_connected_client_count() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();

    let handle = test_handle(input, output, vec![profile(0, "Classic")]);

    let state =
        serde_json::from_str::<serde_json::Value>(&handle.api_state_json().unwrap()).unwrap();
    assert_eq!(state["client_count"], 0);

    let client = handle.subscribe();
    let state =
        serde_json::from_str::<serde_json::Value>(&handle.api_state_json().unwrap()).unwrap();
    assert_eq!(state["client_count"], 1);

    drop(client);
    handle.broadcast_state().unwrap();
    let state =
        serde_json::from_str::<serde_json::Value>(&handle.api_state_json().unwrap()).unwrap();
    assert_eq!(state["client_count"], 0);
}

#[test]
fn review_route_path_accepts_reverse_proxy_prefixes() {
    assert_eq!(review_route_path("/api/state"), "/api/state");
    assert_eq!(review_route_path("/mini-film/api/state"), "/api/state");
    assert_eq!(
        review_route_path("/nested/mini-film/assets/app.js"),
        "/assets/app.js"
    );
    assert_eq!(
        review_route_path("/nested/mini-film/assets/vendor/preact.module.js"),
        "/assets/vendor/preact.module.js"
    );
    assert_eq!(review_route_path("/mini-film/media/1/0"), "/media/1/0");
    assert_eq!(review_route_path("/mini-film/preview/1"), "/preview/1");
    assert_eq!(review_route_path("/mini-film/review"), "/review");
    assert_eq!(review_route_path("/mini-film/"), "/");
}

#[test]
fn review_vendor_assets_are_embedded_as_javascript() {
    assert!(review_text_asset("vendor/preact.module.js").is_some());
    assert!(review_text_asset("vendor/hooks.module.js").is_some());
    assert_eq!(
        review_asset_content_type("vendor/preact.module.js"),
        "application/javascript; charset=utf-8"
    );
}

#[test]
fn publish_flat_album_filters_rating_label_and_tag() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("out");
    let source = output.join("day").join("Classic").join("frame.jpg");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, b"jpg").unwrap();

    let mut store = ReviewStore::new(vec![profile(0, "Classic")]);
    store.images.push(ReviewImage {
        id: 1,
        raw_path: PathBuf::from("/in/day/frame.NEF"),
        relative_path: "day/frame.NEF".to_string(),
        file_name: "frame.NEF".to_string(),
        exif: GalleryExifData::default(),
        selected_profile_index: 0,
        rating: 3,
        label: ReviewLabel::Red,
        labels: vec![ReviewLabel::Red],
        tags: vec!["42".to_string()],
        notes: "keeper".to_string(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![0]),
        preview: ReviewPreview::default(),
        profiles: vec![ReviewProfileRender {
            profile_index: 0,
            profile_stem: "Classic".to_string(),
            status: ReviewRenderStatus::Done,
            output_path: Some(source.clone()),
            error: None,
            duration_ms: Some(1),
            render_key: None,
            updated_at: now_string(),
        }],
        updated_at: now_string(),
    });

    let mut options = test_publish_options("published/final");
    options.labels = HashSet::from([ReviewLabel::Red]);
    options.tags = HashSet::from(["42".to_string()]);
    let report = publish_store_inner(&store, Path::new("/in"), &output, &options, None).unwrap();
    assert_eq!(report.linked, 1);
    assert_eq!(report.skipped, 0);
    assert!(output.join("published/final/frame.jpg").exists());
}

#[test]
fn publish_flat_album_suffixes_non_default_profiles() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("out");
    let classic = output.join("day").join("Classic").join("frame.jpg");
    let fade = output.join("day").join("Fade").join("frame.jpg");
    fs::create_dir_all(classic.parent().unwrap()).unwrap();
    fs::create_dir_all(fade.parent().unwrap()).unwrap();
    fs::write(&classic, b"classic").unwrap();
    fs::write(&fade, b"fade").unwrap();

    let mut store = ReviewStore::new(vec![profile(0, "Classic"), profile(1, "Fade")]);
    store.images.push(ReviewImage {
        id: 1,
        raw_path: PathBuf::from("/in/day/frame.NEF"),
        relative_path: "day/frame.NEF".to_string(),
        file_name: "frame.NEF".to_string(),
        exif: GalleryExifData::default(),
        selected_profile_index: 0,
        rating: 2,
        label: ReviewLabel::None,
        labels: Vec::new(),
        tags: Vec::new(),
        notes: String::new(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![1]),
        preview: ReviewPreview::default(),
        profiles: vec![
            ReviewProfileRender {
                profile_index: 0,
                profile_stem: "Classic".to_string(),
                status: ReviewRenderStatus::Done,
                output_path: Some(classic.clone()),
                error: None,
                duration_ms: Some(1),
                render_key: None,
                updated_at: now_string(),
            },
            ReviewProfileRender {
                profile_index: 1,
                profile_stem: "Fade".to_string(),
                status: ReviewRenderStatus::Done,
                output_path: Some(fade.clone()),
                error: None,
                duration_ms: Some(1),
                render_key: None,
                updated_at: now_string(),
            },
        ],
        updated_at: now_string(),
    });

    let options = test_publish_options("published");
    let report = publish_store_inner(&store, Path::new("/in"), &output, &options, None).unwrap();
    assert_eq!(report.linked, 1);
    assert!(!output.join("published/frame.jpg").exists());
    assert!(output.join("published/frame-Fade.jpg").exists());
}

#[test]
fn publish_store_reports_realtime_progress() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("out");
    let classic = output.join("day").join("Classic").join("frame.jpg");
    let fade = output.join("day").join("Fade").join("frame.jpg");
    fs::create_dir_all(classic.parent().unwrap()).unwrap();
    fs::create_dir_all(fade.parent().unwrap()).unwrap();
    fs::write(&classic, b"classic").unwrap();
    fs::write(&fade, b"fade").unwrap();

    let mut store = ReviewStore::new(vec![profile(0, "Classic"), profile(1, "Fade")]);
    store.images.push(ReviewImage {
        id: 1,
        raw_path: PathBuf::from("/in/day/frame.NEF"),
        relative_path: "day/frame.NEF".to_string(),
        file_name: "frame.NEF".to_string(),
        exif: GalleryExifData::default(),
        selected_profile_index: 0,
        rating: 5,
        label: ReviewLabel::None,
        labels: Vec::new(),
        tags: Vec::new(),
        notes: String::new(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![0, 1]),
        preview: ReviewPreview::default(),
        profiles: vec![
            ReviewProfileRender {
                profile_index: 0,
                profile_stem: "Classic".to_string(),
                status: ReviewRenderStatus::Done,
                output_path: Some(classic.clone()),
                error: None,
                duration_ms: Some(1),
                render_key: None,
                updated_at: now_string(),
            },
            ReviewProfileRender {
                profile_index: 1,
                profile_stem: "Fade".to_string(),
                status: ReviewRenderStatus::Done,
                output_path: Some(fade.clone()),
                error: None,
                duration_ms: Some(1),
                render_key: None,
                updated_at: now_string(),
            },
        ],
        updated_at: now_string(),
    });

    let events = Mutex::new(Vec::new());
    let progress = |event: ReviewPublishProgress| {
        events.lock().unwrap().push(event);
    };
    let options = test_publish_options("published");
    let report =
        publish_store_inner(&store, Path::new("/in"), &output, &options, Some(&progress)).unwrap();
    let events = events.lock().unwrap();
    assert_eq!(report.linked, 2);
    assert!(events.iter().any(|event| event.total == 2));
    assert!(events.iter().any(|event| event.processed == 2));
    assert!(events.iter().any(|event| event.step == "link"));
}

#[test]
fn short_path_sha1_is_stable_and_short() {
    let first = short_path_sha1(Path::new("/tmp/frame.NEF"));
    assert_eq!(first, short_path_sha1(Path::new("/tmp/frame.NEF")));
    assert_ne!(first, short_path_sha1(Path::new("/tmp/other.NEF")));
    assert_eq!(first.len(), 16);
}

#[test]
fn jpeg_detection_requires_marker() {
    assert!(looks_like_jpeg(&[0xff, 0xd8, 0xff, 0xee]));
    assert!(!looks_like_jpeg(b"not jpeg"));
}
