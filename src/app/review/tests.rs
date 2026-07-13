use super::prelude::*;
use super::{
    db::*, handle::*, model::*, preview::*, publish::*, scheduler::*, server::*, store::*,
};
use std::sync::Mutex;

fn profile(index: usize, stem: &str) -> ReviewProfile {
    ReviewProfile {
        index,
        selector: stem.to_string(),
        stem: stem.to_string(),
        retouch_base: BasicRetouchAdjustments::default(),
        hald_path: None,
        metadata: None,
    }
}

fn populated_profile_adjustments(seed: f32) -> ReviewProfileAdjustments {
    ReviewProfileAdjustments {
        exposure: seed + 0.1,
        contrast: seed + 0.2,
        highlights: seed + 0.3,
        shadows: seed + 0.4,
        whites: seed + 0.5,
        blacks: seed + 0.6,
        saturation: seed + 0.7,
        vibrance: seed + 0.8,
        clarity: seed + 0.9,
        parametric: ReviewProfileParametricTone {
            shadows: seed + 1.0,
            darks: seed + 1.1,
            lights: seed + 1.2,
            highlights: seed + 1.3,
            shadow_split: seed + 20.0,
            midtone_split: seed + 50.0,
            highlight_split: seed + 80.0,
        },
        hsl: ReviewProfileHslAdjustments {
            hue: vec![seed + 2.0, seed + 2.1],
            saturation: vec![seed + 2.2],
            luminance: vec![seed + 2.3, seed + 2.4, seed + 2.5],
        },
        calibration: ReviewProfileCalibration {
            red_hue: seed + 3.0,
            red_saturation: seed + 3.1,
            green_hue: seed + 3.2,
            green_saturation: seed + 3.3,
            blue_hue: seed + 3.4,
            blue_saturation: seed + 3.5,
        },
        tone_curve: ReviewProfileToneCurves {
            composite: vec![[0.0, seed + 0.01], [1.0, seed + 0.99]],
            red: vec![[0.25, seed + 0.3]],
            green: vec![[0.5, seed + 0.6]],
            blue: vec![[0.75, seed + 0.8]],
        },
    }
}

fn fully_populated_review_store() -> ReviewStore {
    let mut detailed_profile = profile(7, "Detailed");
    detailed_profile.selector = "profiles/detailed.xmp".to_string();
    detailed_profile.retouch_base = BasicRetouchAdjustments {
        exposure: 0.25,
        highlights: -10.0,
        shadows: 11.0,
        whites: 12.0,
        blacks: -13.0,
        temperature: 140.0,
        offset: -2.0,
        clarity: 15.0,
    };
    detailed_profile.metadata = Some(ReviewProfileMetadata {
        profile_name: "Detailed Profile".to_string(),
        profile_uuid: Some("profile-uuid".to_string()),
        look_name: Some("Detailed Look".to_string()),
        look_uuid: Some("look-uuid".to_string()),
        source_profile_name: Some("Source Profile".to_string()),
        source_profile_uuid: Some("source-uuid".to_string()),
        source_adjustments: populated_profile_adjustments(1.0),
        source_sharpening: ReviewProfileSharpening {
            present: true,
            amount: 1.1,
            radius: 1.2,
            detail: 1.3,
            masking: 1.4,
        },
        emulation_adjustments: populated_profile_adjustments(10.0),
        emulation_sharpening: ReviewProfileSharpening {
            present: false,
            amount: 2.1,
            radius: 2.2,
            detail: 2.3,
            masking: 2.4,
        },
        has_camera_raw_settings: true,
        grain: Some(ReviewProfileGrain {
            amount: 31,
            size: 42,
            frequency: 53,
        }),
        has_hald: true,
        has_pp3: true,
        pp3_name: Some("detailed.pp3".to_string()),
        pp3_adjustments: vec![
            ReviewProfilePp3Section {
                source: "Empty PP3".to_string(),
                section: "Empty".to_string(),
                entries: Vec::new(),
            },
            ReviewProfilePp3Section {
                source: "Profile PP3".to_string(),
                section: "Exposure".to_string(),
                entries: vec![
                    ReviewProfilePp3Entry {
                        key: "Compensation".to_string(),
                        value: "0.5".to_string(),
                    },
                    ReviewProfilePp3Entry {
                        key: "Black".to_string(),
                        value: "-100".to_string(),
                    },
                ],
            },
        ],
    });
    let plain_profile = profile(9, "Plain");
    let timestamp = "2026-07-13T12:34:56+02:00".to_string();
    let detailed_render = ReviewProfileRender {
        profile_index: 7,
        profile_stem: "Detailed".to_string(),
        display_name: Some("Detailed display".to_string()),
        status: ReviewRenderStatus::Failed,
        output_path: Some(PathBuf::from("/out/detailed.jpg")),
        error: Some("render failed".to_string()),
        duration_ms: Some(987),
        render_key: Some("render-key".to_string()),
        processing_key: Some("processing-key".to_string()),
        width: Some(4096),
        height: Some(2731),
        updated_at: timestamp.clone(),
    };
    let mut store = ReviewStore::new(vec![detailed_profile, plain_profile]);
    store.next_id = 4;
    store.ui = ReviewUiState {
        current_image_id: Some(2),
        min_rating: 3,
    };
    store.exif_schema_version = 9;
    store.images = vec![
        ReviewImage {
            id: 1,
            raw_path: PathBuf::from("/in/one.NEF"),
            sooc_sidecar_path: Some(PathBuf::from("/in/one.JPG")),
            relative_path: "day/one.NEF".to_string(),
            file_name: "one.NEF".to_string(),
            exif: GalleryExifData {
                capture_timestamp: Some(-123_456_789),
                rating: Some(4),
                file_size_bytes: Some(55_620_945),
                image_width: Some(8288),
                image_height: Some(5520),
                focal_length: Some("85 mm".to_string()),
                aperture: Some("f/1.8".to_string()),
                shutter_speed: Some("1/250".to_string()),
                iso: Some("640".to_string()),
                auto_iso: Some(true),
                iso_auto_hi_limit: Some("ISO 6400".to_string()),
                white_balance_mode: Some("Auto1".to_string()),
                white_balance_temperature: Some(4860),
                white_balance_offset: Some(-2),
                camera_model: Some("Nikon Z8".to_string()),
                shutter_count: Some(66_278),
                shutter_mode: Some("Auto (Electronic Front Curtain)".to_string()),
                silent_photography: Some(true),
                release_mode: Some("Single Frame".to_string()),
                lens_model: Some("NIKKOR Z 85mm".to_string()),
                shooting_mode: Some("Manual".to_string()),
                exposure_compensation: Some("-0.7 EV".to_string()),
                flash: Some("Off".to_string()),
                active_d_lighting: Some("High".to_string()),
                tags: vec!["camera".to_string(), "camera".to_string()],
                note: Some("camera note".to_string()),
            },
            preview: ReviewPreview {
                status: ReviewRenderStatus::Failed,
                path: Some(PathBuf::from("/out/preview.jpg")),
                error: Some("preview failed".to_string()),
                duration_ms: Some(321),
                render_key: Some("preview-key".to_string()),
                updated_at: timestamp.clone(),
            },
            selected_profile_index: 7,
            rating: 5,
            label: ReviewLabel::Purple,
            labels: vec![ReviewLabel::Red, ReviewLabel::None, ReviewLabel::Red],
            tags: vec!["keeper".to_string(), "keeper".to_string()],
            notes: "manual note".to_string(),
            rating_source: ReviewMetadataSource::Camera,
            tags_source: ReviewMetadataSource::Codex,
            notes_source: ReviewMetadataSource::Manual,
            codex: ReviewCodexAnalysis {
                status: ReviewCodexStatus::Failed,
                flags: CodexAnalysisFlags::all(),
                model: "gpt-mini".to_string(),
                analysis_key: Some("analysis-key".to_string()),
                error: Some("analysis failed".to_string()),
                updated_at: timestamp.clone(),
            },
            retouch: RetouchSettings {
                adjustments: BasicRetouchAdjustments {
                    exposure: 0.75,
                    highlights: -20.0,
                    shadows: 21.0,
                    whites: 22.0,
                    blacks: -23.0,
                    temperature: 240.0,
                    offset: -4.0,
                    clarity: 25.0,
                },
                crop: Some(crate::app::retouch::RetouchCrop {
                    x: 0.1,
                    y: 0.2,
                    width: 0.7,
                    height: 0.6,
                }),
                rotation_degrees: 1.25,
            },
            publish_profile_indexes: None,
            profile_bw_filters: vec![
                ReviewProfileBwFilter {
                    profile_index: 7,
                    filter: BwFilter::None,
                },
                ReviewProfileBwFilter {
                    profile_index: 7,
                    filter: BwFilter::Yellow,
                },
                ReviewProfileBwFilter {
                    profile_index: 7,
                    filter: BwFilter::Red,
                },
            ],
            profiles: vec![detailed_render],
            updated_at: timestamp.clone(),
        },
        ReviewImage {
            id: 2,
            raw_path: PathBuf::from("/in/two.jpg"),
            sooc_sidecar_path: None,
            relative_path: "two.jpg".to_string(),
            file_name: "two.jpg".to_string(),
            exif: GalleryExifData::default(),
            preview: ReviewPreview::default(),
            selected_profile_index: 9,
            rating: 3,
            label: ReviewLabel::None,
            labels: Vec::new(),
            tags: Vec::new(),
            notes: String::new(),
            rating_source: ReviewMetadataSource::Default,
            tags_source: ReviewMetadataSource::Default,
            notes_source: ReviewMetadataSource::Default,
            codex: ReviewCodexAnalysis::default(),
            retouch: RetouchSettings::default(),
            publish_profile_indexes: Some(Vec::new()),
            profile_bw_filters: Vec::new(),
            profiles: Vec::new(),
            updated_at: timestamp.clone(),
        },
        ReviewImage {
            id: 3,
            raw_path: PathBuf::from("/in/three.NEF"),
            sooc_sidecar_path: None,
            relative_path: "three.NEF".to_string(),
            file_name: "three.NEF".to_string(),
            exif: GalleryExifData::default(),
            preview: ReviewPreview::default(),
            selected_profile_index: 7,
            rating: 4,
            label: ReviewLabel::Green,
            labels: vec![ReviewLabel::Green],
            tags: vec!["publish".to_string()],
            notes: "publish selected profiles".to_string(),
            rating_source: ReviewMetadataSource::Manual,
            tags_source: ReviewMetadataSource::Manual,
            notes_source: ReviewMetadataSource::Manual,
            codex: ReviewCodexAnalysis::default(),
            retouch: RetouchSettings::default(),
            publish_profile_indexes: Some(vec![7, SOOC_PROFILE_INDEX]),
            profile_bw_filters: Vec::new(),
            profiles: Vec::new(),
            updated_at: timestamp,
        },
    ];
    store
}

fn create_v5_review_database(path: &Path, store: &ReviewStore) {
    let connection = open_database_at_version_for_test(path, 5).unwrap();
    let store_json = serde_json::to_string_pretty(store).unwrap();
    connection
        .execute(
            "INSERT INTO review_state(
                id, next_id, current_image_id, min_rating, exif_schema_version,
                store_json, store_sha1, updated_at
             ) VALUES (1, 1, NULL, 0, 4, ?1, 'test-sha1', ?2)",
            rusqlite::params![store_json, now_string()],
        )
        .unwrap();
}

fn bw_profile(
    index: usize,
    stem: &str,
    source_saturation: f32,
    emulation_saturation: f32,
) -> ReviewProfile {
    let mut profile = profile(index, stem);
    let mut metadata = ReviewProfileMetadata::default();
    metadata.source_adjustments.saturation = source_saturation;
    metadata.emulation_adjustments.saturation = emulation_saturation;
    profile.metadata = Some(metadata);
    profile
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
        state: Arc::new(ArcSwap::from_pointee(ReviewStore::new(profiles))),
        subscribers: Arc::new(subscribers),
        state_cache: Arc::new(ArcSwapOption::empty()),
        state_path: output.join(SQLITE_STATE_FILE),
        input_root: input.clone(),
        output_root: output.clone(),
        hald_dir: output.join("hald"),
        profiles_root: input.clone(),
        hald_level: 16,
        rawtherapee: PathBuf::from("rawtherapee-cli"),
        output_format: BatchOutputFormat::Jpg,
        lcp_root: None,
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
        grain_engine: mini_film::GrainEngine::default(),
        publish_defaults: ReviewPublishDefaults::new(
            "published".to_string(),
            BatchOutputFormat::Jpg,
            &export,
            ReviewGalleryDefaults {
                template: None,
                thumbnail_long_edge: 1024,
                columns: 4,
            },
            mini_film::GrainEngine::default(),
        ),
        publish_jobs: Arc::new(ArcSwap::from_pointee(Vec::new())),
        next_publish_job_id: Arc::new(AtomicU64::new(1)),
        media_scheduler: Arc::new(ReviewMediaScheduler::default()),
        retouch_scheduler: Arc::new(ReviewRetouchScheduler::default()),
        codex: None,
        codex_scheduler: Arc::new(ReviewCodexScheduler::default()),
        invocation: None,
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
        lcp_root: None,
        export: test_export_options(),
        rerender_raw: false,
        no_grain: false,
        color_noise_iso_threshold: 1600,
        lens_corrections: LensCorrections::default(),
        grain: None,
        grain_preset: None,
        grain_seed: Some(1),
        grain_engine: mini_film::GrainEngine::default(),
        write_metadata: false,
    }
}

fn profile_render(index: usize, stem: &str) -> ReviewProfileRender {
    ReviewProfileRender {
        profile_index: index,
        profile_stem: stem.to_string(),
        display_name: None,
        status: ReviewRenderStatus::Done,
        output_path: None,
        error: None,
        duration_ms: Some(1),
        render_key: None,
        processing_key: Some(review_render_processing_key(index).to_string()),
        width: None,
        height: None,
        updated_at: now_string(),
    }
}

#[test]
fn profile_bw_filter_eligibility_uses_combined_saturation() {
    let eligible = bw_profile(0, "BW", -60.0, -40.0);
    let ineligible = bw_profile(1, "Color", -98.0, 0.0);
    let renders = vec![profile_render(0, "BW"), profile_render(1, "Color")];
    let image = ReviewImage {
        id: 1,
        raw_path: PathBuf::from("/in/frame.NEF"),
        sooc_sidecar_path: None,
        relative_path: "frame.NEF".to_string(),
        file_name: "frame.NEF".to_string(),
        exif: GalleryExifData::default(),
        selected_profile_index: 0,
        rating: 0,
        label: ReviewLabel::None,
        labels: Vec::new(),
        tags: Vec::new(),
        notes: String::new(),
        rating_source: ReviewMetadataSource::Default,
        tags_source: ReviewMetadataSource::Default,
        notes_source: ReviewMetadataSource::Default,
        codex: ReviewCodexAnalysis::default(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![0, 1]),
        profile_bw_filters: normalize_profile_bw_filters(
            &[
                ReviewProfileBwFilter {
                    profile_index: 2,
                    filter: BwFilter::Red,
                },
                ReviewProfileBwFilter {
                    profile_index: 0,
                    filter: BwFilter::None,
                },
                ReviewProfileBwFilter {
                    profile_index: 1,
                    filter: BwFilter::Orange,
                },
                ReviewProfileBwFilter {
                    profile_index: 0,
                    filter: BwFilter::Yellow,
                },
            ],
            &renders,
        ),
        preview: ReviewPreview::default(),
        profiles: renders,
        updated_at: now_string(),
    };

    assert!(review_profile_bw_filter_eligible(&eligible));
    assert!(!review_profile_bw_filter_eligible(&ineligible));
    assert_eq!(
        image.profile_bw_filters,
        vec![
            ReviewProfileBwFilter {
                profile_index: 0,
                filter: BwFilter::Yellow,
            },
            ReviewProfileBwFilter {
                profile_index: 1,
                filter: BwFilter::Orange,
            },
        ]
    );
    assert_eq!(
        effective_bw_filter_for_profile(&image, &eligible),
        BwFilter::Yellow
    );
    assert_eq!(
        effective_bw_filter_for_profile(&image, &ineligible),
        BwFilter::None
    );
}

#[test]
fn preferred_preview_profile_keeps_selected_profile_even_when_publish_unchecked() {
    let image = ReviewImage {
        id: 1,
        raw_path: PathBuf::from("/in/frame.NEF"),
        sooc_sidecar_path: None,
        relative_path: "frame.NEF".to_string(),
        file_name: "frame.NEF".to_string(),
        exif: GalleryExifData::default(),
        selected_profile_index: 2,
        rating: 0,
        label: ReviewLabel::None,
        labels: Vec::new(),
        tags: Vec::new(),
        notes: String::new(),
        rating_source: ReviewMetadataSource::Default,
        tags_source: ReviewMetadataSource::Default,
        notes_source: ReviewMetadataSource::Default,
        codex: ReviewCodexAnalysis::default(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![1]),
        profile_bw_filters: Vec::new(),
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
        Some(2)
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
        sooc_sidecar_path: None,
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
        rating_source: ReviewMetadataSource::Default,
        tags_source: ReviewMetadataSource::Default,
        notes_source: ReviewMetadataSource::Default,
        codex: ReviewCodexAnalysis::default(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![0]),
        profile_bw_filters: Vec::new(),
        profiles: vec![profile_render(0, "Classic")],
        updated_at: now_string(),
    });
    store.images.push(ReviewImage {
        id: 2,
        raw_path: PathBuf::from("/in/camera-b/early.NEF"),
        sooc_sidecar_path: None,
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
        rating_source: ReviewMetadataSource::Default,
        tags_source: ReviewMetadataSource::Default,
        notes_source: ReviewMetadataSource::Default,
        codex: ReviewCodexAnalysis::default(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![0]),
        profile_bw_filters: Vec::new(),
        profiles: vec![profile_render(0, "Classic")],
        updated_at: now_string(),
    });
    store.images.push(ReviewImage {
        id: 3,
        raw_path: PathBuf::from("/in/camera-c/no-exif.NEF"),
        sooc_sidecar_path: None,
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
        rating_source: ReviewMetadataSource::Default,
        tags_source: ReviewMetadataSource::Default,
        notes_source: ReviewMetadataSource::Default,
        codex: ReviewCodexAnalysis::default(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![0]),
        profile_bw_filters: Vec::new(),
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
        sooc_sidecar_path: None,
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
        rating_source: ReviewMetadataSource::Manual,
        tags_source: ReviewMetadataSource::Manual,
        notes_source: ReviewMetadataSource::Manual,
        codex: ReviewCodexAnalysis::default(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![0]),
        profile_bw_filters: Vec::new(),
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
        sooc_sidecar_path: None,
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
        rating_source: ReviewMetadataSource::Default,
        tags_source: ReviewMetadataSource::Default,
        notes_source: ReviewMetadataSource::Default,
        codex: ReviewCodexAnalysis::default(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![0]),
        profile_bw_filters: Vec::new(),
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
        sooc_sidecar_path: None,
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
        rating_source: ReviewMetadataSource::Default,
        tags_source: ReviewMetadataSource::Default,
        notes_source: ReviewMetadataSource::Default,
        codex: ReviewCodexAnalysis::default(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![0]),
        profile_bw_filters: Vec::new(),
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
        sooc_sidecar_path: None,
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
        rating_source: ReviewMetadataSource::Default,
        tags_source: ReviewMetadataSource::Default,
        notes_source: ReviewMetadataSource::Default,
        codex: ReviewCodexAnalysis::default(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![1]),
        profile_bw_filters: Vec::new(),
        profiles: vec![profile_render(0, "Classic"), profile_render(1, "Fade")],
        updated_at: now_string(),
    });

    store.sync_profiles(profiles);

    assert_eq!(store.images[0].selected_profile_index, 1);
    assert_eq!(store.images[0].publish_profile_indexes, Some(vec![1]));
    assert_eq!(store.images[0].profiles[1].status, ReviewRenderStatus::Done);
}

#[test]
fn sync_profiles_invalidates_renders_from_old_processing_pipeline() {
    let profiles = vec![profile(0, "Classic")];
    let mut render = profile_render(0, "Classic");
    render.output_path = Some(PathBuf::from("/out/Classic/frame.jpg"));
    render.processing_key = Some("raw-render-v1".to_string());
    let mut store = ReviewStore::new(profiles.clone());
    store.images.push(ReviewImage {
        id: 1,
        raw_path: PathBuf::from("/in/frame.NEF"),
        sooc_sidecar_path: None,
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
        rating_source: ReviewMetadataSource::Default,
        tags_source: ReviewMetadataSource::Default,
        notes_source: ReviewMetadataSource::Default,
        codex: ReviewCodexAnalysis::default(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![0]),
        profile_bw_filters: Vec::new(),
        profiles: vec![render],
        updated_at: now_string(),
    });

    store.sync_profiles(profiles);

    let render = &store.images[0].profiles[0];
    assert_eq!(render.status, ReviewRenderStatus::Missing);
    assert_eq!(render.output_path, None);
    assert_eq!(
        render.processing_key.as_deref(),
        Some(review_render_processing_key(0))
    );
}

#[test]
fn sync_profiles_preserves_publish_selection_after_restart_with_metadata_changes() {
    let base_profiles = vec![profile(0, "Classic"), profile(1, "Fade")];
    let mut changed_profiles = base_profiles.clone();
    changed_profiles[0].metadata = Some(ReviewProfileMetadata::default());
    changed_profiles[1].metadata = Some(ReviewProfileMetadata::default());

    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let mut store = ReviewStore::new(base_profiles);
    store.images.push(ReviewImage {
        id: 1,
        raw_path: PathBuf::from("/in/frame.NEF"),
        sooc_sidecar_path: None,
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
        rating_source: ReviewMetadataSource::Default,
        tags_source: ReviewMetadataSource::Default,
        notes_source: ReviewMetadataSource::Default,
        codex: ReviewCodexAnalysis::default(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![1]),
        profile_bw_filters: Vec::new(),
        profiles: vec![profile_render(0, "Classic"), profile_render(1, "Fade")],
        updated_at: now_string(),
    });
    save_store(&state_path, &store).unwrap();
    let mut loaded = load_store(&state_path).unwrap().unwrap();

    loaded.sync_profiles(changed_profiles);

    assert_eq!(loaded.images[0].publish_profile_indexes, Some(vec![1]));
    assert_eq!(loaded.images[0].selected_profile_index, 1);
}

#[test]
fn sync_profiles_merges_standalone_sooc_sidecar_after_restart() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let raw = input.join("frame.NEF");
    let sidecar = input.join("frame.JPG");
    fs::write(&raw, b"raw").unwrap();
    fs::write(&sidecar, b"jpg").unwrap();

    let state_path = output.join(SQLITE_STATE_FILE);
    let mut store = ReviewStore::new(vec![profile(0, "Classic")]);
    let raw_id = store.ensure_image(&input, &raw).unwrap().id;
    let sidecar_id = store.ensure_image(&input, &sidecar).unwrap().id;
    {
        let sidecar_image = store
            .images
            .iter_mut()
            .find(|image| image.id == sidecar_id)
            .unwrap();
        sidecar_image.rating = 4;
        sidecar_image.rating_source = ReviewMetadataSource::Manual;
        sidecar_image.label = ReviewLabel::Yellow;
        sidecar_image.labels = vec![ReviewLabel::Yellow];
        sidecar_image.tags = vec!["camera-jpeg".to_string()];
        sidecar_image.tags_source = ReviewMetadataSource::Manual;
        sidecar_image.notes = "sidecar note".to_string();
        sidecar_image.notes_source = ReviewMetadataSource::Manual;
    }
    store.ui.current_image_id = Some(sidecar_id);
    save_store(&state_path, &store).unwrap();

    let mut loaded = load_store(&state_path).unwrap().unwrap();
    loaded.sync_profiles(vec![profile(0, "Classic")]);

    assert_eq!(loaded.images.len(), 1);
    let image = &loaded.images[0];
    assert_eq!(image.id, raw_id);
    assert_eq!(image.raw_path, raw);
    assert_eq!(image.sooc_sidecar_path.as_deref(), Some(sidecar.as_path()));
    assert_eq!(image.rating, 4);
    assert_eq!(image.rating_source, ReviewMetadataSource::Manual);
    assert_eq!(image.labels, vec![ReviewLabel::Yellow]);
    assert_eq!(image.tags, vec!["camera-jpeg"]);
    assert_eq!(image.notes, "sidecar note");
    assert_eq!(loaded.ui.current_image_id, Some(raw_id));
    assert!(
        image
            .profiles
            .iter()
            .any(|render| render.profile_index == SOOC_PROFILE_INDEX)
    );
}

#[test]
fn sqlite_restart_adds_profiles_to_compressed_images_without_losing_review_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let jpg = input.join("frame.JPG");
    fs::write(&jpg, b"jpg").unwrap();

    let mut store = ReviewStore::new(Vec::new());
    let image = store.ensure_image(&input, &jpg).unwrap();
    image.rating = 4;
    image.tags = vec!["portrait".to_string()];
    image.notes = "keep this note".to_string();
    save_store(&output.join(SQLITE_STATE_FILE), &store).unwrap();

    let (mut restored, _) = load_or_migrate_store(&output).unwrap();
    restored.sync_profiles(vec![profile(0, "Classic"), profile(1, "Fade")]);
    let image = restored
        .images
        .iter()
        .find(|image| image.raw_path == jpg)
        .unwrap();
    assert!(image_uses_profile_pipeline(image));
    assert_eq!(image.rating, 4);
    assert_eq!(image.tags, vec!["portrait"]);
    assert_eq!(image.notes, "keep this note");
    assert_eq!(image.profiles.len(), 3);
    assert!(
        image
            .profiles
            .iter()
            .any(|render| render.profile_index == SOOC_PROFILE_INDEX)
    );
    assert_eq!(effective_publish_profile_indexes(image), vec![0, 1]);
    assert!(image.profiles.iter().all(|render| {
        render.processing_key.as_deref()
            == Some(review_render_processing_key_for_input(
                &jpg,
                render.profile_index,
            ))
    }));
}

#[test]
fn unchanged_config_enables_all_profiles_for_legacy_compressed_state() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    fs::create_dir_all(&input).unwrap();
    let jpg = input.join("frame.jpg");
    fs::write(&jpg, b"jpg").unwrap();
    let configured = vec![profile(0, "Classic"), profile(1, "Fade")];
    let mut store = ReviewStore::new(configured.clone());
    let image = store.ensure_image(&input, &jpg).unwrap();
    image.profiles.clear();
    image.publish_profile_indexes = Some(Vec::new());

    store.sync_profiles(configured);

    let image = &store.images[0];
    assert_eq!(image.profiles.len(), 3);
    assert!(
        image
            .profiles
            .iter()
            .any(|render| render.profile_index == SOOC_PROFILE_INDEX)
    );
    assert_eq!(effective_publish_profile_indexes(image), vec![0, 1]);
}

#[test]
fn discovered_raw_merges_existing_standalone_sidecar_and_ignores_stale_jpeg_callbacks() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let raw = input.join("frame.NEF");
    let sidecar = input.join("frame.JPG");
    let standalone_output = output.join("standalone-frame.jpg");
    fs::write(&raw, b"raw").unwrap();
    fs::write(&sidecar, b"jpg").unwrap();

    let handle = test_handle(input, output, vec![profile(0, "Classic")]);
    handle
        .record_compressed_queued(&sidecar, &standalone_output)
        .unwrap();
    {
        let state = handle.store_snapshot();
        assert_eq!(state.images.len(), 1);
        assert_eq!(state.images[0].raw_path, sidecar);
    }

    handle
        .record_discovered_raw_with_sidecar(&raw, Some(&sidecar))
        .unwrap();
    {
        let state = handle.store_snapshot();
        assert_eq!(state.images.len(), 1);
        let image = &state.images[0];
        assert_eq!(image.raw_path, raw);
        assert_eq!(image.sooc_sidecar_path.as_deref(), Some(sidecar.as_path()));
        assert!(
            image
                .profiles
                .iter()
                .any(|render| render.profile_index == SOOC_PROFILE_INDEX)
        );
    }

    handle.record_compressed_processing(&sidecar).unwrap();
    handle
        .record_compressed_done(&sidecar, &standalone_output, Duration::from_millis(5))
        .unwrap();
    handle
        .record_profile_queued(&sidecar, 0, &standalone_output)
        .unwrap();
    handle.record_profile_processing(&sidecar, 0).unwrap();
    handle
        .record_profile_done(&sidecar, 0, &standalone_output, Duration::from_millis(5))
        .unwrap();

    let state = handle.store_snapshot();
    assert_eq!(state.images.len(), 1);
    assert_eq!(state.images[0].raw_path, raw);
    assert_eq!(
        state.images[0].sooc_sidecar_path.as_deref(),
        Some(sidecar.as_path())
    );
}

#[test]
fn review_state_sqlite_round_trips_and_populates_query_tables() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let store = fully_populated_review_store();

    save_store(&state_path, &store).unwrap();
    let loaded = load_store(&state_path).unwrap().unwrap();

    assert_eq!(
        serde_json::to_value(&loaded).unwrap(),
        serde_json::to_value(&store).unwrap()
    );
    assert!(!table_exists(&state_path, "review_state").unwrap());
    assert_eq!(table_count(&state_path, "review_settings").unwrap(), 1);
    assert_eq!(table_count(&state_path, "profiles").unwrap(), 2);
    assert_eq!(table_count(&state_path, "images").unwrap(), 3);
    assert_eq!(table_count(&state_path, "tags").unwrap(), 2);
    assert_eq!(table_count(&state_path, "image_tags").unwrap(), 3);
    assert_eq!(
        table_count(&state_path, "image_profile_renders").unwrap(),
        1
    );
    assert_eq!(
        table_count(&state_path, "image_profile_bw_filters").unwrap(),
        3
    );
    assert_eq!(table_count(&state_path, "profile_pp3_sections").unwrap(), 2);
    assert_eq!(table_count(&state_path, "profile_pp3_entries").unwrap(), 2);
    assert!(json_storage_columns(&state_path).unwrap().is_empty());
    let connection = rusqlite::Connection::open(&state_path).unwrap();
    let source_info = connection
        .query_row(
            "SELECT source_file_size_bytes, source_width, source_height, exif_shutter_count,
                    exif_auto_iso, exif_iso_auto_hi_limit, exif_white_balance_mode,
                    exif_white_balance_temperature, exif_white_balance_offset
             FROM images WHERE image_id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        source_info,
        (
            55_620_945,
            8288,
            5520,
            66_278,
            1,
            "ISO 6400".to_string(),
            "Auto1".to_string(),
            4860,
            -2
        )
    );
    let shutter_details = connection
        .query_row(
            "SELECT exif_shutter_mode, exif_silent_photography, exif_release_mode
             FROM images WHERE image_id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        shutter_details,
        (
            "Auto (Electronic Front Curtain)".to_string(),
            1,
            "Single Frame".to_string()
        )
    );
    let publish_modes = connection
        .prepare("SELECT image_id, publish_profiles_default FROM images ORDER BY image_id")
        .unwrap()
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(publish_modes, vec![(1, 1), (2, 0), (3, 0)]);
    let image_tag_columns = connection
        .prepare("PRAGMA table_info('image_tags')")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(image_tag_columns, vec!["image_id", "tag_id", "position"]);
    let tags = connection
        .prepare("SELECT tag FROM tags ORDER BY tag")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(tags, vec!["keeper", "publish"]);
    let schema_version = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert_eq!(schema_version, LATEST_SCHEMA_VERSION);
}

#[test]
fn legacy_json_review_state_is_migrated_once_after_verified_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path();
    let legacy_path = output.join(LEGACY_JSON_STATE_FILE);
    let sqlite_path = output.join(SQLITE_STATE_FILE);
    let mut store = ReviewStore::new(vec![profile(0, "Classic")]);
    store.ui.min_rating = 1;
    fs::write(&legacy_path, serde_json::to_string_pretty(&store).unwrap()).unwrap();
    fs::write(&sqlite_path, b"").unwrap();

    let (loaded, state_path) = load_or_migrate_store(output).unwrap();

    assert_eq!(state_path, sqlite_path);
    assert_eq!(
        serde_json::to_value(&loaded).unwrap(),
        serde_json::to_value(&store).unwrap()
    );
    assert!(sqlite_path.is_file());
    assert!(!legacy_path.exists());
    assert!(fs::read_dir(output).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("mini-film-review.json.migrated-")
    }));
    let loaded_again = load_store(&sqlite_path).unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(&loaded_again).unwrap(),
        serde_json::to_value(&store).unwrap()
    );
    assert!(!table_exists(&sqlite_path, "review_state").unwrap());
    assert!(json_storage_columns(&sqlite_path).unwrap().is_empty());
}

#[test]
fn existing_v5_sqlite_state_is_migrated_losslessly_and_only_once() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let store = fully_populated_review_store();
    create_v5_review_database(&state_path, &store);

    let loaded = load_store(&state_path).unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(&loaded).unwrap(),
        serde_json::to_value(&store).unwrap()
    );
    assert!(!table_exists(&state_path, "review_state").unwrap());
    assert!(json_storage_columns(&state_path).unwrap().is_empty());

    let connection = rusqlite::Connection::open(&state_path).unwrap();
    let schema_version = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert_eq!(schema_version, LATEST_SCHEMA_VERSION);
    let migration_count = connection
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = ?1",
            [LATEST_SCHEMA_VERSION],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(migration_count, 1);
    drop(connection);

    let loaded_again = load_store(&state_path).unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(loaded_again).unwrap(),
        serde_json::to_value(store).unwrap()
    );
    let connection = rusqlite::Connection::open(&state_path).unwrap();
    let migration_count = connection
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = ?1",
            [LATEST_SCHEMA_VERSION],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(migration_count, 1);
}

#[test]
fn failed_v5_normalization_rolls_back_schema_and_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let mut store = fully_populated_review_store();
    store.next_id = u64::MAX;
    create_v5_review_database(&state_path, &store);
    let original_json = serde_json::to_string_pretty(&store).unwrap();

    let error = load_store(&state_path).unwrap_err();
    assert!(
        format!("{error:#}").contains("does not fit sqlite INTEGER"),
        "{error:#}"
    );

    let connection = rusqlite::Connection::open(&state_path).unwrap();
    let schema_version = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert_eq!(schema_version, 5);
    let stored_json = connection
        .query_row(
            "SELECT store_json FROM review_state WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    assert_eq!(stored_json, original_json);
    let migration_count = connection
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 6",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(migration_count, 0);
    let settings_table_count = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'review_settings'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(settings_table_count, 0);
    let image_json_column_count = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('images') WHERE name = 'image_json'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(image_json_column_count, 1);
}

#[test]
fn existing_v6_sqlite_adds_shutter_metadata_columns() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let connection = open_database_at_version_for_test(&state_path, 6).unwrap();
    connection
        .execute_batch(
            "ALTER TABLE images DROP COLUMN exif_shutter_count;
             ALTER TABLE images DROP COLUMN exif_shutter_mode;
             ALTER TABLE images DROP COLUMN exif_silent_photography;
             ALTER TABLE images DROP COLUMN exif_release_mode;",
        )
        .unwrap();
    drop(connection);

    assert!(load_store(&state_path).unwrap().is_none());

    let connection = rusqlite::Connection::open(&state_path).unwrap();
    let column_count = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('images')
             WHERE name IN (
                 'exif_shutter_count', 'exif_shutter_mode',
                 'exif_silent_photography', 'exif_release_mode'
             )",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(column_count, 4);
    let schema_version = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert_eq!(schema_version, LATEST_SCHEMA_VERSION);
}

#[test]
fn existing_v7_sqlite_adds_shutter_detail_columns() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let connection = open_database_at_version_for_test(&state_path, 7).unwrap();
    connection
        .execute_batch(
            "ALTER TABLE images DROP COLUMN exif_shutter_mode;
             ALTER TABLE images DROP COLUMN exif_silent_photography;
             ALTER TABLE images DROP COLUMN exif_release_mode;",
        )
        .unwrap();
    drop(connection);

    assert!(load_store(&state_path).unwrap().is_none());

    let connection = rusqlite::Connection::open(&state_path).unwrap();
    let column_count = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('images')
             WHERE name IN (
                 'exif_shutter_mode', 'exif_silent_photography', 'exif_release_mode'
             )",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(column_count, 3);
    let schema_version = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert_eq!(schema_version, LATEST_SCHEMA_VERSION);
}

#[test]
fn existing_v8_sqlite_adds_auto_iso_columns() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let connection = open_database_at_version_for_test(&state_path, 8).unwrap();
    connection
        .execute_batch(
            "ALTER TABLE images DROP COLUMN exif_auto_iso;
             ALTER TABLE images DROP COLUMN exif_iso_auto_hi_limit;",
        )
        .unwrap();
    drop(connection);

    assert!(load_store(&state_path).unwrap().is_none());

    let connection = rusqlite::Connection::open(&state_path).unwrap();
    let column_count = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('images')
             WHERE name IN ('exif_auto_iso', 'exif_iso_auto_hi_limit')",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(column_count, 2);
    let schema_version = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert_eq!(schema_version, LATEST_SCHEMA_VERSION);
}

#[test]
fn existing_v9_sqlite_adds_white_balance_columns() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let connection = open_database_at_version_for_test(&state_path, 9).unwrap();
    connection
        .execute_batch(
            "ALTER TABLE images DROP COLUMN exif_white_balance_mode;
             ALTER TABLE images DROP COLUMN exif_white_balance_temperature;",
        )
        .unwrap();
    drop(connection);

    assert!(load_store(&state_path).unwrap().is_none());

    let connection = rusqlite::Connection::open(&state_path).unwrap();
    let column_count = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('images')
             WHERE name IN ('exif_white_balance_mode', 'exif_white_balance_temperature')",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(column_count, 2);
    let schema_version = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert_eq!(schema_version, LATEST_SCHEMA_VERSION);
}

#[test]
fn existing_v10_sqlite_adds_white_balance_offset_column() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let connection = open_database_at_version_for_test(&state_path, 10).unwrap();
    connection
        .execute_batch("ALTER TABLE images DROP COLUMN exif_white_balance_offset;")
        .unwrap();
    drop(connection);

    assert!(load_store(&state_path).unwrap().is_none());

    let connection = rusqlite::Connection::open(&state_path).unwrap();
    let column_count = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('images')
             WHERE name = 'exif_white_balance_offset'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(column_count, 1);
    let schema_version = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert_eq!(schema_version, LATEST_SCHEMA_VERSION);
}

#[test]
fn newer_sqlite_schema_is_rejected_without_modification() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let connection = rusqlite::Connection::open(&state_path).unwrap();
    connection
        .execute_batch("PRAGMA user_version = 12;")
        .unwrap();
    drop(connection);

    let error = load_store(&state_path).unwrap_err();
    assert!(
        format!("{error:#}").contains("newer than supported version"),
        "{error:#}"
    );

    let connection = rusqlite::Connection::open(&state_path).unwrap();
    let schema_version = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert_eq!(schema_version, 12);
    let migration_table_count = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'schema_migrations'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(migration_table_count, 0);
}

#[test]
fn schedule_ready_codex_jobs_does_not_reschedule_done_images() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let raw = input.join("frame.jpg");
    let preview = output.join("preview.jpg");
    fs::write(&raw, b"jpg").unwrap();
    fs::write(&preview, b"jpg").unwrap();

    let mut handle = test_handle(
        input,
        output.clone(),
        vec![profile(0, "Classic"), profile(1, "Fade")],
    );
    handle.codex = Some(ReviewCodexConfig {
        flags: CodexAnalysisFlags::all(),
        codex_binary: PathBuf::from("codex"),
        model: "yolo11n".to_string(),
        timeout: Duration::from_secs(60),
    });

    handle
        .update_store(|store| {
            store.images.push(ReviewImage {
                id: 1,
                raw_path: raw.clone(),
                sooc_sidecar_path: None,
                relative_path: "frame.jpg".to_string(),
                file_name: "frame.jpg".to_string(),
                exif: GalleryExifData::default(),
                preview: ReviewPreview {
                    status: ReviewRenderStatus::Done,
                    path: Some(preview.clone()),
                    render_key: None,
                    error: None,
                    duration_ms: Some(1),
                    updated_at: now_string(),
                },
                selected_profile_index: 0,
                rating: 0,
                label: ReviewLabel::None,
                labels: Vec::new(),
                tags: Vec::new(),
                notes: String::new(),
                rating_source: ReviewMetadataSource::Manual,
                tags_source: ReviewMetadataSource::Manual,
                notes_source: ReviewMetadataSource::Manual,
                codex: ReviewCodexAnalysis {
                    status: ReviewCodexStatus::Done,
                    flags: CodexAnalysisFlags::all(),
                    model: "yolo11n".to_string(),
                    analysis_key: Some("stale-key".to_string()),
                    error: None,
                    updated_at: now_string(),
                },
                retouch: RetouchSettings::default(),
                publish_profile_indexes: Some(vec![0]),
                profile_bw_filters: Vec::new(),
                profiles: Vec::new(),
                updated_at: now_string(),
            });
            Ok(())
        })
        .unwrap();

    handle.schedule_ready_codex_jobs().unwrap();

    let state = handle.store_snapshot();
    let image = &state.images[0];
    assert_eq!(image.codex.status, ReviewCodexStatus::Done);
    assert_eq!(image.codex.analysis_key.as_deref(), Some("stale-key"));
    assert!(handle.codex_scheduler.pending.load_full().is_empty());
}

#[test]
fn review_media_scheduler_orders_each_pipeline_by_capture_time_then_filename() {
    assert_eq!(REVIEW_THUMBNAIL_WORKERS, 1);
    assert_eq!(REVIEW_PREVIEW_WORKERS, 2);
    let scheduler = ReviewMediaScheduler::default();
    scheduler.schedule(
        PathBuf::from("late.jpg"),
        3,
        Some(300),
        "late.jpg".to_string(),
    );
    scheduler.schedule(
        PathBuf::from("missing-time.jpg"),
        4,
        None,
        "missing-time.jpg".to_string(),
    );
    scheduler.schedule(
        PathBuf::from("early-b.jpg"),
        2,
        Some(100),
        "early-b.jpg".to_string(),
    );
    scheduler.schedule(
        PathBuf::from("early-a.jpg"),
        1,
        Some(100),
        "early-a.jpg".to_string(),
    );

    for kind in [ReviewMediaKind::Thumbnail, ReviewMediaKind::Preview] {
        let order = (0..4)
            .map(|_| scheduler.next_job(kind).image_id)
            .collect::<Vec<_>>();
        assert_eq!(order, [1, 2, 3, 4]);
    }
}

#[test]
fn review_media_scheduler_does_not_run_duplicate_tier_jobs_concurrently() {
    let scheduler = Arc::new(ReviewMediaScheduler::default());
    let raw = PathBuf::from("frame.jpg");
    scheduler.schedule(raw.clone(), 1, Some(100), "frame.jpg".to_string());
    let first = scheduler.next_job(ReviewMediaKind::Preview);
    scheduler.schedule(raw.clone(), 1, Some(100), "frame.jpg".to_string());

    let worker_scheduler = Arc::clone(&scheduler);
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        sender
            .send(worker_scheduler.next_job(ReviewMediaKind::Preview))
            .unwrap();
    });
    assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
    scheduler.finish(ReviewMediaKind::Preview, &first.raw);
    let second = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    scheduler.finish(ReviewMediaKind::Preview, &second.raw);
    worker.join().unwrap();
    assert_eq!(first.raw, second.raw);
}

#[test]
fn codex_scheduler_claims_distinct_jobs_across_two_workers() {
    assert_eq!(REVIEW_CODEX_WORKERS, 2);
    let scheduler = Arc::new(ReviewCodexScheduler::default());
    scheduler.schedule(PathBuf::from("first.jpg"), "first-key".to_string());
    scheduler.schedule(PathBuf::from("second.jpg"), "second-key".to_string());

    let barrier = Arc::new(std::sync::Barrier::new(REVIEW_CODEX_WORKERS + 1));
    let (sender, receiver) = std::sync::mpsc::channel();
    let workers = (0..REVIEW_CODEX_WORKERS)
        .map(|_| {
            let scheduler = Arc::clone(&scheduler);
            let barrier = Arc::clone(&barrier);
            let sender = sender.clone();
            thread::spawn(move || {
                barrier.wait();
                sender.send(scheduler.next_job().raw).unwrap();
            })
        })
        .collect::<Vec<_>>();
    drop(sender);
    barrier.wait();

    let claimed = (0..REVIEW_CODEX_WORKERS)
        .map(|_| receiver.recv_timeout(Duration::from_secs(2)).unwrap())
        .collect::<HashSet<_>>();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(
        claimed,
        HashSet::from([PathBuf::from("first.jpg"), PathBuf::from("second.jpg")])
    );
    assert!(scheduler.pending.load_full().is_empty());
}

#[test]
fn base_render_done_triggers_pending_retouch_without_marking_done() {
    let output = PathBuf::from("frame.jpg");
    let mut render = ReviewProfileRender {
        profile_index: 0,
        profile_stem: "Classic".to_string(),
        display_name: None,
        status: ReviewRenderStatus::Queued,
        output_path: None,
        error: Some("old".to_string()),
        duration_ms: None,
        render_key: Some("retouch-key".to_string()),
        processing_key: Some(review_render_processing_key(0).to_string()),
        width: None,
        height: None,
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
fn retouch_cache_output_is_stable_from_base_or_cached_path() {
    let base = PathBuf::from("/out/Classic/frame.jpg");
    let cache = retouch_cache_output(&base, "abc123");

    assert_eq!(
        cache,
        PathBuf::from("/out/Classic/.frame.retouch-cache-abc123.jpg")
    );
    assert_eq!(retouch_base_output(&cache), base);
    assert_eq!(
        retouch_cache_output(&cache, "def456"),
        PathBuf::from("/out/Classic/.frame.retouch-cache-def456.jpg")
    );
    assert_eq!(
        retouch_temp_output(&cache, "def456"),
        PathBuf::from("/out/Classic/.frame.retouch-def456.jpg")
    );
}

#[test]
fn base_render_done_uses_cached_retouch_output_without_scheduling() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let raw = input.join("frame.NEF");
    let rendered = output.join("Classic").join("frame.jpg");
    fs::create_dir_all(rendered.parent().unwrap()).unwrap();
    fs::write(&raw, b"raw").unwrap();
    fs::write(&rendered, b"base").unwrap();

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
    let cached = retouch_cache_output(&rendered, &expected_key);
    fs::write(&cached, b"cached").unwrap();
    handle
        .update_store(|store| {
            let image = store
                .images
                .iter_mut()
                .find(|image| image.raw_path == raw)
                .unwrap();
            image.retouch = saved_retouch.clone();
            Ok(())
        })
        .unwrap();

    handle.record_profile_queued(&raw, 0, &rendered).unwrap();
    handle
        .record_profile_done(&raw, 0, &rendered, Duration::from_millis(42))
        .unwrap();

    let store = handle.store_snapshot();
    let render = &store.images[0].profiles[0];
    assert_eq!(render.status, ReviewRenderStatus::Done);
    assert_eq!(render.output_path.as_deref(), Some(cached.as_path()));
    assert_eq!(render.render_key, None);
    assert!(handle.retouch_scheduler.pending.load_full().is_empty());
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
        handle
            .update_store(|store| {
                let image = store
                    .images
                    .iter_mut()
                    .find(|image| image.raw_path == raw)
                    .unwrap();
                image.retouch = saved_retouch.clone();
                Ok(())
            })
            .unwrap();
    }

    handle.record_profile_queued(&raw, 0, &rendered).unwrap();
    {
        let store = handle.store_snapshot();
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
    assert_eq!(job.profile_index, Some(0));
    assert_eq!(job.output, rendered);
    assert_eq!(job.render_key, expected_key);
}

#[test]
fn review_update_uses_cached_bw_filter_output_without_scheduling() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let raw = input.join("frame.NEF");
    let rendered = output.join("BW").join("frame.jpg");
    fs::create_dir_all(rendered.parent().unwrap()).unwrap();
    fs::write(&raw, b"raw").unwrap();
    fs::write(&rendered, b"base").unwrap();

    let handle = test_handle(input, output, vec![bw_profile(0, "BW", -100.0, 0.0)]);
    handle.record_discovered_raw(&raw).unwrap();
    handle
        .record_profile_done(&raw, 0, &rendered, Duration::from_millis(42))
        .unwrap();
    let render_key = profile_render_key_value(
        &RetouchSettings::default(),
        RetouchWhiteBalance::default(),
        BwFilter::Yellow,
    );
    let cached = retouch_cache_output(&rendered, &render_key);
    fs::write(&cached, b"yellow").unwrap();

    handle
        .apply_review_update(ReviewUpdateRequest {
            image_id: 1,
            rating: 0,
            label: ReviewLabel::None,
            labels: Vec::new(),
            tags: Vec::new(),
            notes: String::new(),
            retouch: None,
            selected_profile_index: Some(0),
            publish_profile_indexes: Some(vec![0]),
            profile_bw_filters: Some(vec![ReviewProfileBwFilter {
                profile_index: 0,
                filter: BwFilter::Yellow,
            }]),
            advance_after_update: false,
        })
        .unwrap();

    let store = handle.store_snapshot();
    let image = &store.images[0];
    assert_eq!(
        effective_bw_filter_for_profile(image, &store.profiles[0]),
        BwFilter::Yellow
    );
    let render = &image.profiles[0];
    assert_eq!(render.status, ReviewRenderStatus::Done);
    assert_eq!(render.output_path.as_deref(), Some(cached.as_path()));
    assert_eq!(render.render_key, None);
    assert!(handle.retouch_scheduler.pending.load_full().is_empty());
}

#[test]
fn retouch_scheduler_coalesces_same_raw_profile_to_latest_job() {
    let scheduler = ReviewRetouchScheduler::default();
    scheduler.schedule_after(
        PathBuf::from("frame.NEF"),
        Some(1),
        PathBuf::from("old.jpg"),
        "old".to_string(),
        Duration::ZERO,
    );
    scheduler.schedule_after(
        PathBuf::from("frame.NEF"),
        Some(1),
        PathBuf::from("new.jpg"),
        "new".to_string(),
        Duration::ZERO,
    );

    let job = scheduler.next_job();

    assert_eq!(job.raw, PathBuf::from("frame.NEF"));
    assert_eq!(job.profile_index, Some(1));
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
            selected_profile_index: Some(0),
            publish_profile_indexes: Some(vec![0, 1]),
            profile_bw_filters: None,
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
            selected_profile_index: Some(0),
            publish_profile_indexes: Some(vec![0, 1]),
            profile_bw_filters: None,
            advance_after_update: true,
        })
        .unwrap();

    let state =
        serde_json::from_str::<serde_json::Value>(&handle.api_state_json().unwrap()).unwrap();
    assert_eq!(state["ui"]["current_image_id"], 1);
    assert_eq!(state["ui"]["min_rating"], 1);
}

#[test]
fn review_update_without_profile_selection_preserves_current_profile() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(input.join("day")).unwrap();
    fs::create_dir_all(&output).unwrap();
    let raw = input.join("day").join("frame-1.NEF");
    fs::write(&raw, b"raw").unwrap();

    let handle = test_handle(
        input,
        output,
        vec![profile(0, "Classic"), profile(1, "Fade")],
    );
    handle.record_discovered_raw(&raw).unwrap();
    handle
        .apply_review_update(ReviewUpdateRequest {
            image_id: 1,
            rating: 3,
            label: ReviewLabel::Red,
            labels: vec![ReviewLabel::Red],
            tags: vec!["keep".to_string()],
            notes: String::new(),
            retouch: None,
            selected_profile_index: Some(1),
            publish_profile_indexes: Some(vec![0, 1]),
            profile_bw_filters: None,
            advance_after_update: false,
        })
        .unwrap();

    handle
        .apply_review_update(ReviewUpdateRequest {
            image_id: 1,
            rating: 4,
            label: ReviewLabel::Green,
            labels: vec![ReviewLabel::Green],
            tags: vec!["keep".to_string(), "edit".to_string()],
            notes: String::new(),
            retouch: None,
            selected_profile_index: None,
            publish_profile_indexes: Some(vec![0, 1]),
            profile_bw_filters: None,
            advance_after_update: false,
        })
        .unwrap();

    let state =
        serde_json::from_str::<serde_json::Value>(&handle.api_state_json().unwrap()).unwrap();
    assert_eq!(state["images"][0]["selected_profile_index"], 1);
    assert_eq!(state["images"][0]["rating"], 4);
    assert_eq!(state["images"][0]["labels"], serde_json::json!(["green"]));
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
            selected_profile_index: Some(1),
            publish_profile_indexes: Some(vec![1]),
            profile_bw_filters: None,
            advance_after_update: false,
        })
        .unwrap();
    handle
        .apply_ui_update(ReviewUiUpdateRequest {
            current_image_id: Some(1),
            min_rating: 3,
        })
        .unwrap();

    handle
        .update_publish_jobs(|jobs| {
            jobs.push(ReviewPublishJob {
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
                gallery_urls: Vec::new(),
                error: None,
            });
            Ok(())
        })
        .unwrap();
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
fn review_state_publish_defaults_include_daemon_grain_engine() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();

    let handle = test_handle(input, output, vec![profile(0, "Classic")]);

    let state =
        serde_json::from_str::<serde_json::Value>(&handle.api_state_json().unwrap()).unwrap();
    assert_eq!(
        state["publish_defaults"]["grain_engine"],
        mini_film::GrainEngine::default().to_string()
    );
}

#[test]
fn publish_args_rerender_when_publish_grain_engine_differs_from_daemon() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();

    let handle = test_handle(input, output, vec![profile(0, "Classic")]);
    let matching = handle
        .publish_args_from_request(&PublishRequest {
            grain_engine: Some(mini_film::GrainEngine::default().to_string()),
            ..PublishRequest::default()
        })
        .unwrap();
    assert!(!matching.rerender_raw);
    assert_eq!(matching.grain_engine, mini_film::GrainEngine::default());

    let changed = handle
        .publish_args_from_request(&PublishRequest {
            grain_engine: Some(mini_film::GrainEngine::Rfgr.to_string()),
            ..PublishRequest::default()
        })
        .unwrap();
    assert!(changed.rerender_raw);
    assert_eq!(changed.grain_engine, mini_film::GrainEngine::Rfgr);
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
    assert_eq!(
        review_route_path("/mini-film/crop-source/1"),
        "/crop-source/1"
    );
    assert_eq!(review_route_path("/mini-film/original/1"), "/original/1");
    assert_eq!(review_route_path("/mini-film/thumbnail/1"), "/thumbnail/1");
    assert_eq!(review_route_path("/mini-film/preview/1"), "/preview/1");
    assert_eq!(
        review_route_path("/mini-film/outputs/galleries/day/index.html"),
        "/outputs/galleries/day/index.html"
    );
    assert_eq!(review_route_path("/mini-film/review"), "/review");
    assert_eq!(review_route_path("/mini-film/tv"), "/tv");
    assert_eq!(review_route_path("/mini-film/"), "/");
}

#[tokio::test]
async fn original_response_serves_typed_compressed_source_files_inline() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let jpg = input.join("frame.JPG");
    fs::write(&jpg, b"original jpeg bytes").unwrap();

    let handle = test_handle(input.clone(), output.clone(), vec![profile(0, "Classic")]);
    handle
        .record_compressed_queued(&jpg, &output.join("frame.jpg"))
        .unwrap();

    let response = route_request(
        axum::http::Method::GET,
        "/original/1",
        axum::body::Bytes::new(),
        &handle,
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap(),
        "image/jpeg"
    );
    assert!(
        response
            .headers()
            .get(axum::http::header::CONTENT_DISPOSITION)
            .is_none()
    );
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"original jpeg bytes");

    let heic = input.join("frame-2.HEIC");
    fs::write(&heic, b"original heic bytes").unwrap();
    handle
        .record_compressed_queued(&heic, &output.join("frame-2.jpg"))
        .unwrap();
    let response = route_request(
        axum::http::Method::GET,
        "/original/2",
        axum::body::Bytes::new(),
        &handle,
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap(),
        "image/heic"
    );

    let raw = input.join("frame-3.NEF");
    fs::write(&raw, b"raw bytes").unwrap();
    handle
        .update_store(|store| {
            store.ensure_image(&input, &raw)?;
            Ok(())
        })
        .unwrap();
    let response = route_request(
        axum::http::Method::GET,
        "/original/3",
        axum::body::Bytes::new(),
        &handle,
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn compressed_review_media_routes_serve_distinct_cached_sizes_and_full_output() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let jpg = input.join("frame.JPG");
    let full = output.join("frame.jpg");
    fs::write(&jpg, b"original jpeg bytes").unwrap();
    fs::write(&full, b"full output bytes").unwrap();

    let handle = test_handle(input, output.clone(), Vec::new());
    handle.record_compressed_queued(&jpg, &full).unwrap();
    let thumbnail = handle.compressed_thumbnail_path_for(&jpg, 1);
    let preview = handle.compressed_display_preview_path_for(&jpg, 1);
    fs::create_dir_all(thumbnail.parent().unwrap()).unwrap();
    fs::create_dir_all(preview.parent().unwrap()).unwrap();
    fs::write(&thumbnail, b"thumbnail bytes").unwrap();
    fs::write(&preview, b"preview bytes").unwrap();
    handle
        .update_store(|store| {
            let image = store.images.iter_mut().find(|image| image.id == 1).unwrap();
            image.exif.image_width = Some(8288);
            image.exif.image_height = Some(5520);
            Ok(())
        })
        .unwrap();

    let cache_root = output
        .join(".mini-film-review-previews")
        .join(COMPRESSED_REVIEW_CACHE_VERSION);
    assert!(thumbnail.starts_with(&cache_root));
    assert!(preview.starts_with(&cache_root));
    let state = handle.api_state_value().unwrap();
    let image = &state["images"][0];
    assert_eq!(image["thumbnail_url"], "thumbnail/1");
    assert_eq!(image["preview_url"], "preview/1");
    assert_eq!(image["crop_source_url"], "preview/1");
    assert_eq!(image["full_url"], "original/1");
    assert_eq!(image["processing_mode"], "direct");
    assert_eq!(
        image["source_file_size_bytes"],
        json!(fs::metadata(&jpg).unwrap().len())
    );
    assert_eq!(image["source_width"], 8288);
    assert_eq!(image["source_height"], 5520);

    for (route, expected) in [
        ("/thumbnail/1", &b"thumbnail bytes"[..]),
        ("/preview/1", &b"preview bytes"[..]),
    ] {
        let response = route_request(
            axum::http::Method::GET,
            route,
            axum::body::Bytes::new(),
            &handle,
        )
        .await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], expected);
    }

    handle
        .update_store(|store| {
            let image = store.images.iter_mut().find(|image| image.id == 1).unwrap();
            image.preview.status = ReviewRenderStatus::Done;
            image.preview.path = Some(full.clone());
            Ok(())
        })
        .unwrap();
    let state = handle.api_state_value().unwrap();
    assert_eq!(state["images"][0]["full_url"], "original/1");
    let response = route_request(
        axum::http::Method::GET,
        "/media/1",
        axum::body::Bytes::new(),
        &handle,
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"full output bytes");
}

#[test]
fn profiled_compressed_review_state_exposes_profiles_and_original_source() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let jpg = input.join("frame.JPG");
    fs::write(&jpg, b"original jpeg bytes").unwrap();
    let rendered = output.join("Classic").join("frame.jpg");
    fs::create_dir_all(rendered.parent().unwrap()).unwrap();
    fs::write(&rendered, b"profiled output").unwrap();
    let sooc = output.join(SOOC_PROFILE_STEM).join("frame.jpg");
    fs::create_dir_all(sooc.parent().unwrap()).unwrap();
    fs::write(&sooc, b"sooc output").unwrap();

    let handle = test_handle(input, output, vec![profile(0, "Classic")]);
    handle.record_profiled_compressed_discovered(&jpg).unwrap();
    handle.record_profile_queued(&jpg, 0, &rendered).unwrap();
    handle
        .record_profile_done(&jpg, 0, &rendered, Duration::from_millis(10))
        .unwrap();
    handle
        .record_profile_queued(&jpg, SOOC_PROFILE_INDEX, &sooc)
        .unwrap();
    handle
        .record_profile_done(&jpg, SOOC_PROFILE_INDEX, &sooc, Duration::from_millis(5))
        .unwrap();

    let state = handle.api_state_value().unwrap();
    let image = &state["images"][0];
    assert_eq!(image["source_type"], "compressed");
    assert_eq!(image["processing_mode"], "profiled");
    assert_eq!(image["full_url"], "original/1");
    assert_eq!(image["profiles"].as_array().unwrap().len(), 2);
    assert_eq!(image["profiles"][0]["profile_index"], 0);
    assert_eq!(image["profiles"][0]["base_url"], "media/1/0/base");
    assert_eq!(image["profiles"][1]["profile_index"], SOOC_PROFILE_INDEX);
    assert_eq!(image["profiles"][1]["profile_stem"], SOOC_PROFILE_STEM);
    assert_eq!(
        image["profiles"][1]["display_name"],
        SOOC_PROFILE_DISPLAY_NAME
    );
    assert_eq!(image["publish_profile_indexes"], json!([0]));
}

#[tokio::test]
async fn crop_source_and_profile_base_routes_serve_uncropped_media() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let raw = input.join("frame.NEF");
    let sidecar = input.join("frame.JPG");
    fs::write(&raw, b"raw").unwrap();
    fs::write(&sidecar, b"sidecar").unwrap();
    let base = output.join("Classic").join("frame.jpg");
    let cached = retouch_cache_output(&base, "crop");
    fs::create_dir_all(base.parent().unwrap()).unwrap();
    fs::write(&base, b"uncropped profile").unwrap();
    fs::write(&cached, b"cropped profile").unwrap();

    let handle = test_handle(input.clone(), output, vec![profile(0, "Classic")]);
    handle
        .update_store(|store| {
            let image = store.ensure_image(&input, &raw)?;
            image.sooc_sidecar_path = Some(sidecar.clone());
            let render = image
                .profiles
                .iter_mut()
                .find(|render| render.profile_index == 0)
                .unwrap();
            render.status = ReviewRenderStatus::Done;
            render.output_path = Some(cached.clone());
            Ok(())
        })
        .unwrap();
    let crop_source = handle.crop_source_preview_path_for(&sidecar, 1);
    fs::create_dir_all(crop_source.parent().unwrap()).unwrap();
    fs::write(&crop_source, b"crop source").unwrap();

    let state = handle.api_state_value().unwrap();
    assert_eq!(state["images"][0]["crop_source_url"], "crop-source/1");
    assert_eq!(
        state["images"][0]["profiles"][0]["base_url"],
        "media/1/0/base"
    );

    for (route, expected) in [
        ("/crop-source/1", &b"crop source"[..]),
        ("/media/1/0/base", &b"uncropped profile"[..]),
    ] {
        let response = route_request(
            axum::http::Method::GET,
            route,
            axum::body::Bytes::new(),
            &handle,
        )
        .await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], expected);
    }
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
        sooc_sidecar_path: None,
        relative_path: "day/frame.NEF".to_string(),
        file_name: "frame.NEF".to_string(),
        exif: GalleryExifData::default(),
        selected_profile_index: 0,
        rating: 3,
        label: ReviewLabel::Red,
        labels: vec![ReviewLabel::Red],
        tags: vec!["42".to_string()],
        notes: "keeper".to_string(),
        rating_source: ReviewMetadataSource::Manual,
        tags_source: ReviewMetadataSource::Manual,
        notes_source: ReviewMetadataSource::Manual,
        codex: ReviewCodexAnalysis::default(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![0]),
        profile_bw_filters: Vec::new(),
        preview: ReviewPreview::default(),
        profiles: vec![ReviewProfileRender {
            profile_index: 0,
            profile_stem: "Classic".to_string(),
            display_name: None,
            status: ReviewRenderStatus::Done,
            output_path: Some(source.clone()),
            error: None,
            duration_ms: Some(1),
            render_key: None,
            processing_key: Some(review_render_processing_key(0).to_string()),
            width: None,
            height: None,
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
        sooc_sidecar_path: None,
        relative_path: "day/frame.NEF".to_string(),
        file_name: "frame.NEF".to_string(),
        exif: GalleryExifData::default(),
        selected_profile_index: 0,
        rating: 2,
        label: ReviewLabel::None,
        labels: Vec::new(),
        tags: Vec::new(),
        notes: String::new(),
        rating_source: ReviewMetadataSource::Default,
        tags_source: ReviewMetadataSource::Default,
        notes_source: ReviewMetadataSource::Default,
        codex: ReviewCodexAnalysis::default(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![1]),
        profile_bw_filters: Vec::new(),
        preview: ReviewPreview::default(),
        profiles: vec![
            ReviewProfileRender {
                profile_index: 0,
                profile_stem: "Classic".to_string(),
                display_name: None,
                status: ReviewRenderStatus::Done,
                output_path: Some(classic.clone()),
                error: None,
                duration_ms: Some(1),
                render_key: None,
                processing_key: Some(review_render_processing_key(0).to_string()),
                width: None,
                height: None,
                updated_at: now_string(),
            },
            ReviewProfileRender {
                profile_index: 1,
                profile_stem: "Fade".to_string(),
                display_name: None,
                status: ReviewRenderStatus::Done,
                output_path: Some(fade.clone()),
                error: None,
                duration_ms: Some(1),
                render_key: None,
                processing_key: Some(review_render_processing_key(1).to_string()),
                width: None,
                height: None,
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
        sooc_sidecar_path: None,
        relative_path: "day/frame.NEF".to_string(),
        file_name: "frame.NEF".to_string(),
        exif: GalleryExifData::default(),
        selected_profile_index: 0,
        rating: 5,
        label: ReviewLabel::None,
        labels: Vec::new(),
        tags: Vec::new(),
        notes: String::new(),
        rating_source: ReviewMetadataSource::Default,
        tags_source: ReviewMetadataSource::Default,
        notes_source: ReviewMetadataSource::Default,
        codex: ReviewCodexAnalysis::default(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: Some(vec![0, 1]),
        profile_bw_filters: Vec::new(),
        preview: ReviewPreview::default(),
        profiles: vec![
            ReviewProfileRender {
                profile_index: 0,
                profile_stem: "Classic".to_string(),
                display_name: None,
                status: ReviewRenderStatus::Done,
                output_path: Some(classic.clone()),
                error: None,
                duration_ms: Some(1),
                render_key: None,
                processing_key: Some(review_render_processing_key(0).to_string()),
                width: None,
                height: None,
                updated_at: now_string(),
            },
            ReviewProfileRender {
                profile_index: 1,
                profile_stem: "Fade".to_string(),
                display_name: None,
                status: ReviewRenderStatus::Done,
                output_path: Some(fade.clone()),
                error: None,
                duration_ms: Some(1),
                render_key: None,
                processing_key: Some(review_render_processing_key(1).to_string()),
                width: None,
                height: None,
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
