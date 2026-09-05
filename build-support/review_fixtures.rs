//! Deterministic public DTO examples for cross-language validator checks.
//! Construct values in Rust rather than maintaining another hand-written TypeScript contract.

use crate::review_contract::*;
use serde::Serialize;

const TIME: &str = "2026-09-05T12:00:00+00:00";

/// Each field matches a response catalog entry and is serialized by the production DTO implementation.
#[derive(Serialize)]
pub struct ResponseFixtures {
    pub state: ReviewStateSnapshot,
    pub patch: ReviewStatePatch,
    pub sampler_job: ReviewSamplerJobSnapshot,
    pub diffusion_job: ReviewDiffusionJob,
    pub keepalive: ReviewKeepalive,
    pub error: ReviewError,
}

/// Include fixed-length tone-curve tuples and optional profile metadata in validator coverage.
fn adjustments() -> ReviewProfileAdjustments {
    ReviewProfileAdjustments {
        exposure: 0.25,
        contrast: 10.0,
        highlights: -10.0,
        shadows: 5.0,
        whites: 0.0,
        blacks: 0.0,
        saturation: 0.0,
        vibrance: 0.0,
        clarity: 0.0,
        parametric: ReviewProfileParametricTone {
            shadows: 0.0,
            darks: 0.0,
            lights: 0.0,
            highlights: 0.0,
            shadow_split: 25.0,
            midtone_split: 50.0,
            highlight_split: 75.0,
        },
        hsl: ReviewProfileHslAdjustments {
            hue: vec![0.0; 8],
            saturation: vec![0.0; 8],
            luminance: vec![0.0; 8],
        },
        calibration: ReviewProfileCalibration {
            red_hue: 0.0,
            red_saturation: 0.0,
            green_hue: 0.0,
            green_saturation: 0.0,
            blue_hue: 0.0,
            blue_saturation: 0.0,
        },
        tone_curve: ReviewProfileToneCurves {
            composite: vec![[0.0, 0.0], [1.0, 1.0]],
            red: vec![],
            green: vec![],
            blue: vec![],
        },
    }
}

/// Profile data includes all nested metadata shapes without real user filenames or identifiers.
fn profile() -> ReviewProfile {
    let sharpening = ReviewProfileSharpening {
        present: true,
        amount: 25.0,
        radius: 1.0,
        detail: 25.0,
        masking: 0.0,
    };
    ReviewProfile {
        index: 0,
        identity: "fixture:neutral".to_owned(),
        selector: "neutral.xmp".to_owned(),
        stem: "neutral".to_owned(),
        sampler_added: false,
        enabled_by_default: true,
        configured_from_cli: true,
        retouch_base: BasicRetouchAdjustments::default(),
        metadata: Some(ReviewProfileMetadata {
            profile_name: "Neutral".to_owned(),
            profile_uuid: None,
            look_name: None,
            look_uuid: None,
            source_profile_name: Some("Camera Neutral".to_owned()),
            source_profile_uuid: None,
            source_adjustments: adjustments(),
            source_sharpening: sharpening.clone(),
            emulation_adjustments: adjustments(),
            emulation_sharpening: sharpening,
            has_camera_raw_settings: true,
            grain: Some(ReviewProfileGrain {
                amount: 10,
                size: 20,
                frequency: 30,
            }),
            has_hald: true,
            has_pp3: true,
            pp3_name: Some("neutral.pp3".to_owned()),
            pp3_adjustments: vec![ReviewProfilePp3Section {
                source: "profile".to_owned(),
                section: "Exposure".to_owned(),
                entries: vec![ReviewProfilePp3Entry {
                    key: "Compensation".to_owned(),
                    value: "0.25".to_owned(),
                }],
            }],
        }),
    }
}

/// Public EXIF deliberately has no slots for serial numbers, burst internals, or source paths.
fn exif() -> GalleryExifData {
    GalleryExifData {
        capture_timestamp: Some(1_788_609_600),
        capture_subsecond: Some("125".to_owned()),
        rating: Some(3),
        file_size_bytes: Some(12_345),
        image_width: Some(6000),
        image_height: Some(4000),
        focus_frame_width: Some(6000),
        focus_frame_height: Some(4000),
        focus_regions: vec![GalleryFocusRegion {
            x: 0.4,
            y: 0.4,
            width: 0.2,
            height: 0.2,
            primary: true,
        }],
        focal_length: Some("50 mm".to_owned()),
        aperture: Some("f/2.8".to_owned()),
        shutter_speed: Some("1/250".to_owned()),
        iso: Some("100".to_owned()),
        auto_iso: Some(false),
        iso_auto_hi_limit: None,
        white_balance_mode: Some("Daylight".to_owned()),
        white_balance_temperature: Some(5500),
        white_balance_offset: Some(0),
        camera_model: Some("Fixture camera".to_owned()),
        shutter_count: Some(10),
        shutter_mode: None,
        silent_photography: Some(false),
        release_mode: None,
        lens_model: Some("Fixture 50 mm".to_owned()),
        shooting_mode: Some("Manual".to_owned()),
        exposure_compensation: Some("0".to_owned()),
        flash: None,
        active_d_lighting: None,
        tags: vec!["fixture".to_owned()],
        note: None,
    }
}

/// One completed image covers media nullability, provenance, diffusion aliases, and crop values.
fn image() -> ReviewImage {
    let settings = DiffusionSettings::default();
    ReviewImage {
        id: 1,
        source_type: ReviewSourceType::Raw,
        processing_mode: ReviewProcessingMode::Profiled,
        relative_path: "fixture.nef".to_owned(),
        file_name: "fixture.nef".to_owned(),
        source_file_size_bytes: Some(12_345),
        source_width: Some(6000),
        source_height: Some(4000),
        exif: exif(),
        preview_status: ReviewRenderStatus::Done,
        thumbnail_url: None,
        preview_url: Some("preview/1".to_owned()),
        crop_source_url: Some("preview/1".to_owned()),
        crop_source_updated_at: TIME.to_owned(),
        full_url: None,
        preview_error: None,
        preview_duration_ms: Some(100),
        preview_retouch_pending: false,
        preview_updated_at: TIME.to_owned(),
        selected_profile_index: 0,
        rating: 3,
        label: ReviewLabel::Red,
        labels: vec![ReviewLabel::Red],
        tags: vec!["fixture".to_owned()],
        notes: "Fixture note".to_owned(),
        rating_source: ReviewMetadataSource::Camera,
        tags_source: ReviewMetadataSource::Manual,
        notes_source: ReviewMetadataSource::Manual,
        codex: ReviewImageCodex {
            status: ReviewCodexStatus::Skipped,
            flags: CodexAnalysisFlags::default(),
            model: String::new(),
            error: None,
            updated_at: TIME.to_owned(),
        },
        retouch: RetouchSettings {
            adjustments: BasicRetouchAdjustments::default(),
            crop: Some(RetouchCrop {
                x: 0.1,
                y: 0.1,
                width: 0.8,
                height: 0.8,
            }),
            rotation_degrees: 1.5,
        },
        publish_profile_indexes: vec![0],
        profile_bw_filters: vec![ReviewProfileBwFilter {
            profile_index: 0,
            filter: BwFilter::None,
        }],
        profile_diffusion_settings: vec![],
        profiles: vec![ReviewProfileRender {
            profile_index: 0,
            profile_stem: "neutral".to_owned(),
            display_name: None,
            enabled: true,
            status: ReviewRenderStatus::Done,
            url: Some("media/1/0".to_owned()),
            base_url: Some("media/1/0/base".to_owned()),
            error: None,
            duration_ms: Some(100),
            file_size_bytes: Some(1000),
            width: Some(3000),
            height: Some(2000),
            retouch_pending: false,
            dcp_profile_filename: Some("camera.dcp".to_owned()),
            lcp_profile_filename: None,
            bw_filter_eligible: false,
            bw_filter: BwFilter::None,
            diffusion: ReviewEffectiveDiffusion {
                settings,
                source: ReviewDiffusionSettingSource::Daemon,
            },
            diffusion_settings: settings,
            diffusion_source: ReviewDiffusionSettingSource::Daemon,
            updated_at: TIME.to_owned(),
        }],
        updated_at: TIME.to_owned(),
    }
}

/// Build a stable snapshot covering complete output records while omitting any machine-specific paths.
fn snapshot() -> ReviewStateSnapshot {
    ReviewStateSnapshot {
        version: "fixture".to_owned(),
        invocation: Some("mini-film review fixture".to_owned()),
        profiles: vec![profile()],
        client_count: 1,
        codex: ReviewCodexSummary {
            enabled: false,
            flags: None,
            model: None,
            queued: 0,
            processing: 0,
            done: 0,
            failed: 0,
        },
        publish_defaults: ReviewPublishDefaults {
            album: "published".to_owned(),
            output_format: "jpg".to_owned(),
            jpg_quality: 90,
            resize: None,
            long_edge: None,
            max_width: None,
            max_height: None,
            jpeg_subsampling: "444".to_owned(),
            strip_metadata: false,
            progressive_jpeg: false,
            gallery: Some("modern".to_owned()),
            gallery_thumbnail_long_edge: 1024,
            gallery_columns: 4,
            grain_engine: "fft".to_owned(),
            normalize_grain_mpix: Some(12.0),
        },
        diffusion_default: DiffusionSettings::default(),
        profile_diffusion_settings: vec![],
        publish_jobs: vec![ReviewPublishJob {
            id: 1,
            album: "published".to_owned(),
            status: ReviewPublishJobStatus::Done,
            started_at: TIME.to_owned(),
            finished_at: Some(TIME.to_owned()),
            processed: 1,
            total: 1,
            step: "done".to_owned(),
            current: None,
            linked: 1,
            skipped: 0,
            galleries: 1,
            gallery_urls: vec!["outputs/published/index.html".to_owned()],
            error: None,
        }],
        capabilities: ReviewCapabilities {
            panorama: PanoramaCapability {
                available: true,
                reason: None,
            },
            sampler: true,
            diffusion: true,
        },
        panorama: ReviewPanoramaState {
            busy: false,
            projects: vec![ReviewPanoramaProject {
                id: 1,
                name: "Fixture panorama".to_owned(),
                status: ReviewPanoramaStatus::Ready,
                matching_mode: PanoramaMatchingMode::Automatic,
                selected_projection: Some(PanoramaProjection::Cylindrical),
                output_file_name: None,
                result_image_id: None,
                progress_stage: Some("previews".to_owned()),
                progress_completed: 1,
                progress_total: 1,
                error: None,
                created_at: TIME.to_owned(),
                updated_at: TIME.to_owned(),
                image_ids: vec![1],
                previews: vec![ReviewPanoramaPreview {
                    matching_mode: PanoramaMatchingMode::Automatic,
                    projection: PanoramaProjection::Cylindrical,
                    status: ReviewPanoramaPreviewStatus::Done,
                    url: Some("panorama-preview/1/automatic/cylindrical".to_owned()),
                    duration_ms: Some(100),
                    error: None,
                    updated_at: TIME.to_owned(),
                }],
            }],
        },
        ui: ReviewUiState {
            current_image_id: Some(1),
            min_rating: 0,
            labels: vec![ReviewLabel::Red],
        },
        bursts: vec![],
        images: vec![image()],
        publish_root: "published".to_owned(),
    }
}

/// Create all fixtures through the shared typed serializers, including a real computed state patch.
pub fn responses() -> ResponseFixtures {
    let state = snapshot();
    let mut next = state.clone();
    next.invocation = None;
    next.diffusion_default.softness = 31;
    next.profile_diffusion_settings
        .push(ReviewProfileDiffusionSetting {
            profile_index: 0,
            settings: next.diffusion_default,
        });
    next.images[0].notes = "Updated fixture note".to_owned();
    let patch = ReviewStatePatch::between(&state, &next);
    ResponseFixtures {
        state,
        patch,
        sampler_job: ReviewSamplerJobSnapshot {
            id: 1,
            image_id: 1,
            file_name: "fixture.nef".to_owned(),
            status: ReviewSamplerJobStatus::Done,
            source_url: Some("sampler-media/1/source".to_owned()),
            source_width: Some(512),
            source_height: Some(341),
            completed: 1,
            total: 1,
            failed: 0,
            workers: 1,
            error: None,
            entries: vec![ReviewSamplerEntrySnapshot {
                key: "neutral".to_owned(),
                name: "Neutral".to_owned(),
                filename: "neutral.xmp".to_owned(),
                parts: vec!["Neutral".to_owned()],
                status: ReviewSamplerEntryStatus::Done,
                thumbnail_url: Some("sampler-media/1/neutral".to_owned()),
                duration_ms: Some(50),
                error: None,
                current_enabled: true,
                all_enabled: false,
                configured_from_cli: true,
                selected: true,
            }],
        },
        diffusion_job: ReviewDiffusionJob {
            id: 1,
            status: ReviewDiffusionJobStatus::Done,
            image_id: 1,
            profile_index: 0,
            settings: DiffusionSettings::default(),
            before_url: Some("diffusion-preview/1/before".to_owned()),
            after_url: Some("diffusion-preview/1/after".to_owned()),
            preview_width: Some(2048),
            preview_height: Some(1365),
            focus_source: Some(ReviewDiffusionFocusSource::CameraFocus),
            detail_areas: vec![ReviewDiffusionDetailArea {
                kind: ReviewDiffusionDetailAreaKind::Focus,
                x: 100,
                y: 100,
                width: 200,
                height: 200,
            }],
            error: None,
            source_url: None,
            source_width: None,
            source_height: None,
            preview_url: None,
            result_url: None,
            updated_at: None,
            before_updated_at: None,
            after_updated_at: None,
        },
        keepalive: ReviewKeepalive {
            kind: ReviewKeepaliveType::Keepalive,
            datetime: TIME.to_owned(),
            version: "fixture".to_owned(),
        },
        error: ReviewError {
            error: "Fixture error".to_owned(),
        },
    }
}
