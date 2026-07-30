use super::prelude::*;
use super::{
    db::*, handle::*, model::*, preview::*, publish::*, sampler::*, scheduler::*, server::*,
    store::*,
};
use std::sync::Mutex;

fn profile(index: usize, stem: &str) -> ReviewProfile {
    ReviewProfile {
        index,
        identity: format!("test:{stem}"),
        selector: stem.to_string(),
        stem: stem.to_string(),
        sampler_added: false,
        enabled_by_default: true,
        configured_from_cli: true,
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
        contrast: 11.4,
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
        enabled: true,
        status: ReviewRenderStatus::Failed,
        output_path: Some(PathBuf::from("/out/day/Detailed/detailed.jpg")),
        error: Some("render failed".to_string()),
        duration_ms: Some(987),
        render_key: Some("render-key".to_string()),
        processing_key: Some("processing-key".to_string()),
        dcp_profile_filename: Some("Nikon Z 7 2 Adobe Standard.dcp".to_string()),
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
                focus_frame_width: Some(5504),
                focus_frame_height: Some(8256),
                focus_regions: vec![
                    GalleryFocusRegion {
                        x: 0.25,
                        y: 0.3,
                        width: 0.1,
                        height: 0.12,
                        primary: true,
                    },
                    GalleryFocusRegion {
                        x: 0.5,
                        y: 0.45,
                        width: 0.03,
                        height: 0.06,
                        primary: false,
                    },
                ],
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
                path: Some(PathBuf::from("/out/.mini-film-review-previews/preview.jpg")),
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
                    contrast: 25.0,
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

fn persisted_store(mut store: ReviewStore) -> ReviewStore {
    for profile in &mut store.profiles {
        profile.configured_from_cli = false;
    }
    store
}

fn migrated_pre_contrast_store(mut store: ReviewStore) -> ReviewStore {
    for image in &mut store.images {
        image.retouch.adjustments.clarity = 0.0;
    }
    store
}

fn rebase_review_store(
    mut store: ReviewStore,
    input_root: &Path,
    output_root: &Path,
) -> ReviewStore {
    for image in &mut store.images {
        image.raw_path = input_root.join(image.raw_path.strip_prefix("/in").unwrap());
        image.sooc_sidecar_path = image
            .sooc_sidecar_path
            .as_deref()
            .map(|path| input_root.join(path.strip_prefix("/in").unwrap()));
        image.preview.path = image
            .preview
            .path
            .as_deref()
            .map(|path| output_root.join(path.strip_prefix("/out").unwrap()));
        for render in &mut image.profiles {
            render.output_path = render
                .output_path
                .as_deref()
                .map(|path| output_root.join(path.strip_prefix("/out").unwrap()));
        }
    }
    store
}

fn rebase_review_cache_paths(
    mut store: ReviewStore,
    output_root: &Path,
    cache_root: &Path,
) -> ReviewStore {
    for image in &mut store.images {
        image.preview.path = image.preview.path.as_deref().map(|path| {
            path.strip_prefix(output_root)
                .ok()
                .filter(|relative| crate::app::cache::is_cache_relative_path(relative))
                .map_or_else(|| path.to_path_buf(), |relative| cache_root.join(relative))
        });
        for render in &mut image.profiles {
            render.output_path = render.output_path.as_deref().map(|path| {
                path.strip_prefix(output_root)
                    .ok()
                    .filter(|relative| crate::app::cache::is_cache_relative_path(relative))
                    .map_or_else(|| path.to_path_buf(), |relative| cache_root.join(relative))
            });
        }
    }
    store
}

fn migrated_pre_sampler_store(mut store: ReviewStore) -> ReviewStore {
    store = migrated_pre_contrast_store(persisted_store(store));
    for profile in &mut store.profiles {
        profile.identity = format!("legacy:{}:{}", profile.index, profile.selector.trim());
        profile.sampler_added = false;
        profile.enabled_by_default = true;
    }
    for image in &mut store.images {
        image.exif.focus_frame_width = None;
        image.exif.focus_frame_height = None;
        image.exif.focus_regions.clear();
    }
    store
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

fn test_processing_key(
    input: &Path,
    profile_index: usize,
    normalize_grain_mpix: Option<f64>,
) -> String {
    review_render_processing_key_for_input_with_options(
        input,
        profile_index,
        normalize_grain_mpix,
        &test_export_options(),
    )
}

fn test_handle(input: PathBuf, output: PathBuf, profiles: Vec<ReviewProfile>) -> ReviewHandle {
    let export = test_export_options();
    let (subscribers, _) = broadcast::channel(256);
    let database_runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap(),
    );
    let mut store = ReviewStore::new(profiles);
    store.render_export.clone_from(&export);
    let database = {
        let database_runtime = Arc::clone(&database_runtime);
        let input = input.clone();
        let output = output.clone();
        let store = store.clone();
        std::thread::spawn(move || {
            let (database, _) = database_runtime
                .block_on(ReviewDatabase::open_output(&input, &output))
                .unwrap();
            database_runtime
                .block_on(database.replace_store(&store))
                .unwrap();
            database
        })
        .join()
        .unwrap()
    };
    let state_path = database.path().to_path_buf();
    let cache_root = database.cache_root().to_path_buf();
    ReviewHandle {
        state: Arc::new(ArcSwap::from_pointee(store)),
        subscribers: Arc::new(subscribers),
        state_cache: Arc::new(ArcSwapOption::empty()),
        state_path,
        database,
        database_runtime,
        input_root: input.clone(),
        output_root: output.clone(),
        cache_root,
        hald_dir: output.join("hald"),
        profiles_root: input.clone(),
        hald_level: 16,
        rawtherapee: PathBuf::from("rawtherapee-cli"),
        dng_fallback: crate::app::dng::DngFallbackConfig::default(),
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
        normalize_grain_mpix: Some(mini_film::DEFAULT_GRAIN_REFERENCE_MPIX),
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
            Some(mini_film::DEFAULT_GRAIN_REFERENCE_MPIX),
        ),
        publish_jobs: Arc::new(ArcSwap::from_pointee(Vec::new())),
        next_publish_job_id: Arc::new(AtomicU64::new(1)),
        media_scheduler: Arc::new(ReviewMediaScheduler::default()),
        retouch_scheduler: Arc::new(ReviewRetouchScheduler::default()),
        codex: None,
        codex_scheduler: Arc::new(ReviewCodexScheduler::default()),
        invocation: None,
        panorama_config: crate::app::panorama::PanoramaConfig {
            hugin_bin_dir: None,
            rawtherapee: PathBuf::from("rawtherapee-cli"),
            dng_fallback: crate::app::dng::DngFallbackConfig::default(),
            convert: PathBuf::from("convert"),
            jobs: 1,
            color_noise_iso_threshold: 1600,
            lens_corrections: LensCorrections::default(),
            lcp_root: None,
        },
        panorama_capability: crate::app::panorama::PanoramaCapability {
            available: false,
            reason: Some("not available in tests".to_string()),
        },
        panorama_projects: Arc::new(ArcSwap::from_pointee(Vec::new())),
        panorama_operation: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        sampler_registry: Arc::new(ReviewSamplerRegistry::default()),
        trusted_input_sender: None,
        converted_input_sender: None,
    }
}

fn test_async_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
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
        dng_fallback: crate::app::dng::DngFallbackConfig::default(),
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
        normalize_grain_mpix: Some(mini_film::DEFAULT_GRAIN_REFERENCE_MPIX),
        write_metadata: false,
    }
}

fn profile_render(index: usize, stem: &str) -> ReviewProfileRender {
    ReviewProfileRender {
        profile_index: index,
        profile_stem: stem.to_string(),
        display_name: None,
        enabled: true,
        status: ReviewRenderStatus::Done,
        output_path: None,
        error: None,
        duration_ms: Some(1),
        render_key: None,
        processing_key: Some(review_render_processing_key(index).to_string()),
        dcp_profile_filename: None,
        width: None,
        height: None,
        updated_at: now_string(),
    }
}

fn priority_image(
    id: u64,
    raw: &str,
    capture_timestamp: i64,
    rating: u8,
    selected_profile_index: usize,
    profiles: Vec<ReviewProfileRender>,
) -> ReviewImage {
    ReviewImage {
        id,
        raw_path: PathBuf::from(raw),
        sooc_sidecar_path: None,
        relative_path: raw.trim_start_matches('/').to_string(),
        file_name: Path::new(raw)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string(),
        exif: GalleryExifData {
            capture_timestamp: Some(capture_timestamp),
            ..GalleryExifData::default()
        },
        preview: ReviewPreview::default(),
        selected_profile_index,
        rating,
        label: ReviewLabel::None,
        labels: Vec::new(),
        tags: Vec::new(),
        notes: String::new(),
        rating_source: ReviewMetadataSource::Default,
        tags_source: ReviewMetadataSource::Default,
        notes_source: ReviewMetadataSource::Default,
        codex: ReviewCodexAnalysis::default(),
        retouch: RetouchSettings::default(),
        publish_profile_indexes: None,
        profile_bw_filters: Vec::new(),
        profiles,
        updated_at: now_string(),
    }
}

#[test]
fn render_priority_snapshot_orders_six_buckets_and_enforces_eligibility() {
    let sampler_index = SAMPLER_PROFILE_INDEX_BASE;
    let mut disabled = profile_render(2, "Disabled");
    disabled.enabled = false;
    let mut sooc = profile_render(SOOC_PROFILE_INDEX, SOOC_PROFILE_STEM);
    sooc.enabled = false;
    let current = priority_image(
        10,
        "/in/current.NEF",
        100,
        5,
        sampler_index,
        vec![
            profile_render(0, "Classic"),
            profile_render(sampler_index, "Sampler"),
        ],
    );
    let visible = priority_image(
        20,
        "/in/visible.NEF",
        200,
        3,
        2,
        vec![
            profile_render(0, "Classic"),
            profile_render(1, "Fade"),
            disabled,
            sooc,
        ],
    );
    let later_hidden = priority_image(
        30,
        "/in/later.NEF",
        400,
        1,
        0,
        vec![profile_render(0, "Classic"), profile_render(1, "Fade")],
    );
    let earlier_hidden = priority_image(
        40,
        "/in/earlier.NEF",
        300,
        1,
        0,
        vec![profile_render(0, "Classic"), profile_render(1, "Fade")],
    );
    let mut store = ReviewStore::new(Vec::new());
    store.images = vec![later_hidden, visible, current, earlier_hidden];
    store.ui = ReviewUiState {
        current_image_id: Some(10),
        min_rating: 3,
    };

    let priorities = store.render_priority_snapshot();
    let ordered = [
        priorities.key_for(Some(10), Some(sampler_index), 50),
        priorities.key_for(Some(10), Some(0), 40),
        priorities.key_for(Some(20), Some(0), 30),
        priorities.key_for(Some(20), Some(1), 20),
        priorities.key_for(Some(30), Some(0), 10),
        priorities.key_for(Some(30), Some(1), 0),
    ]
    .into_iter()
    .map(Option::unwrap)
    .collect::<Vec<_>>();
    assert!(ordered.windows(2).all(|pair| pair[0] < pair[1]));

    assert!(priorities.key_for(Some(20), Some(2), 0).is_none());
    assert!(
        priorities
            .key_for(Some(20), Some(SOOC_PROFILE_INDEX), 0)
            .is_some()
    );
    assert!(
        priorities.key_for(Some(20), None, 0).unwrap()
            < priorities.key_for(Some(20), Some(1), 0).unwrap()
    );
    assert!(
        priorities.key_for(Some(40), Some(0), 999).unwrap()
            < priorities.key_for(Some(30), Some(0), 0).unwrap()
    );

    let captured_id = Some(10);
    store
        .images
        .iter_mut()
        .find(|image| image.id == 10)
        .unwrap()
        .raw_path = PathBuf::from("/in/current.dng");
    let rebound = store.render_priority_snapshot();
    assert!(
        rebound
            .key_for(captured_id, Some(sampler_index), 0)
            .is_some()
    );

    store.ui.current_image_id = Some(20);
    store
        .images
        .iter_mut()
        .find(|image| image.id == 20)
        .unwrap()
        .selected_profile_index = SOOC_PROFILE_INDEX;
    let selected_sooc = store.render_priority_snapshot();
    assert!(
        selected_sooc
            .key_for(Some(20), Some(SOOC_PROFILE_INDEX), 100)
            .unwrap()
            < selected_sooc.key_for(Some(20), Some(0), 0).unwrap()
    );

    store.ui.current_image_id = Some(10);
    store.ui.min_rating = 4;
    store
        .images
        .iter_mut()
        .find(|image| image.id == 40)
        .unwrap()
        .rating = 5;
    let filtered = store.render_priority_snapshot();
    assert!(
        filtered.key_for(Some(40), Some(0), 100).unwrap()
            < filtered
                .key_for(Some(20), Some(SOOC_PROFILE_INDEX), 0)
                .unwrap()
    );
    store
        .images
        .iter_mut()
        .find(|image| image.id == 20)
        .unwrap()
        .rating = 5;
    let rerated = store.render_priority_snapshot();
    assert!(
        rerated
            .key_for(Some(20), Some(SOOC_PROFILE_INDEX), 100)
            .unwrap()
            < rerated.key_for(Some(40), Some(0), 0).unwrap()
    );

    store
        .images
        .push(priority_image(50, "/in/direct.JPG", 500, 0, 0, Vec::new()));
    store.ui.current_image_id = Some(50);
    let direct = store.render_priority_snapshot();
    assert!(
        direct.key_for(Some(50), None, 100).unwrap()
            < direct
                .key_for(Some(20), Some(SOOC_PROFILE_INDEX), 0)
                .unwrap()
    );
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
fn scheduled_profile_completion_records_dcp_provenance() {
    let mut render = profile_render(SAMPLER_PROFILE_INDEX_BASE, "Sampler Film");
    render.status = ReviewRenderStatus::Processing;
    render.render_key = Some("render-key".to_string());

    apply_profile_retouch_done(
        &mut render,
        Path::new("/out/Sampler Film/frame.jpg"),
        Duration::from_millis(42),
        Some("Nikon Z 7 2 Adobe Standard.dcp"),
    );

    assert_eq!(render.status, ReviewRenderStatus::Done);
    assert_eq!(render.render_key, None);
    assert_eq!(
        render.dcp_profile_filename.as_deref(),
        Some("Nikon Z 7 2 Adobe Standard.dcp")
    );
}

#[test]
fn requeued_profile_keeps_dcp_provenance_visible() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let raw = input.join("frame.NEF");
    let rendered = output.join("Classic").join("frame.jpg");
    fs::write(&raw, b"raw").unwrap();
    fs::create_dir_all(rendered.parent().unwrap()).unwrap();
    fs::write(&rendered, b"rendered").unwrap();
    let handle = test_handle(input, output, vec![profile(0, "Classic")]);

    handle.record_discovered_raw(&raw).unwrap();
    handle.record_profile_queued(&raw, 0, &rendered).unwrap();
    handle
        .record_profile_done_with_dcp(
            &raw,
            0,
            &rendered,
            Duration::from_millis(42),
            Some("Nikon Z 7 2 Adobe Standard.dcp"),
        )
        .unwrap();
    handle.record_profile_queued(&raw, 0, &rendered).unwrap();

    let store = handle.store_snapshot();
    let image = &store.images[0];
    assert_eq!(
        effective_dcp_profile_filename(image, &image.profiles[0]),
        Some("Nikon Z 7 2 Adobe Standard.dcp")
    );
}

#[test]
fn dcp_provenance_fallback_is_limited_to_current_raw_profile_renders() {
    let mut store = fully_populated_review_store();
    let image = &mut store.images[0];
    let known = &mut image.profiles[0];
    known.status = ReviewRenderStatus::Done;
    known.render_key = None;
    known.processing_key = Some("raw-dcp-key".to_string());
    let mut sidebar_render = profile_render(9, "Sidebar Film");
    sidebar_render.status = ReviewRenderStatus::Queued;
    sidebar_render.processing_key = Some("raw-dcp-key".to_string());
    image.profiles.push(sidebar_render);

    assert_eq!(
        effective_dcp_profile_filename(image, &image.profiles[1]),
        Some("Nikon Z 7 2 Adobe Standard.dcp")
    );

    image.profiles[1].profile_index = SOOC_PROFILE_INDEX;
    assert_eq!(
        effective_dcp_profile_filename(image, &image.profiles[1]),
        None
    );

    image.profiles[1].profile_index = 9;
    image.raw_path = PathBuf::from("/in/one.JPG");
    assert_eq!(
        effective_dcp_profile_filename(image, &image.profiles[1]),
        None
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
    let fade_rendered = output.join("day").join("Fade").join("frame.jpg");
    fs::create_dir_all(rendered.parent().unwrap()).unwrap();
    fs::create_dir_all(fade_rendered.parent().unwrap()).unwrap();
    fs::write(&rendered, b"jpg").unwrap();
    fs::write(&fade_rendered, b"jpg").unwrap();

    let mut classic = profile(0, "Classic");
    classic.retouch_base = BasicRetouchAdjustments {
        exposure: 0.5,
        highlights: -18.0,
        shadows: 22.0,
        whites: 9.0,
        blacks: -7.0,
        ..BasicRetouchAdjustments::default()
    };
    let handle = test_handle(input, output, vec![classic, profile(1, "Fade")]);

    handle.record_discovered_raw(&raw).unwrap();
    handle
        .record_profile_done_with_dcp(
            &raw,
            0,
            &rendered,
            Duration::from_millis(42),
            Some("Nikon Z 7 2 Adobe Standard.dcp"),
        )
        .unwrap();
    handle
        .record_profile_done(&raw, 1, &fade_rendered, Duration::from_millis(43))
        .unwrap();
    let state = handle.api_state_value().unwrap();
    let text = serde_json::to_string(&state).unwrap();
    assert!(text.contains("\"selected_profile_index\":0"));
    assert!(text.contains("\"publish_profile_indexes\":[0,1]"));
    assert!(text.contains("\"status\":\"done\""));
    assert!(text.contains("\"dcp_profile_filename\":\"Nikon Z 7 2 Adobe Standard.dcp\""));
    assert!(text.contains("media/1/0"));
    assert_eq!(
        state["images"][0]["profiles"][1]["dcp_profile_filename"],
        json!("Nikon Z 7 2 Adobe Standard.dcp")
    );
    let base = &state["profiles"][0]["retouch_base"];
    assert_eq!(base["exposure"], json!(0.5));
    assert_eq!(base["highlights"], json!(-18.0));
    assert_eq!(base["shadows"], json!(22.0));
    assert_eq!(base["whites"], json!(9.0));
    assert_eq!(base["blacks"], json!(-7.0));
}

#[test]
fn dng_rebind_keeps_review_identity_and_user_data_in_sqlite() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let nef = input.join("one.NEF");
    let dng = input.join("one.dng");
    fs::write(&nef, b"unsupported nef").unwrap();
    fs::write(&dng, b"validated elsewhere before rebinding").unwrap();

    let mut before = rebase_review_store(fully_populated_review_store(), &input, &output);
    before.images.truncate(1);
    before.next_id = 2;
    before.ui.current_image_id = Some(1);
    before.images[0].raw_path = nef.clone();
    before.images[0].relative_path = "one.NEF".to_string();
    before.images[0].file_name = "one.NEF".to_string();
    let original = before.images[0].clone();

    let mut after = before.clone();
    assert!(after.rebind_raw_source(&input, &nef, &dng).unwrap());
    assert!(!after.rebind_raw_source(&input, &nef, &dng).unwrap());
    let rebound = &after.images[0];
    assert_eq!(rebound.id, original.id);
    assert_eq!(rebound.raw_path, dng);
    assert_eq!(rebound.relative_path, "one.dng");
    assert_eq!(rebound.file_name, "one.dng");
    assert_eq!(rebound.rating, original.rating);
    assert_eq!(rebound.label, original.label);
    assert_eq!(rebound.labels, original.labels);
    assert_eq!(rebound.tags, original.tags);
    assert_eq!(rebound.notes, original.notes);
    assert_eq!(rebound.rating_source, original.rating_source);
    assert_eq!(rebound.tags_source, original.tags_source);
    assert_eq!(rebound.notes_source, original.notes_source);
    assert_eq!(rebound.retouch, original.retouch);
    assert_eq!(
        rebound.publish_profile_indexes,
        original.publish_profile_indexes
    );
    assert_eq!(rebound.profile_bw_filters, original.profile_bw_filters);
    assert_eq!(
        rebound.selected_profile_index,
        original.selected_profile_index
    );
    assert_eq!(rebound.sooc_sidecar_path, original.sooc_sidecar_path);
    assert_eq!(
        rebound.exif.focus_regions.len(),
        original.exif.focus_regions.len()
    );
    for (rebound, original) in rebound
        .exif
        .focus_regions
        .iter()
        .zip(&original.exif.focus_regions)
    {
        assert!((rebound.x - original.x).abs() < 0.000_001);
        assert!((rebound.y - original.y).abs() < 0.000_001);
        assert!((rebound.width - original.width).abs() < 0.000_001);
        assert!((rebound.height - original.height).abs() < 0.000_001);
        assert_eq!(rebound.primary, original.primary);
    }
    assert_eq!(rebound.codex.flags, original.codex.flags);
    assert_eq!(rebound.codex.model, original.codex.model);
    assert_eq!(rebound.profiles.len(), original.profiles.len());
    assert_eq!(
        rebound.profiles[0].profile_index,
        original.profiles[0].profile_index
    );
    assert_eq!(rebound.profiles[0].enabled, original.profiles[0].enabled);

    let runtime = test_async_runtime();
    let database = runtime
        .block_on(ReviewDatabase::open_output(&input, &output))
        .unwrap()
        .0;
    runtime.block_on(database.replace_store(&before)).unwrap();
    runtime
        .block_on(database.apply_delta(&before, &after))
        .unwrap();
    drop(database);

    let restored = load_store_with_roots(&output.join(SQLITE_STATE_FILE), &input, &output)
        .unwrap()
        .unwrap();
    assert_eq!(restored.images.len(), 1);
    let restored = &restored.images[0];
    assert_eq!(restored.id, original.id);
    assert_eq!(restored.raw_path, dng);
    assert_eq!(restored.rating, original.rating);
    assert_eq!(restored.labels, original.labels);
    assert_eq!(restored.tags, original.tags);
    assert_eq!(restored.notes, original.notes);
    assert_eq!(restored.retouch, original.retouch);
    assert_eq!(
        restored.publish_profile_indexes,
        original.publish_profile_indexes
    );
    assert_eq!(restored.profile_bw_filters, original.profile_bw_filters);
    assert_eq!(restored.profiles.len(), original.profiles.len());
    assert_eq!(
        restored.profiles[0].profile_index,
        original.profiles[0].profile_index
    );
    assert_eq!(restored.profiles[0].enabled, original.profiles[0].enabled);
}

#[test]
fn stable_image_render_updates_follow_dng_rebind_without_creating_a_ghost_image() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let nef = input.join("one.NEF");
    let dng = input.join("one.dng");
    let rendered = output.join("Classic/one.jpg");
    fs::write(&nef, b"unsupported nef").unwrap();
    fs::write(&dng, b"validated dng").unwrap();
    fs::create_dir_all(rendered.parent().unwrap()).unwrap();
    fs::write(&rendered, b"rendered").unwrap();

    let handle = test_handle(input, output, vec![profile(0, "Classic")]);
    handle.record_discovered_raw(&nef).unwrap();
    handle.record_profile_queued(&nef, 0, &rendered).unwrap();
    assert!(handle.rebind_raw_source(&nef, &dng).unwrap());
    assert_eq!(
        handle.review_raw_for_image_id(1).as_deref(),
        Some(dng.as_path())
    );

    handle.record_profile_processing_for_image(1, 0).unwrap();
    handle
        .record_profile_done_with_dcp_for_image(1, 0, &rendered, Duration::from_millis(10), None)
        .unwrap();

    let store = handle.store_snapshot();
    assert_eq!(store.images.len(), 1);
    assert_eq!(store.images[0].id, 1);
    assert_eq!(store.images[0].raw_path, dng);
    assert_eq!(store.images[0].profiles[0].status, ReviewRenderStatus::Done);
    drop(store);

    handle
        .update_store(|store| {
            let render = &mut store.images[0].profiles[0];
            render.status = ReviewRenderStatus::Queued;
            render.render_key = Some("retouch-after-rebind".to_string());
            Ok(())
        })
        .unwrap();
    let snapshot = handle
        .retouch_task_snapshot(1, 0, "retouch-after-rebind")
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.raw, dng);
}

#[test]
fn dng_rebind_does_not_hide_a_changed_grain_normalization_key() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    fs::create_dir_all(&input).unwrap();
    let nef = input.join("one.NEF");
    let dng = input.join("one.dng");
    fs::write(&nef, b"unsupported nef").unwrap();
    fs::write(&dng, b"validated dng").unwrap();
    let profiles = vec![profile(0, "Classic")];
    let mut store = ReviewStore::new(profiles.clone());
    let old_processing_key =
        review_render_processing_key_for_input_with_normalization(&nef, 0, Some(12.0));
    {
        let image = store.ensure_image(&input, &nef).unwrap();
        image.profiles[0].status = ReviewRenderStatus::Done;
        image.profiles[0].processing_key = Some(old_processing_key.clone());
    }

    store.normalize_grain_mpix = Some(24.0);
    assert!(store.rebind_raw_source(&input, &nef, &dng).unwrap());
    assert_eq!(
        store.images[0].profiles[0].processing_key.as_deref(),
        Some(
            review_render_processing_key_for_input_with_normalization(&dng, 0, Some(24.0)).as_str()
        )
    );
    assert_eq!(
        store.images[0].profiles[0].status,
        ReviewRenderStatus::Missing
    );

    store.sync_profiles(profiles);
    assert_eq!(
        store.images[0].profiles[0].status,
        ReviewRenderStatus::Missing
    );
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
    assert_eq!(render.processing_key, Some(review_render_processing_key(0)));
}

#[test]
fn rendered_profile_processing_keys_track_input_sharpening_policy() {
    let normalization = grain_normalization_identity(Some(mini_film::DEFAULT_GRAIN_REFERENCE_MPIX));
    let export = review_export_processing_identity(&ExportOptions::default());
    for input in [Path::new("frame.jpg"), Path::new("frame.HEIC")] {
        assert_eq!(
            review_render_processing_key_for_input(input, 0),
            format!("profiled-compressed-render-v5-normalized-grain:{normalization}:{export}")
        );
    }
    assert_eq!(
        review_render_processing_key_for_input(Path::new("frame.NEF"), 0),
        format!("{RAW_RENDER_PIPELINE_KEY}:dcp-none:{normalization}:{export}")
    );
    assert_eq!(
        review_render_processing_key_for_input(Path::new("frame.TIFF"), 0),
        format!("profiled-tiff-render-v3-normalized-grain:{normalization}:{export}")
    );
}

#[test]
fn rendered_profile_processing_keys_track_grain_normalization() {
    for input in [
        Path::new("frame.jpg"),
        Path::new("frame.NEF"),
        Path::new("frame.TIFF"),
    ] {
        let default =
            review_render_processing_key_for_input_with_normalization(input, 0, Some(12.0));
        let custom =
            review_render_processing_key_for_input_with_normalization(input, 0, Some(24.0));
        let disabled = review_render_processing_key_for_input_with_normalization(input, 0, None);
        assert_ne!(default, custom);
        assert_ne!(default, disabled);
        assert_ne!(custom, disabled);

        assert_eq!(
            review_render_processing_key_for_input_with_normalization(
                input,
                SOOC_PROFILE_INDEX,
                Some(12.0),
            ),
            review_render_processing_key_for_input_with_normalization(
                input,
                SOOC_PROFILE_INDEX,
                None,
            ),
        );
    }
}

#[test]
fn rendered_profile_processing_keys_track_every_export_option() {
    let input = Path::new("frame.NEF");
    let base = ExportOptions::default();
    let expected = review_render_processing_key_for_input_with_options(input, 0, Some(12.0), &base);
    assert_eq!(
        expected,
        review_render_processing_key_for_input_with_options(input, 0, Some(12.0), &base)
    );

    let mut variants = Vec::new();
    let mut changed = base.clone();
    changed.jpg_quality = 87;
    variants.push(changed);
    let mut changed = base.clone();
    changed.resize = Some("3000x2000>".to_string());
    variants.push(changed);
    let mut changed = base.clone();
    changed.long_edge = Some(3000);
    variants.push(changed);
    let mut changed = base.clone();
    changed.max_width = Some(3000);
    variants.push(changed);
    let mut changed = base.clone();
    changed.max_height = Some(2000);
    variants.push(changed);
    let mut changed = base.clone();
    changed.jpeg_subsampling = crate::cli::JpegSubsampling::S420;
    variants.push(changed);
    let mut changed = base.clone();
    changed.strip_metadata = true;
    variants.push(changed);
    let mut changed = base.clone();
    changed.progressive_jpeg = true;
    variants.push(changed);

    for export in variants {
        assert_ne!(
            expected,
            review_render_processing_key_for_input_with_options(input, 0, Some(12.0), &export,)
        );
    }

    let sooc_default = review_render_processing_key_for_input_with_options(
        input,
        SOOC_PROFILE_INDEX,
        Some(12.0),
        &base,
    );
    let mut progressive = base.clone();
    progressive.progressive_jpeg = true;
    assert_ne!(
        sooc_default,
        review_render_processing_key_for_input_with_options(
            input,
            SOOC_PROFILE_INDEX,
            Some(12.0),
            &progressive,
        )
    );
    assert_eq!(
        sooc_default,
        review_render_processing_key_for_input_with_options(input, SOOC_PROFILE_INDEX, None, &base,)
    );
}

#[test]
fn sync_profiles_invalidates_completed_render_when_export_options_change() {
    let temp = tempfile::tempdir().unwrap();
    let raw = temp.path().join("frame.NEF");
    fs::write(&raw, b"raw").unwrap();
    let profiles = vec![profile(0, "Classic")];
    let mut store = ReviewStore::new(profiles.clone());
    let render = &mut store.ensure_image(temp.path(), &raw).unwrap().profiles[0];
    render.enabled = false;
    render.status = ReviewRenderStatus::Done;
    render.output_path = Some(temp.path().join("Classic/frame.jpg"));

    store.sync_profiles(profiles.clone());
    assert!(!store.images[0].profiles[0].enabled);
    assert_eq!(store.images[0].profiles[0].status, ReviewRenderStatus::Done);

    store.render_export.long_edge = Some(2048);
    store.render_export.progressive_jpeg = true;
    store.sync_profiles(profiles);

    let expected_processing_key = review_render_processing_key_for_input_with_options(
        &raw,
        0,
        store.normalize_grain_mpix,
        &store.render_export,
    );
    let render = &store.images[0].profiles[0];
    assert!(!render.enabled);
    assert_eq!(render.status, ReviewRenderStatus::Missing);
    assert_eq!(render.output_path, None);
    assert_eq!(
        render.processing_key.as_deref(),
        Some(expected_processing_key.as_str())
    );
}

#[test]
fn profiled_retouch_cache_keys_track_grain_normalization() {
    let retouch = RetouchSettings {
        adjustments: BasicRetouchAdjustments {
            exposure: 0.25,
            ..BasicRetouchAdjustments::default()
        },
        ..RetouchSettings::default()
    };
    let default = profile_render_key_value(
        &retouch,
        RetouchWhiteBalance::default(),
        BwFilter::None,
        Some(12.0),
        &review_render_processing_key_with_normalization(0, Some(12.0)),
    );
    let custom = profile_render_key_value(
        &retouch,
        RetouchWhiteBalance::default(),
        BwFilter::None,
        Some(24.0),
        &review_render_processing_key_with_normalization(0, Some(24.0)),
    );
    let disabled = profile_render_key_value(
        &retouch,
        RetouchWhiteBalance::default(),
        BwFilter::None,
        None,
        &review_render_processing_key_with_normalization(0, None),
    );

    assert_ne!(default, custom);
    assert_ne!(default, disabled);
    assert_ne!(custom, disabled);
}

#[test]
fn retouch_cache_keys_track_base_export_options() {
    let retouch = RetouchSettings {
        adjustments: BasicRetouchAdjustments {
            exposure: 0.25,
            ..BasicRetouchAdjustments::default()
        },
        ..RetouchSettings::default()
    };
    let input = Path::new("frame.NEF");
    let base_export = ExportOptions::default();
    let base_processing =
        review_render_processing_key_for_input_with_options(input, 0, Some(12.0), &base_export);
    let mut changed_export = base_export.clone();
    changed_export.long_edge = Some(2048);
    changed_export.progressive_jpeg = true;
    let changed_processing =
        review_render_processing_key_for_input_with_options(input, 0, Some(12.0), &changed_export);

    assert_ne!(
        profile_render_key_value(
            &retouch,
            RetouchWhiteBalance::default(),
            BwFilter::None,
            Some(12.0),
            &base_processing,
        ),
        profile_render_key_value(
            &retouch,
            RetouchWhiteBalance::default(),
            BwFilter::None,
            Some(12.0),
            &changed_processing,
        )
    );
    assert_ne!(
        retouch_render_key(&retouch, &base_export),
        retouch_render_key(&retouch, &changed_export)
    );
}

#[test]
fn sync_profiles_removes_internal_panorama_staging_records() {
    let mut store = fully_populated_review_store();
    let mut staging = store.images[0].clone();
    staging.id = 99;
    staging.raw_path = PathBuf::from("/in/Panoramas/.mini-film-panorama-result-AbC123.tif");
    staging.relative_path = "Panoramas/.mini-film-panorama-result-AbC123.tif".to_string();
    staging.file_name = ".mini-film-panorama-result-AbC123.tif".to_string();
    store.images.push(staging);
    store.ui.current_image_id = Some(99);

    store.sync_profiles(store.profiles.clone());

    assert!(
        store
            .images
            .iter()
            .all(|image| !is_internal_staging_input_file(&image.raw_path))
    );
    assert_ne!(store.ui.current_image_id, Some(99));
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
    save_store_with_roots(&state_path, &store, &input, &output).unwrap();

    let mut loaded = load_store_with_roots(&state_path, &input, &output)
        .unwrap()
        .unwrap();
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
    save_store_with_roots(&output.join(SQLITE_STATE_FILE), &store, &input, &output).unwrap();

    let mut restored = load_store_with_roots(&output.join(SQLITE_STATE_FILE), &input, &output)
        .unwrap()
        .unwrap();
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
        render.processing_key
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
fn review_state_seaorm_round_trips_every_normalized_collection() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let store = fully_populated_review_store();

    save_store(&state_path, &store).unwrap();
    let loaded = load_store(&state_path).unwrap().unwrap();

    assert_eq!(
        serde_json::to_value(&loaded).unwrap(),
        serde_json::to_value(persisted_store(store)).unwrap()
    );
    let facts = database_facts(&state_path).unwrap();
    assert_eq!(facts.schema_version, 20);
    assert!(facts.has_seaql_ledger);
    assert_eq!(facts.seaql_migration_count, 9);
    assert_eq!(
        facts.seaql_migrations,
        [
            "m20260718_000001_v18_baseline",
            "m20260719_000002_panorama_projects",
            "m20260721_000003_review_sampler",
            "m20260726_000004_focus_regions",
            "m20260726_000005_relative_paths",
            "m20260726_000006_cache_root",
            "m20260727_000007_auto_import",
            "m20260727_000008_retouch_contrast",
            "m20260729_000009_dcp_provenance",
        ]
    );
    assert!(!facts.has_legacy_ledger);
    assert!(!facts.has_review_state);
    assert!(!facts.has_json_storage_columns);
    assert_eq!(facts.counts["review_settings"], 1);
    assert_eq!(facts.counts["profiles"], 2);
    assert_eq!(facts.counts["images"], 3);
    assert_eq!(facts.counts["tags"], 2);
    assert_eq!(facts.counts["image_tags"], 3);
    assert_eq!(facts.counts["image_profile_renders"], 1);
    assert_eq!(facts.counts["image_profile_bw_filters"], 3);
    assert_eq!(facts.counts["image_focus_regions"], 2);
    assert_eq!(facts.counts["profile_pp3_sections"], 2);
    assert_eq!(facts.counts["profile_pp3_entries"], 2);
    assert_eq!(facts.counts["panorama_projects"], 0);
    assert_eq!(facts.counts["panorama_project_images"], 0);
    assert_eq!(facts.counts["panorama_previews"], 0);
    assert_eq!(facts.counts["auto_import_devices"], 0);
    assert_eq!(facts.counts["auto_import_storages"], 0);
    assert_eq!(facts.counts["auto_import_groups"], 0);
    assert_eq!(facts.counts["auto_import_assets"], 0);
    assert_eq!(facts.counts["auto_import_sources"], 0);
    assert_eq!(facts.indexes.len(), 28);
    let paths = stored_path_facts(&state_path).unwrap();
    assert_eq!(paths.input_root, "/in");
    assert_eq!(paths.output_root, "/out");
    assert!(
        Path::new(&paths.cache_root)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("mini-film."))
    );
    assert!(
        paths
            .source_paths
            .iter()
            .chain(&paths.output_paths)
            .all(|path| Path::new(path).is_relative())
    );
    assert_domain_constraints(&state_path).unwrap();
}

#[test]
fn normalized_v11_database_is_backed_up_and_adopted_losslessly_once() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let backup_path = temp.path().join("mini-film-review.sqlite.pre-seaorm-v11");
    let store = fully_populated_review_store();
    make_v11_database(&state_path, &store).unwrap();

    let before_facts = database_facts(&state_path).unwrap();
    assert_eq!(before_facts.schema_version, 11);
    assert!(before_facts.has_legacy_ledger);
    assert!(!before_facts.has_seaql_ledger);
    assert!(!backup_path.exists());

    let loaded = load_store(&state_path).unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(&loaded).unwrap(),
        serde_json::to_value(migrated_pre_contrast_store(persisted_store(store.clone()))).unwrap()
    );
    assert!(backup_path.is_file());
    let backup_bytes = fs::read(&backup_path).unwrap();

    let after_facts = database_facts(&state_path).unwrap();
    assert_eq!(after_facts.schema_version, 20);
    assert!(!after_facts.has_legacy_ledger);
    assert!(after_facts.has_seaql_ledger);
    assert_eq!(after_facts.seaql_migration_count, 9);
    assert_eq!(
        after_facts.seaql_migrations,
        [
            "m20260718_000001_v18_baseline",
            "m20260719_000002_panorama_projects",
            "m20260721_000003_review_sampler",
            "m20260726_000004_focus_regions",
            "m20260726_000005_relative_paths",
            "m20260726_000006_cache_root",
            "m20260727_000007_auto_import",
            "m20260727_000008_retouch_contrast",
            "m20260729_000009_dcp_provenance",
        ]
    );
    assert!(!after_facts.has_json_storage_columns);

    let loaded_again = load_store(&state_path).unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(&loaded_again).unwrap(),
        serde_json::to_value(migrated_pre_contrast_store(persisted_store(store))).unwrap()
    );
    assert_eq!(fs::read(&backup_path).unwrap(), backup_bytes);
    let backup_facts = database_facts(&backup_path).unwrap();
    assert_eq!(backup_facts.schema_version, 11);
    assert!(backup_facts.has_legacy_ledger);
    assert!(!backup_facts.has_seaql_ledger);
}

#[test]
fn pre_release_two_entry_seaorm_ledger_is_collapsed_without_data_loss() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let store = fully_populated_review_store();
    make_pre_release_seaorm_database(&state_path, &store).unwrap();

    let before = database_facts(&state_path).unwrap();
    assert_eq!(
        before.seaql_migrations,
        [
            "m20260718_000001_create_review_schema",
            "m20260718_000002_adopt_seaorm",
        ]
    );

    let loaded = load_store(&state_path).unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(&loaded).unwrap(),
        serde_json::to_value(migrated_pre_contrast_store(persisted_store(store))).unwrap()
    );
    let after = database_facts(&state_path).unwrap();
    assert_eq!(after.seaql_migration_count, 9);
    assert_eq!(
        after.seaql_migrations,
        [
            "m20260718_000001_v18_baseline",
            "m20260719_000002_panorama_projects",
            "m20260721_000003_review_sampler",
            "m20260726_000004_focus_regions",
            "m20260726_000005_relative_paths",
            "m20260726_000006_cache_root",
            "m20260727_000007_auto_import",
            "m20260727_000008_retouch_contrast",
            "m20260729_000009_dcp_provenance",
        ]
    );
}

#[test]
fn schema_v12_migrates_to_panorama_schema_without_review_data_loss() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let store = fully_populated_review_store();
    make_schema_v12_database(&state_path, &store).unwrap();

    let loaded = load_store(&state_path).unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(&loaded).unwrap(),
        serde_json::to_value(migrated_pre_sampler_store(store)).unwrap()
    );
    let after = database_facts(&state_path).unwrap();
    assert_eq!(after.schema_version, 20);
    assert_eq!(after.seaql_migration_count, 9);
    assert_eq!(after.counts["panorama_projects"], 0);
    assert_eq!(after.counts["panorama_project_images"], 0);
    assert_eq!(after.counts["panorama_previews"], 0);
}

#[test]
fn schema_v13_migrates_sampler_state_without_review_data_loss() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let mut store = fully_populated_review_store();
    store.images[0].publish_profile_indexes = Some(Vec::new());
    store.images[0].profiles[0].enabled = false;
    let expected = migrated_pre_sampler_store(store.clone());
    make_schema_v13_database(&state_path, &store).unwrap();

    let loaded = load_store(&state_path).unwrap().unwrap();

    assert_eq!(
        serde_json::to_value(&loaded).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(!loaded.images[0].profiles[0].enabled);
    assert!(loaded.profiles.iter().all(|profile| {
        !profile.sampler_added
            && profile.enabled_by_default
            && profile.identity.starts_with("legacy:")
    }));
    let after = database_facts(&state_path).unwrap();
    assert_eq!(after.schema_version, 20);
    assert_eq!(after.seaql_migration_count, 9);
    assert_eq!(after.indexes.len(), 28);
}

#[test]
fn schema_v14_adds_focus_region_storage_without_review_data_loss() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let mut store = fully_populated_review_store();
    for image in &mut store.images {
        image.exif.focus_frame_width = None;
        image.exif.focus_frame_height = None;
        image.exif.focus_regions.clear();
    }
    make_schema_v14_database(&state_path, &store).unwrap();

    let loaded = load_store(&state_path).unwrap().unwrap();

    assert_eq!(
        serde_json::to_value(&loaded).unwrap(),
        serde_json::to_value(migrated_pre_contrast_store(persisted_store(store))).unwrap()
    );
    let after = database_facts(&state_path).unwrap();
    assert_eq!(after.schema_version, 20);
    assert_eq!(after.seaql_migration_count, 9);
    assert_eq!(after.counts["image_focus_regions"], 0);
    assert_eq!(after.indexes.len(), 28);
}

#[test]
fn schema_v15_paths_migrate_and_rebase_after_input_and_output_move() {
    let temp = tempfile::tempdir().unwrap();
    let old_output = temp.path().join("old-output");
    let new_input = temp.path().join("moved-input");
    let new_output = temp.path().join("moved-output");
    fs::create_dir_all(&old_output).unwrap();
    fs::create_dir_all(&new_input).unwrap();
    fs::create_dir_all(&new_output).unwrap();
    let old_state = old_output.join(SQLITE_STATE_FILE);
    let new_state = new_output.join(SQLITE_STATE_FILE);
    let store = fully_populated_review_store();
    make_schema_v15_database(&old_state, &store).unwrap();

    fs::rename(&old_state, &new_state).unwrap();
    let loaded = load_store_with_roots(&new_state, &new_input, &new_output)
        .unwrap()
        .unwrap();
    let paths = stored_path_facts(&new_state).unwrap();
    let expected = rebase_review_cache_paths(
        rebase_review_store(
            migrated_pre_contrast_store(persisted_store(store)),
            &new_input,
            &new_output,
        ),
        &new_output,
        Path::new(&paths.cache_root),
    );
    assert_eq!(
        serde_json::to_value(loaded).unwrap(),
        serde_json::to_value(expected).unwrap()
    );

    let facts = database_facts(&new_state).unwrap();
    assert_eq!(facts.schema_version, 20);
    assert_eq!(facts.seaql_migration_count, 9);
    assert_eq!(paths.input_root, new_input.to_string_lossy());
    assert_eq!(paths.output_root, new_output.to_string_lossy());
    assert!(
        paths
            .source_paths
            .iter()
            .chain(&paths.output_paths)
            .all(|path| Path::new(path).is_relative())
    );
}

#[test]
fn schema_v15_render_paths_infer_moved_output_without_preview_cache() {
    let temp = tempfile::tempdir().unwrap();
    let old_output = temp.path().join("old-output");
    let new_input = temp.path().join("moved-input");
    let new_output = temp.path().join("moved-output");
    fs::create_dir_all(&old_output).unwrap();
    fs::create_dir_all(&new_input).unwrap();
    fs::create_dir_all(&new_output).unwrap();
    let old_state = old_output.join(SQLITE_STATE_FILE);
    let new_state = new_output.join(SQLITE_STATE_FILE);
    let mut store = fully_populated_review_store();
    for image in &mut store.images {
        image.preview.path = None;
    }
    make_schema_v15_database(&old_state, &store).unwrap();

    fs::rename(&old_state, &new_state).unwrap();
    let loaded = load_store_with_roots(&new_state, &new_input, &new_output)
        .unwrap()
        .unwrap();
    let expected = rebase_review_store(
        migrated_pre_contrast_store(persisted_store(store)),
        &new_input,
        &new_output,
    );
    assert_eq!(
        serde_json::to_value(loaded).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(
        stored_path_facts(&new_state)
            .unwrap()
            .output_paths
            .iter()
            .all(|path| Path::new(path).is_relative())
    );
}

#[test]
fn schema_v17_adds_normalized_auto_import_tables_without_review_data_loss() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let store = fully_populated_review_store();
    make_schema_v17_database(&state_path, &store).unwrap();

    let before = database_facts(&state_path).unwrap();
    assert_eq!(before.schema_version, 17);
    assert_eq!(before.seaql_migration_count, 6);
    assert_eq!(before.counts["auto_import_devices"], 0);

    let loaded = load_store(&state_path).unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(&loaded).unwrap(),
        serde_json::to_value(migrated_pre_contrast_store(persisted_store(store))).unwrap()
    );
    let after = database_facts(&state_path).unwrap();
    assert_eq!(after.schema_version, 20);
    assert_eq!(after.seaql_migration_count, 9);
    assert_eq!(after.counts["auto_import_devices"], 0);
    assert_eq!(after.counts["auto_import_storages"], 0);
    assert_eq!(after.counts["auto_import_groups"], 0);
    assert_eq!(after.counts["auto_import_assets"], 0);
    assert_eq!(after.counts["auto_import_sources"], 0);
    assert_eq!(after.indexes.len(), 28);
}

#[test]
fn schema_v18_splits_contrast_from_clarity_without_losing_old_edits() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let store = fully_populated_review_store();
    make_schema_v18_database(&state_path, &store).unwrap();

    let loaded = load_store(&state_path).unwrap().unwrap();
    let expected = migrated_pre_contrast_store(persisted_store(store));
    assert_eq!(
        serde_json::to_value(&loaded).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    let edited = loaded
        .images
        .iter()
        .find(|image| image.retouch.adjustments.contrast != 0.0)
        .unwrap();
    assert_eq!(edited.retouch.adjustments.contrast, 25.0);
    assert_eq!(edited.retouch.adjustments.clarity, 0.0);
    let detailed = loaded
        .profiles
        .iter()
        .find(|profile| profile.index == 7)
        .unwrap();
    assert_eq!(detailed.retouch_base.contrast, 11.4);
    assert_eq!(detailed.retouch_base.clarity, 15.0);

    let after = database_facts(&state_path).unwrap();
    assert_eq!(after.schema_version, 20);
    assert_eq!(after.seaql_migration_count, 9);
}

#[test]
fn relative_path_database_rebases_when_roots_move_again() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    let new_input = temp.path().join("moved-input");
    let new_output = temp.path().join("moved-output");
    fs::create_dir_all(&new_input).unwrap();
    fs::create_dir_all(&new_output).unwrap();
    let store = fully_populated_review_store();
    save_store(&state_path, &store).unwrap();

    let loaded = load_store_with_roots(&state_path, &new_input, &new_output)
        .unwrap()
        .unwrap();
    let paths = stored_path_facts(&state_path).unwrap();
    let expected = rebase_review_cache_paths(
        rebase_review_store(persisted_store(store), &new_input, &new_output),
        &new_output,
        Path::new(&paths.cache_root),
    );
    assert_eq!(
        serde_json::to_value(loaded).unwrap(),
        serde_json::to_value(expected).unwrap()
    );

    assert_eq!(paths.input_root, new_input.to_string_lossy());
    assert_eq!(paths.output_root, new_output.to_string_lossy());
    assert!(
        paths
            .source_paths
            .iter()
            .chain(&paths.output_paths)
            .all(|path| Path::new(path).is_relative())
    );
}

#[test]
fn sampler_profiles_persist_with_current_and_all_availability() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    let state_path = temp.path().join(SQLITE_STATE_FILE);
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let first = input.join("first.jpg");
    let second = input.join("second.jpg");
    let third = input.join("third.jpg");
    fs::write(&first, b"jpg").unwrap();
    fs::write(&second, b"jpg").unwrap();
    fs::write(&third, b"jpg").unwrap();

    let mut store = ReviewStore::new(Vec::new());
    let first_id = store.ensure_image(&input, &first).unwrap().id;
    let mut sampler_profile = profile(0, "Portra 400");
    sampler_profile.identity = "xmp:portra-400".to_string();
    sampler_profile.selector = "/profiles/Portra 400.xmp".to_string();
    sampler_profile.sampler_added = true;
    sampler_profile.enabled_by_default = false;
    sampler_profile.configured_from_cli = false;
    let profile_index = store.ensure_sampler_profile(sampler_profile).unwrap();
    assert_eq!(profile_index, SAMPLER_PROFILE_INDEX_BASE);
    assert!(!store.images[0].profiles[0].enabled);

    store
        .set_profile_enabled_for_image(first_id, profile_index, true)
        .unwrap();
    let second_id = store.ensure_image(&input, &second).unwrap().id;
    assert!(
        store.images[0]
            .profiles
            .iter()
            .any(|render| render.profile_index == profile_index && render.enabled)
    );
    assert!(
        store.images[1]
            .profiles
            .iter()
            .any(|render| render.profile_index == profile_index && !render.enabled)
    );

    save_store_with_roots(&state_path, &store, &input, &output).unwrap();
    let mut restored = load_store_with_roots(&state_path, &input, &output)
        .unwrap()
        .unwrap();
    restored.sync_profiles(Vec::new());
    assert_eq!(restored.profiles.len(), 1);
    assert!(restored.profiles[0].sampler_added);
    assert!(!restored.profiles[0].configured_from_cli);
    assert!(
        restored
            .images
            .iter()
            .find(|image| image.id == first_id)
            .unwrap()
            .profiles
            .iter()
            .any(|render| render.profile_index == profile_index && render.enabled)
    );
    assert!(
        restored
            .images
            .iter()
            .find(|image| image.id == second_id)
            .unwrap()
            .profiles
            .iter()
            .any(|render| render.profile_index == profile_index && !render.enabled)
    );

    restored
        .set_profile_enabled_for_all(profile_index, true)
        .unwrap();
    let third_id = restored.ensure_image(&input, &third).unwrap().id;
    assert!(restored.profiles[0].enabled_by_default);
    assert!(restored.images.iter().all(|image| {
        image
            .profiles
            .iter()
            .any(|render| render.profile_index == profile_index && render.enabled)
    }));
    assert_eq!(
        effective_publish_profile_indexes(
            restored
                .images
                .iter()
                .find(|image| image.id == third_id)
                .unwrap()
        ),
        vec![profile_index]
    );
}

#[test]
fn opening_old_output_catalog_moves_it_to_input_without_data_loss() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let old_state = output.join(SQLITE_STATE_FILE);
    let new_state = input.join(SQLITE_STATE_FILE);
    let store = rebase_review_store(fully_populated_review_store(), &input, &output);
    let expected = persisted_store(store.clone());
    save_store_with_roots(&old_state, &store, &input, &output).unwrap();
    let facts_before = database_facts(&old_state).unwrap();

    let runtime = test_async_runtime();
    let (database, loaded) = runtime
        .block_on(ReviewDatabase::open_output(&input, &output))
        .unwrap();
    let expected = rebase_review_cache_paths(expected, &output, database.cache_root());

    assert_eq!(database.path(), new_state);
    assert_eq!(
        serde_json::to_value(loaded.unwrap()).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    drop(database);
    assert!(new_state.is_file());
    assert!(
        fs::symlink_metadata(&old_state)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::canonicalize(&old_state).unwrap(),
        fs::canonicalize(&new_state).unwrap()
    );
    let canonical_state = fs::canonicalize(&new_state).unwrap();
    assert_eq!(
        resolve_review_state_for_publish(&old_state, &input, &output).unwrap(),
        canonical_state
    );
    assert_eq!(
        resolve_review_state_for_publish(&new_state, &input, &output).unwrap(),
        canonical_state
    );

    let restored = load_store_with_roots(&new_state, &input, &output)
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    let facts_after = database_facts(&new_state).unwrap();
    assert_eq!(facts_after.schema_version, facts_before.schema_version);
    assert_eq!(facts_after.seaql_migrations, facts_before.seaql_migrations);
    assert_eq!(facts_after.counts, facts_before.counts);
    assert_eq!(facts_after.indexes, facts_before.indexes);
}

#[test]
fn reopening_after_input_move_repairs_catalog_link_and_rebases_paths() {
    let temp = tempfile::tempdir().unwrap();
    let old_input = temp.path().join("old-input");
    let new_input = temp.path().join("new-input");
    let output = temp.path().join("output");
    fs::create_dir_all(&old_input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let base_store = fully_populated_review_store();
    let store = rebase_review_store(base_store.clone(), &old_input, &output);

    let runtime = test_async_runtime();
    let (database, _) = runtime
        .block_on(ReviewDatabase::open_output(&old_input, &output))
        .unwrap();
    runtime.block_on(database.replace_store(&store)).unwrap();
    drop(database);

    let output_state = output.join(SQLITE_STATE_FILE);
    fs::rename(&old_input, &new_input).unwrap();
    assert!(fs::canonicalize(&output_state).is_err());

    let (database, loaded) = runtime
        .block_on(ReviewDatabase::open_output(&new_input, &output))
        .unwrap();
    let expected = rebase_review_cache_paths(
        rebase_review_store(persisted_store(base_store), &new_input, &output),
        &output,
        database.cache_root(),
    );
    assert_eq!(
        serde_json::to_value(loaded.unwrap()).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert_eq!(database.path(), new_input.join(SQLITE_STATE_FILE));
    assert_eq!(
        fs::canonicalize(&output_state).unwrap(),
        fs::canonicalize(new_input.join(SQLITE_STATE_FILE)).unwrap()
    );
    let paths = stored_path_facts(database.path()).unwrap();
    assert_eq!(paths.input_root, new_input.to_string_lossy());
    assert_eq!(paths.output_root, output.to_string_lossy());
}

#[test]
fn legacy_output_caches_migrate_and_remain_disposable() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let mut store = rebase_review_store(fully_populated_review_store(), &input, &output);
    let legacy_preview = output
        .join(".mini-film-review-previews")
        .join("preview.jpg");
    let legacy_retouch = output
        .join("day")
        .join("Detailed")
        .join(".detailed.retouch-cache-key.jpg");
    store.images[0].preview.path = Some(legacy_preview.clone());
    store.images[0].profiles[0].output_path = Some(legacy_retouch.clone());

    let runtime = test_async_runtime();
    let (database, _) = runtime
        .block_on(ReviewDatabase::open_output(&input, &output))
        .unwrap();
    runtime.block_on(database.replace_store(&store)).unwrap();
    let cache_root = database.cache_root().to_path_buf();
    let timestamp = "2026-07-26T12:34:56+02:00".to_string();
    let mut panorama = ReviewPanoramaProject {
        id: 0,
        name: "Disposable preview".to_string(),
        status: ReviewPanoramaStatus::Ready,
        matching_mode: PanoramaMatchingMode::Automatic,
        selected_projection: Some(PanoramaProjection::Cylindrical),
        output_path: None,
        result_image_id: None,
        progress_stage: None,
        progress_completed: 0,
        progress_total: 0,
        error: None,
        created_at: timestamp.clone(),
        updated_at: timestamp.clone(),
        image_ids: vec![1],
        previews: vec![ReviewPanoramaPreview {
            matching_mode: PanoramaMatchingMode::Automatic,
            projection: PanoramaProjection::Cylindrical,
            status: ReviewPanoramaPreviewStatus::Done,
            path: Some(output.join(".mini-film-panoramas").join("cylindrical.jpg")),
            cache_key: Some("panorama-key".to_string()),
            duration_ms: Some(10),
            error: None,
            updated_at: timestamp,
        }],
    };
    runtime
        .block_on(database.create_panorama_project(&mut panorama))
        .unwrap();
    drop(database);

    for (path, bytes) in [
        (&legacy_preview, b"preview".as_slice()),
        (&legacy_retouch, b"retouch".as_slice()),
        (
            &output.join(".mini-film-panoramas").join("cylindrical.jpg"),
            b"panorama".as_slice(),
        ),
        (
            &output
                .join(".mini-film-sampler")
                .join("review-sampler-v1")
                .join("source.tif"),
            b"sampler".as_slice(),
        ),
        (
            &output
                .join("album")
                .join(".mini-film-profile-inputs")
                .join("source.tif"),
            b"profile input".as_slice(),
        ),
        (
            &output
                .join(".mini-film-gallery-thumbnails")
                .join("Detailed")
                .join("thumb.jpg"),
            b"thumbnail".as_slice(),
        ),
    ] {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let gallery = output.join("layout").join("index.html");
    fs::create_dir_all(gallery.parent().unwrap()).unwrap();
    fs::write(
        &gallery,
        "../.mini-film-gallery-thumbnails/Detailed/thumb.jpg",
    )
    .unwrap();

    let (database, loaded) = runtime
        .block_on(ReviewDatabase::open_output(&input, &output))
        .unwrap();
    assert_eq!(database.cache_root(), cache_root);
    let loaded = loaded.unwrap();
    let migrated_preview = cache_root
        .join(".mini-film-review-previews")
        .join("preview.jpg");
    let migrated_retouch = cache_root
        .join(".mini-film-retouch")
        .join("day")
        .join("Detailed")
        .join(".detailed.retouch-cache-key.jpg");
    assert_eq!(
        loaded.images[0].preview.path.as_deref(),
        Some(migrated_preview.as_path())
    );
    assert_eq!(
        loaded.images[0].profiles[0].output_path.as_deref(),
        Some(migrated_retouch.as_path())
    );
    assert_eq!(fs::read(&migrated_preview).unwrap(), b"preview");
    assert_eq!(fs::read(&migrated_retouch).unwrap(), b"retouch");
    assert!(
        cache_root
            .join(".mini-film-sampler/review-sampler-v1/source.tif")
            .is_file()
    );
    assert!(
        cache_root
            .join(".mini-film-legacy-output/album/.mini-film-profile-inputs/source.tif")
            .is_file()
    );
    assert!(output.join("thumbnails/Detailed/thumb.jpg").is_file());
    assert_eq!(
        fs::read_to_string(&gallery).unwrap(),
        "../thumbnails/Detailed/thumb.jpg"
    );
    assert!(
        walkdir::WalkDir::new(&output)
            .min_depth(1)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .all(|entry| !crate::app::cache::is_cache_directory_name(entry.file_name()))
    );
    let projects = runtime.block_on(database.load_panorama_projects()).unwrap();
    assert_eq!(
        projects[0].previews[0].path.as_deref(),
        Some(
            cache_root
                .join(".mini-film-panoramas/cylindrical.jpg")
                .as_path()
        )
    );
    let stored_paths = stored_path_facts(database.path()).unwrap();
    assert_eq!(stored_paths.cache_root, cache_root.to_string_lossy());
    assert!(
        stored_paths
            .output_paths
            .iter()
            .filter(|path| path.contains(".mini-film-"))
            .all(|path| path.starts_with(".mini-film-cache/"))
    );
    drop(database);

    fs::remove_dir_all(&cache_root).unwrap();
    let (database, reloaded) = runtime
        .block_on(ReviewDatabase::open_output(&input, &output))
        .unwrap();
    assert_eq!(database.cache_root(), cache_root);
    assert!(cache_root.is_dir());
    assert_eq!(
        serde_json::to_value(reloaded.unwrap()).unwrap(),
        serde_json::to_value(loaded).unwrap()
    );
    assert!(!migrated_preview.exists());
    assert!(!migrated_retouch.exists());
}

#[test]
fn panorama_projects_round_trip_normalized_relationships() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let runtime = test_async_runtime();
    let (database, _) = runtime
        .block_on(ReviewDatabase::open_output(&input, &output))
        .unwrap();
    let store = rebase_review_store(fully_populated_review_store(), &input, &output);
    runtime.block_on(database.replace_store(&store)).unwrap();
    let timestamp = "2026-07-19T12:34:56+02:00".to_string();
    let mut project = ReviewPanoramaProject {
        id: 0,
        name: "Alps sweep".to_string(),
        status: ReviewPanoramaStatus::Ready,
        matching_mode: PanoramaMatchingMode::MultiRow,
        selected_projection: Some(PanoramaProjection::Panini),
        output_path: Some(input.join("Panoramas/Alps sweep.tif")),
        result_image_id: Some(3),
        progress_stage: Some("complete".to_string()),
        progress_completed: 4,
        progress_total: 4,
        error: None,
        created_at: timestamp.clone(),
        updated_at: timestamp.clone(),
        image_ids: vec![3, 1, 2],
        previews: vec![ReviewPanoramaPreview {
            matching_mode: PanoramaMatchingMode::MultiRow,
            projection: PanoramaProjection::Panini,
            status: ReviewPanoramaPreviewStatus::Done,
            path: Some(output.join(".mini-film-panoramas/panini.jpg")),
            cache_key: Some("preview-key".to_string()),
            duration_ms: Some(4321),
            error: None,
            updated_at: timestamp,
        }],
    };

    runtime
        .block_on(database.create_panorama_project(&mut project))
        .unwrap();
    assert!(project.id > 0);
    let loaded = runtime.block_on(database.load_panorama_projects()).unwrap();
    assert_eq!(loaded, vec![project.clone()]);

    project.status = ReviewPanoramaStatus::Complete;
    project.progress_completed = 1;
    project.progress_total = 1;
    runtime
        .block_on(database.save_panorama_project(&project))
        .unwrap();
    assert_eq!(
        runtime.block_on(database.load_panorama_projects()).unwrap(),
        vec![project]
    );

    let facts = database_facts(&output.join(SQLITE_STATE_FILE)).unwrap();
    assert_eq!(facts.counts["panorama_projects"], 1);
    assert_eq!(facts.counts["panorama_project_images"], 3);
    assert_eq!(facts.counts["panorama_previews"], 1);
}

#[test]
fn json_only_state_is_rejected_without_modification() {
    let temp = tempfile::tempdir().unwrap();
    let json_path = temp.path().join("mini-film-review.json");
    let sqlite_path = temp.path().join(SQLITE_STATE_FILE);
    let bytes = serde_json::to_vec_pretty(&fully_populated_review_store()).unwrap();
    fs::write(&json_path, &bytes).unwrap();

    let error = load_store(&json_path).unwrap_err();

    assert!(
        format!("{error:#}").contains("final mini-film 17.x"),
        "{error:#}"
    );
    assert_eq!(fs::read(&json_path).unwrap(), bytes);
    assert!(!sqlite_path.exists());
}

#[test]
fn sqlite_v1_through_v10_are_rejected_without_modification() {
    for version in 1..=10 {
        let temp = tempfile::tempdir().unwrap();
        let state_path = temp.path().join(SQLITE_STATE_FILE);
        make_legacy_version_database(&state_path, version).unwrap();
        let before = fs::read(&state_path).unwrap();

        let error = load_store(&state_path).unwrap_err();

        assert!(
            format!("{error:#}").contains("final mini-film 17.x"),
            "v{version}: {error:#}"
        );
        assert_eq!(fs::read(&state_path).unwrap(), before, "schema v{version}");
        assert!(
            !temp
                .path()
                .join("mini-film-review.sqlite.pre-seaorm-v11")
                .exists()
        );
    }
}

#[test]
fn incremental_store_delta_preserves_complete_state() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let old_state_path = output.join(SQLITE_STATE_FILE);
    let state_path = input.join(SQLITE_STATE_FILE);
    let before = rebase_review_store(fully_populated_review_store(), &input, &output);
    save_store_with_roots(&old_state_path, &before, &input, &output).unwrap();
    let facts_before = database_facts(&old_state_path).unwrap();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (database, loaded) = runtime
        .block_on(ReviewDatabase::open_output(&input, &output))
        .unwrap();
    let loaded = loaded.unwrap();
    let mut after = loaded.clone();
    after.images[0].rating = 2;
    after.images[0].rating_source = ReviewMetadataSource::Manual;
    after.images[0].updated_at = now_string();
    runtime
        .block_on(database.apply_delta(&loaded, &after))
        .unwrap();
    drop(database);

    let restored = load_store_with_roots(&state_path, &input, &output)
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(&restored).unwrap(),
        serde_json::to_value(&after).unwrap()
    );
    let facts_after = database_facts(&state_path).unwrap();
    assert_eq!(facts_after.counts, facts_before.counts);
    assert_eq!(facts_after.indexes, facts_before.indexes);
}

#[test]
fn failed_database_write_does_not_publish_memory_state_and_marks_handle_unhealthy() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let handle = test_handle(input, output.clone(), vec![profile(0, "Classic")]);

    let error = handle
        .update_store(|store| {
            store.next_id = u64::MAX;
            Ok(())
        })
        .unwrap_err();

    assert!(
        format!("{error:#}").contains("does not fit sqlite INTEGER"),
        "{error:#}"
    );
    assert_eq!(handle.store_snapshot().next_id, 1);
    assert!(handle.ensure_database_healthy().is_err());
    assert_eq!(
        load_store(&output.join(SQLITE_STATE_FILE))
            .unwrap()
            .unwrap()
            .next_id,
        1
    );
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
        enabled: true,
        status: ReviewRenderStatus::Queued,
        output_path: None,
        error: Some("old".to_string()),
        duration_ms: None,
        render_key: Some("retouch-key".to_string()),
        processing_key: Some(review_render_processing_key(0).to_string()),
        dcp_profile_filename: None,
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
    let output_root = Path::new("/out");
    let cache_root = Path::new("/tmp/mini-film.test");
    let base = PathBuf::from("/out/Classic/frame.jpg");
    let cache = retouch_cache_output(&base, "abc123", output_root, cache_root);

    assert_eq!(
        cache,
        PathBuf::from(
            "/tmp/mini-film.test/.mini-film-retouch/Classic/.frame.retouch-cache-abc123.jpg"
        )
    );
    assert_eq!(retouch_base_output(&cache, output_root, cache_root), base);
    assert_eq!(
        retouch_cache_output(&cache, "def456", output_root, cache_root),
        PathBuf::from(
            "/tmp/mini-film.test/.mini-film-retouch/Classic/.frame.retouch-cache-def456.jpg"
        )
    );
    assert_eq!(
        retouch_temp_output(&cache, "def456"),
        PathBuf::from("/tmp/mini-film.test/.mini-film-retouch/Classic/.frame.retouch-def456.jpg")
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
    let expected_key = profile_render_key_value(
        &saved_retouch,
        RetouchWhiteBalance::default(),
        BwFilter::None,
        Some(mini_film::DEFAULT_GRAIN_REFERENCE_MPIX),
        &test_processing_key(&raw, 0, Some(mini_film::DEFAULT_GRAIN_REFERENCE_MPIX)),
    );
    let cached = retouch_cache_output(
        &rendered,
        &expected_key,
        handle.output_root(),
        handle.cache_root(),
    );
    fs::create_dir_all(cached.parent().unwrap()).unwrap();
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
fn direct_compressed_retouch_uses_cache_without_replacing_source_link() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let original = input.join("frame.JPG");
    let base = output.join("frame.jpg");
    fs::write(&original, b"original").unwrap();
    crate::app::managed_symlink::ensure_file_symlink(&original, &base, true).unwrap();
    let handle = test_handle(input, output, Vec::new());
    handle.record_compressed_queued(&original, &base).unwrap();
    handle
        .record_compressed_done(&original, &base, Duration::from_millis(1))
        .unwrap();
    let retouch = RetouchSettings {
        crop: Some(crate::app::retouch::RetouchCrop {
            x: 0.1,
            y: 0.1,
            width: 0.8,
            height: 0.8,
        }),
        ..RetouchSettings::default()
    }
    .normalized();
    let render_key = retouch_render_key(&retouch, &handle.export).unwrap();
    let cached = retouch_cache_output(
        &base,
        &render_key,
        handle.output_root(),
        handle.cache_root(),
    );
    fs::create_dir_all(cached.parent().unwrap()).unwrap();
    fs::write(&cached, b"cached crop").unwrap();

    handle
        .apply_review_update(ReviewUpdateRequest {
            image_id: 1,
            rating: 0,
            label: ReviewLabel::None,
            labels: Vec::new(),
            tags: Vec::new(),
            notes: String::new(),
            retouch: Some(retouch),
            selected_profile_index: None,
            publish_profile_indexes: None,
            enabled_profile_indexes: None,
            profile_bw_filters: None,
            advance_after_update: false,
        })
        .unwrap();

    let store = handle.store_snapshot();
    assert_eq!(
        store.images[0].preview.path.as_deref(),
        Some(cached.as_path())
    );
    assert_eq!(store.images[0].preview.status, ReviewRenderStatus::Done);
    assert_eq!(store.images[0].preview.render_key, None);
    assert!(
        fs::symlink_metadata(&base)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(handle.retouch_scheduler.pending.load_full().is_empty());
}

#[test]
fn cached_retouch_output_is_current_for_its_base_profile() {
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
            ..BasicRetouchAdjustments::default()
        },
        ..RetouchSettings::default()
    }
    .normalized();
    let render_key = profile_render_key_value(
        &saved_retouch,
        RetouchWhiteBalance::default(),
        BwFilter::None,
        Some(mini_film::DEFAULT_GRAIN_REFERENCE_MPIX),
        &test_processing_key(&raw, 0, Some(mini_film::DEFAULT_GRAIN_REFERENCE_MPIX)),
    );
    let cached = retouch_cache_output(
        &rendered,
        &render_key,
        handle.output_root(),
        handle.cache_root(),
    );
    fs::create_dir_all(cached.parent().unwrap()).unwrap();
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

    assert!(handle.profile_render_current(&raw, 0, &rendered));
    fs::remove_file(&cached).unwrap();
    assert!(!handle.profile_render_current(&raw, 0, &rendered));
}

#[test]
fn profile_render_current_rejects_changed_export_options() {
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
    handle.record_profile_queued(&raw, 0, &rendered).unwrap();
    handle
        .record_profile_done(&raw, 0, &rendered, Duration::from_millis(42))
        .unwrap();
    assert!(handle.profile_render_current(&raw, 0, &rendered));

    let mut changed = handle.clone();
    changed.export.long_edge = Some(2048);
    changed.export.progressive_jpeg = true;
    assert!(!changed.profile_render_current(&raw, 0, &rendered));
}

#[test]
fn missing_profile_render_does_not_adopt_a_stale_base_output() {
    let temp = tempfile::tempdir().unwrap();
    let output_root = temp.path().join("out");
    let cache_root = temp.path().join("cache");
    let output = output_root.join("Classic").join("frame.jpg");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::write(&output, b"stale base render").unwrap();
    let mut render = profile_render(0, "Classic");
    render.status = ReviewRenderStatus::Missing;

    assert!(!apply_cached_profile_output(
        &mut render,
        &output,
        "normalized-grain-key",
        true,
        &output_root,
        &cache_root,
    ));
    assert_eq!(render.status, ReviewRenderStatus::Missing);

    render.status = ReviewRenderStatus::Done;
    assert!(apply_cached_profile_output(
        &mut render,
        &output,
        "normalized-grain-key",
        true,
        &output_root,
        &cache_root,
    ));
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
    let expected_key = profile_render_key_value(
        &saved_retouch,
        RetouchWhiteBalance::default(),
        BwFilter::None,
        Some(mini_film::DEFAULT_GRAIN_REFERENCE_MPIX),
        &test_processing_key(&raw, 0, Some(mini_film::DEFAULT_GRAIN_REFERENCE_MPIX)),
    );
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
    let job = handle
        .retouch_scheduler
        .next_job(|| handle.render_priority_snapshot());
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
        Some(mini_film::DEFAULT_GRAIN_REFERENCE_MPIX),
        &test_processing_key(&raw, 0, Some(mini_film::DEFAULT_GRAIN_REFERENCE_MPIX)),
    );
    let cached = retouch_cache_output(
        &rendered,
        &render_key,
        handle.output_root(),
        handle.cache_root(),
    );
    fs::create_dir_all(cached.parent().unwrap()).unwrap();
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
            enabled_profile_indexes: None,
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
fn review_update_enables_sampler_profile_and_aligns_publish_selection() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let raw = input.join("frame.NEF");
    fs::write(&raw, b"raw").unwrap();
    let mut sampler_profile = profile(SAMPLER_PROFILE_INDEX_BASE, "Sampler Film");
    sampler_profile.sampler_added = true;
    sampler_profile.enabled_by_default = false;
    sampler_profile.configured_from_cli = false;
    let handle = test_handle(input, output, vec![sampler_profile]);
    handle.record_discovered_raw(&raw).unwrap();
    assert!(!handle.store_snapshot().images[0].profiles[0].enabled);

    handle
        .apply_review_update(ReviewUpdateRequest {
            image_id: 1,
            rating: 0,
            label: ReviewLabel::None,
            labels: Vec::new(),
            tags: Vec::new(),
            notes: String::new(),
            retouch: None,
            selected_profile_index: Some(SAMPLER_PROFILE_INDEX_BASE),
            publish_profile_indexes: None,
            enabled_profile_indexes: Some(vec![SAMPLER_PROFILE_INDEX_BASE]),
            profile_bw_filters: None,
            advance_after_update: false,
        })
        .unwrap();

    let store = handle.store_snapshot();
    let image = &store.images[0];
    assert!(image.profiles[0].enabled);
    assert_eq!(image.selected_profile_index, SAMPLER_PROFILE_INDEX_BASE);
    assert_eq!(
        effective_publish_profile_indexes(image),
        vec![SAMPLER_PROFILE_INDEX_BASE]
    );
    assert_eq!(handle.retouch_scheduler.pending.load_full().len(), 1);
}

#[test]
fn disabling_queued_profile_marks_it_missing_and_unschedulable() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let raw = input.join("frame.NEF");
    fs::write(&raw, b"raw").unwrap();
    let handle = test_handle(
        input,
        output.clone(),
        vec![profile(0, "Classic"), profile(1, "Fade")],
    );
    handle.record_discovered_raw(&raw).unwrap();
    handle
        .record_profile_queued(&raw, 1, &output.join("Fade/frame.jpg"))
        .unwrap();

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
            enabled_profile_indexes: Some(vec![0]),
            profile_bw_filters: None,
            advance_after_update: false,
        })
        .unwrap();

    let store = handle.store_snapshot();
    let disabled = store.images[0]
        .profiles
        .iter()
        .find(|render| render.profile_index == 1)
        .unwrap();
    assert!(!disabled.enabled);
    assert_eq!(disabled.status, ReviewRenderStatus::Missing);
    assert_eq!(disabled.render_key, None);
    assert!(
        handle
            .render_priority_snapshot()
            .key_for(Some(1), Some(1), 0)
            .is_none()
    );
    assert!(
        !handle
            .record_profile_queued(&raw, 1, &output.join("Fade/frame.jpg"))
            .unwrap()
    );
    assert_eq!(
        handle.store_snapshot().images[0].profiles[1].status,
        ReviewRenderStatus::Missing
    );

    handle
        .update_store(|store| {
            let render = store.images[0]
                .profiles
                .iter_mut()
                .find(|render| render.profile_index == 1)
                .unwrap();
            render.enabled = true;
            render.status = ReviewRenderStatus::Processing;
            render.render_key = Some("active-retouch".to_string());
            store.set_profile_enabled_for_image(1, 1, false)?;
            Ok(())
        })
        .unwrap();
    let store = handle.store_snapshot();
    let disabled = store.images[0]
        .profiles
        .iter()
        .find(|render| render.profile_index == 1)
        .unwrap();
    assert!(!disabled.enabled);
    assert_eq!(disabled.status, ReviewRenderStatus::Missing);
    assert_eq!(disabled.render_key, None);
}

#[test]
fn retouch_scheduler_reorders_due_jobs_from_fresh_review_state() {
    let mut disabled = profile_render(2, "Disabled");
    disabled.enabled = false;
    let mut store = ReviewStore::new(Vec::new());
    store.images = vec![
        priority_image(
            1,
            "/in/first.NEF",
            100,
            5,
            1,
            vec![profile_render(0, "Classic"), profile_render(1, "Fade")],
        ),
        priority_image(
            2,
            "/in/second.NEF",
            200,
            5,
            0,
            vec![
                profile_render(0, "Classic"),
                profile_render(1, "Fade"),
                disabled,
            ],
        ),
    ];
    store.ui = ReviewUiState {
        current_image_id: Some(1),
        min_rating: 0,
    };
    let scheduler = ReviewRetouchScheduler::default();
    for (image_id, raw, profile_index) in [
        (2, "/in/second.NEF", Some(0)),
        (1, "/in/first.NEF", Some(0)),
        (1, "/in/first.NEF", Some(1)),
        (2, "/in/second.NEF", Some(2)),
    ] {
        scheduler.schedule_after(
            ReviewRetouchRequest {
                image_id,
                raw: PathBuf::from(raw),
                profile_index,
                output: PathBuf::from(format!("{raw}-{profile_index:?}.jpg")),
                render_key: format!("{raw}-{profile_index:?}"),
            },
            Duration::ZERO,
        );
    }

    let first = scheduler.next_job(|| store.render_priority_snapshot());
    assert_eq!((first.image_id, first.profile_index), (1, Some(1)));

    store.ui.current_image_id = Some(2);
    let second = scheduler.next_job(|| store.render_priority_snapshot());
    assert_eq!((second.image_id, second.profile_index), (2, Some(0)));
    let third = scheduler.next_job(|| store.render_priority_snapshot());
    assert_eq!((third.image_id, third.profile_index), (1, Some(0)));
    assert!(scheduler.pending.load_full().is_empty());
}

#[test]
fn retouch_scheduler_coalesces_stable_image_profile_to_latest_job() {
    let scheduler = ReviewRetouchScheduler::default();
    scheduler.schedule_after(
        ReviewRetouchRequest {
            image_id: 7,
            raw: PathBuf::from("frame.NEF"),
            profile_index: Some(1),
            output: PathBuf::from("old.jpg"),
            render_key: "old".to_string(),
        },
        Duration::ZERO,
    );
    scheduler.schedule_after(
        ReviewRetouchRequest {
            image_id: 7,
            raw: PathBuf::from("frame.dng"),
            profile_index: Some(1),
            output: PathBuf::from("new.jpg"),
            render_key: "new".to_string(),
        },
        Duration::ZERO,
    );

    assert_eq!(scheduler.pending.load_full().len(), 1);
    let priorities = ReviewStore::new(Vec::new()).render_priority_snapshot();
    let job = scheduler.next_job(|| priorities.clone());

    assert_eq!(job.image_id, 7);
    assert_eq!(job.raw, PathBuf::from("frame.dng"));
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
            enabled_profile_indexes: None,
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
            enabled_profile_indexes: None,
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
            enabled_profile_indexes: None,
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
            enabled_profile_indexes: None,
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
            enabled_profile_indexes: None,
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
    assert_eq!(
        state["publish_defaults"]["normalize_grain_mpix"],
        mini_film::DEFAULT_GRAIN_REFERENCE_MPIX
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
fn publish_args_rerender_when_grain_normalization_differs_from_daemon() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();

    let handle = test_handle(input, output, vec![profile(0, "Classic")]);
    let inherited = handle
        .publish_args_from_request(&PublishRequest::default())
        .unwrap();
    assert!(!inherited.rerender_raw);
    assert_eq!(
        inherited.normalize_grain_mpix,
        Some(mini_film::DEFAULT_GRAIN_REFERENCE_MPIX)
    );

    let matching = handle
        .publish_args_from_request(&PublishRequest {
            normalize_grain: Some(true),
            normalize_grain_mpix: Some(mini_film::DEFAULT_GRAIN_REFERENCE_MPIX),
            ..PublishRequest::default()
        })
        .unwrap();
    assert!(!matching.rerender_raw);

    let omitted_enablement = handle
        .publish_args_from_request(&PublishRequest {
            normalize_grain_mpix: Some(24.5),
            ..PublishRequest::default()
        })
        .unwrap();
    assert!(!omitted_enablement.rerender_raw);
    assert_eq!(
        omitted_enablement.normalize_grain_mpix,
        Some(mini_film::DEFAULT_GRAIN_REFERENCE_MPIX)
    );

    let custom = handle
        .publish_args_from_request(&PublishRequest {
            normalize_grain: Some(true),
            normalize_grain_mpix: Some(24.5),
            ..PublishRequest::default()
        })
        .unwrap();
    assert!(custom.rerender_raw);
    assert_eq!(custom.normalize_grain_mpix, Some(24.5));

    let disabled = handle
        .publish_args_from_request(&PublishRequest {
            normalize_grain: Some(false),
            normalize_grain_mpix: Some(24.5),
            ..PublishRequest::default()
        })
        .unwrap();
    assert!(disabled.rerender_raw);
    assert_eq!(disabled.normalize_grain_mpix, None);

    assert!(
        handle
            .publish_args_from_request(&PublishRequest {
                normalize_grain: Some(false),
                normalize_grain_mpix: Some(0.0),
                ..PublishRequest::default()
            })
            .is_ok()
    );
    assert!(
        handle
            .publish_args_from_request(&PublishRequest {
                normalize_grain: Some(true),
                normalize_grain_mpix: Some(0.0),
                ..PublishRequest::default()
            })
            .is_err()
    );
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
        review_route_path("/mini-film/sampler-media/1/source"),
        "/sampler-media/1/source"
    );
    assert_eq!(
        review_route_path("/mini-film/outputs/galleries/day/index.html"),
        "/outputs/galleries/day/index.html"
    );
    assert_eq!(review_route_path("/mini-film/review"), "/review");
    assert_eq!(review_route_path("/mini-film/tv"), "/tv");
    assert_eq!(review_route_path("/mini-film/"), "/");
}

#[test]
fn original_response_serves_typed_compressed_source_files_inline() {
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
    let runtime = test_async_runtime();

    let response = runtime.block_on(route_request(
        axum::http::Method::GET,
        "/original/1",
        axum::body::Bytes::new(),
        &handle,
    ));
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
    let body = runtime
        .block_on(axum::body::to_bytes(response.into_body(), usize::MAX))
        .unwrap();
    assert_eq!(&body[..], b"original jpeg bytes");

    let heic = input.join("frame-2.HEIC");
    fs::write(&heic, b"original heic bytes").unwrap();
    handle
        .record_compressed_queued(&heic, &output.join("frame-2.jpg"))
        .unwrap();
    let response = runtime.block_on(route_request(
        axum::http::Method::GET,
        "/original/2",
        axum::body::Bytes::new(),
        &handle,
    ));
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
    let response = runtime.block_on(route_request(
        axum::http::Method::GET,
        "/original/3",
        axum::body::Bytes::new(),
        &handle,
    ));
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
}

#[test]
fn sooc_media_response_uses_linked_source_content_type() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let heic = input.join("frame.HEIC");
    let managed = output.join(SOOC_PROFILE_STEM).join("frame.heic");
    fs::write(&heic, b"original heic bytes").unwrap();
    crate::app::managed_symlink::ensure_file_symlink(&heic, &managed, true).unwrap();

    let handle = test_handle(input, output, vec![profile(0, "Classic")]);
    handle.record_profiled_compressed_discovered(&heic).unwrap();
    handle
        .record_profile_queued(&heic, SOOC_PROFILE_INDEX, &managed)
        .unwrap();
    handle
        .record_profile_done(
            &heic,
            SOOC_PROFILE_INDEX,
            &managed,
            Duration::from_millis(1),
        )
        .unwrap();

    let response = test_async_runtime().block_on(route_request(
        axum::http::Method::GET,
        &format!("/media/1/{SOOC_PROFILE_INDEX}"),
        axum::body::Bytes::new(),
        &handle,
    ));
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap(),
        "image/heic"
    );
}

#[test]
fn published_gallery_download_route_archives_portable_assets() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    let gallery = output.join("finals");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(gallery.join("thumbnails")).unwrap();
    fs::create_dir_all(gallery.join(".mini-film-profile-inputs")).unwrap();
    fs::write(gallery.join("index.html"), b"<html>gallery</html>").unwrap();
    fs::write(gallery.join("gallery.js"), b"console.log('gallery')").unwrap();
    fs::write(gallery.join("gallery.css"), b"body { color: black; }").unwrap();
    fs::write(gallery.join("photo.jpg"), b"full jpeg").unwrap();
    fs::write(gallery.join("thumbnails/photo.jpg"), b"thumbnail jpeg").unwrap();
    fs::write(
        gallery.join(".mini-film-profile-inputs/private.jpg"),
        b"internal render input",
    )
    .unwrap();

    let handle = test_handle(input, output, vec![profile(0, "Classic")]);
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
        .record_publish_job_done(
            1,
            &PublishReport {
                linked: 1,
                skipped: 0,
                min_rating: 0,
                galleries: 1,
                gallery_roots: vec![gallery],
            },
        )
        .unwrap();
    let runtime = test_async_runtime();

    let response = runtime.block_on(route_request(
        axum::http::Method::GET,
        "/api/publish/1/gallery.zip",
        axum::body::Bytes::new(),
        &handle,
    ));
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap(),
        "application/zip"
    );
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::CONTENT_DISPOSITION)
            .unwrap(),
        "attachment; filename=\"finals.zip\""
    );
    let body = runtime
        .block_on(axum::body::to_bytes(response.into_body(), usize::MAX))
        .unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(body)).unwrap();
    let names = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_string())
        .collect::<HashSet<_>>();
    assert!(names.contains("finals/index.html"));
    assert!(names.contains("finals/gallery.js"));
    assert!(names.contains("finals/gallery.css"));
    assert!(names.contains("finals/photo.jpg"));
    assert!(names.contains("finals/thumbnails/photo.jpg"));
    assert!(!names.contains("finals/.mini-film-profile-inputs/private.jpg"));
    let mut photo = String::new();
    archive
        .by_name("finals/photo.jpg")
        .unwrap()
        .read_to_string(&mut photo)
        .unwrap();
    assert_eq!(photo, "full jpeg");
}

#[test]
fn compressed_review_media_routes_serve_distinct_cached_sizes_and_full_output() {
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

    let cache_root = handle
        .cache_root()
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
    let runtime = test_async_runtime();

    for (route, expected) in [
        ("/thumbnail/1", &b"thumbnail bytes"[..]),
        ("/preview/1", &b"preview bytes"[..]),
    ] {
        let response = runtime.block_on(route_request(
            axum::http::Method::GET,
            route,
            axum::body::Bytes::new(),
            &handle,
        ));
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = runtime
            .block_on(axum::body::to_bytes(response.into_body(), usize::MAX))
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
    let response = runtime.block_on(route_request(
        axum::http::Method::GET,
        "/media/1",
        axum::body::Bytes::new(),
        &handle,
    ));
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = runtime
        .block_on(axum::body::to_bytes(response.into_body(), usize::MAX))
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
    assert_eq!(
        image["profiles"][0]["file_size_bytes"],
        fs::metadata(&rendered).unwrap().len()
    );
    assert_eq!(image["profiles"][1]["profile_index"], SOOC_PROFILE_INDEX);
    assert_eq!(image["profiles"][1]["profile_stem"], SOOC_PROFILE_STEM);
    assert_eq!(
        image["profiles"][1]["file_size_bytes"],
        fs::metadata(&sooc).unwrap().len()
    );
    assert_eq!(
        image["profiles"][1]["display_name"],
        SOOC_PROFILE_DISPLAY_NAME
    );
    assert_eq!(image["publish_profile_indexes"], json!([0]));
}

#[test]
fn profile_pp3_route_includes_complete_per_image_adjustment_chain() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let jpg = input.join("frame.JPG");
    fs::write(&jpg, b"original jpeg bytes").unwrap();
    let pp3 = input.join("Classic.pp3");
    let source_curve = format!("Curve={}\n", "1;0;0;1;1;".repeat(80));
    fs::write(
        &pp3,
        format!("[Exposure]\nAuto=false\nCompensation=0.25\n{source_curve}"),
    )
    .unwrap();

    let mut classic = bw_profile(0, "Classic", -100.0, 0.0);
    classic.selector = pp3.display().to_string();
    let handle = test_handle(input.clone(), output, vec![classic]);
    handle.record_profiled_compressed_discovered(&jpg).unwrap();
    handle
        .update_store(|store| {
            let image = store.images.first_mut().unwrap();
            image.retouch = RetouchSettings {
                adjustments: BasicRetouchAdjustments {
                    highlights: -20.0,
                    shadows: 30.0,
                    whites: 10.0,
                    blacks: -5.0,
                    ..BasicRetouchAdjustments::default()
                },
                ..RetouchSettings::default()
            };
            image.profile_bw_filters = vec![ReviewProfileBwFilter {
                profile_index: 0,
                filter: BwFilter::Yellow,
            }];
            Ok(())
        })
        .unwrap();
    let runtime = test_async_runtime();

    let response = runtime.block_on(route_request(
        axum::http::Method::GET,
        "/api/profile/0/pp3/1",
        axum::body::Bytes::new(),
        &handle,
    ));

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap(),
        "text/plain; charset=utf-8"
    );
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::CONTENT_DISPOSITION)
            .unwrap(),
        "attachment"
    );
    let body = runtime
        .block_on(axum::body::to_bytes(response.into_body(), usize::MAX))
        .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("# Layer 1/4: Classic.pp3"));
    assert!(body.contains("# Layer 2/4: retouch.pp3"));
    assert!(body.contains("# Layer 3/4: bw-filter.pp3"));
    assert!(body.contains("# Layer 4/4: compressed-no-sharpening.pp3"));
    assert!(body.contains(&source_curve));
    assert!(body.contains(
        "[ToneEqualizer]\nEnabled=true\nBand0=-5\nBand1=30\nBand2=0\nBand3=-20\nBand4=10\n"
    ));
    assert!(body.contains("[Black & White]\nEnabled=true"));
    assert!(body.contains("Filter=Yellow"));
    assert_eq!(body.matches("Enabled=false").count(), 5);
    assert!(body.contains("[PostResizeSharpening]\nEnabled=false"));
    assert!(!body.contains("auto-matched-curve.pp3"));
    assert!(!body.contains("color-noise.pp3"));
    assert!(!body.contains("lens-corrections.pp3"));
}

#[test]
fn crop_source_and_profile_base_routes_serve_uncropped_media() {
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
    fs::create_dir_all(base.parent().unwrap()).unwrap();
    fs::write(&base, b"uncropped profile").unwrap();

    let handle = test_handle(input.clone(), output, vec![profile(0, "Classic")]);
    let cached = retouch_cache_output(&base, "crop", handle.output_root(), handle.cache_root());
    fs::create_dir_all(cached.parent().unwrap()).unwrap();
    fs::write(&cached, b"cropped profile").unwrap();

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
    let runtime = test_async_runtime();

    for (route, expected) in [
        ("/crop-source/1", &b"crop source"[..]),
        ("/media/1/0/base", &b"uncropped profile"[..]),
    ] {
        let response = runtime.block_on(route_request(
            axum::http::Method::GET,
            route,
            axum::body::Bytes::new(),
            &handle,
        ));
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = runtime
            .block_on(axum::body::to_bytes(response.into_body(), usize::MAX))
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
fn focus_overlay_toggles_the_svg_hidden_attribute() {
    let script = review_script();
    assert!(script.contains(r#"els.focusOverlay.setAttribute("hidden", "")"#));
    assert!(script.contains(r#"els.focusOverlay.removeAttribute("hidden")"#));
    assert!(!script.contains("els.focusOverlay.hidden ="));
}

#[test]
fn publish_accepts_managed_direct_compressed_link() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    let original = input.join("frame.JPG");
    let managed = output.join("frame.jpg");
    fs::write(&original, b"original jpeg").unwrap();
    crate::app::managed_symlink::ensure_file_symlink(&original, &managed, true).unwrap();

    let mut store = ReviewStore::new(Vec::new());
    let image = store.ensure_image(&input, &original).unwrap();
    image.rating = 5;
    image.preview.status = ReviewRenderStatus::Done;
    image.preview.path = Some(managed.clone());
    let options = test_publish_options("published");

    let report = publish_store_inner(&store, &input, &output, &options, None).unwrap();

    assert_eq!(report.linked, 1);
    assert_eq!(
        fs::read(output.join("published/frame.jpg")).unwrap(),
        b"original jpeg"
    );
    assert!(
        fs::symlink_metadata(&managed)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn publish_accepts_managed_sooc_link() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("in");
    let output = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(output.join(SOOC_PROFILE_STEM)).unwrap();
    let raw = input.join("frame.NEF");
    let sidecar = input.join("frame.JPG");
    let managed = output.join(SOOC_PROFILE_STEM).join("frame.jpg");
    fs::write(&raw, b"raw").unwrap();
    fs::write(&sidecar, b"original jpeg").unwrap();
    crate::app::managed_symlink::ensure_file_symlink(&sidecar, &managed, true).unwrap();

    let mut store = ReviewStore::new(vec![profile(0, "Classic")]);
    let image = store.ensure_image(&input, &raw).unwrap();
    image.sooc_sidecar_path = Some(sidecar);
    image.rating = 5;
    image.publish_profile_indexes = Some(vec![SOOC_PROFILE_INDEX]);
    let mut render = profile_render(SOOC_PROFILE_INDEX, SOOC_PROFILE_STEM);
    render.output_path = Some(managed.clone());
    image.profiles.push(render);
    let options = test_publish_options("published");

    let report = publish_store_inner(&store, &input, &output, &options, None).unwrap();

    assert_eq!(report.linked, 1);
    assert_eq!(
        fs::read(output.join("published/frame-sooc.jpg")).unwrap(),
        b"original jpeg"
    );
    assert!(
        fs::symlink_metadata(&managed)
            .unwrap()
            .file_type()
            .is_symlink()
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
            enabled: true,
            status: ReviewRenderStatus::Done,
            output_path: Some(source.clone()),
            error: None,
            duration_ms: Some(1),
            render_key: None,
            processing_key: Some(review_render_processing_key(0).to_string()),
            dcp_profile_filename: None,
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
                enabled: true,
                status: ReviewRenderStatus::Done,
                output_path: Some(classic.clone()),
                error: None,
                duration_ms: Some(1),
                render_key: None,
                processing_key: Some(review_render_processing_key(0).to_string()),
                dcp_profile_filename: None,
                width: None,
                height: None,
                updated_at: now_string(),
            },
            ReviewProfileRender {
                profile_index: 1,
                profile_stem: "Fade".to_string(),
                display_name: None,
                enabled: true,
                status: ReviewRenderStatus::Done,
                output_path: Some(fade.clone()),
                error: None,
                duration_ms: Some(1),
                render_key: None,
                processing_key: Some(review_render_processing_key(1).to_string()),
                dcp_profile_filename: None,
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
                enabled: true,
                status: ReviewRenderStatus::Done,
                output_path: Some(classic.clone()),
                error: None,
                duration_ms: Some(1),
                render_key: None,
                processing_key: Some(review_render_processing_key(0).to_string()),
                dcp_profile_filename: None,
                width: None,
                height: None,
                updated_at: now_string(),
            },
            ReviewProfileRender {
                profile_index: 1,
                profile_stem: "Fade".to_string(),
                display_name: None,
                enabled: true,
                status: ReviewRenderStatus::Done,
                output_path: Some(fade.clone()),
                error: None,
                duration_ms: Some(1),
                render_key: None,
                processing_key: Some(review_render_processing_key(1).to_string()),
                dcp_profile_filename: None,
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
