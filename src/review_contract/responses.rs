//! Public response bodies and metadata projections for the embedded review app.
//! Private paths and camera identifiers have no fields here and cannot leak through serialization.

use super::*;
use serde::Serialize;

/// The two source categories emitted by the current review projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewSourceType {
    Raw,
    Compressed,
}

/// Whether review renders configured profiles or directly displays compressed input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewProcessingMode {
    Profiled,
    Direct,
}

/// Effective diffusion controls and their inheritance source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ReviewEffectiveDiffusion {
    pub settings: DiffusionSettings,
    pub source: ReviewDiffusionSettingSource,
}

/// A profile render's public progress, metadata, and currently available media URLs.
#[derive(Clone, Debug, PartialEq, Serialize, schemars::JsonSchema)]
pub struct ReviewProfileRender {
    pub profile_index: usize,
    pub profile_stem: String,
    pub display_name: Option<String>,
    pub enabled: bool,
    pub status: ReviewRenderStatus,
    pub url: Option<String>,
    pub base_url: Option<String>,
    pub error: Option<String>,
    pub duration_ms: Option<u64>,
    pub file_size_bytes: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub retouch_pending: bool,
    pub dcp_profile_filename: Option<String>,
    pub lcp_profile_filename: Option<String>,
    pub bw_filter_eligible: bool,
    pub bw_filter: BwFilter,
    pub diffusion: ReviewEffectiveDiffusion,
    pub diffusion_settings: DiffusionSettings,
    pub diffusion_source: ReviewDiffusionSettingSource,
    pub updated_at: String,
}

/// User-visible analysis progress; internal analysis cache keys remain private.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ReviewImageCodex {
    pub status: ReviewCodexStatus,
    pub flags: CodexAnalysisFlags,
    pub model: String,
    pub error: Option<String>,
    pub updated_at: String,
}

/// One complete public image record, including legacy parallel label/profile fields.
#[derive(Clone, Debug, PartialEq, Serialize, schemars::JsonSchema)]
pub struct ReviewImage {
    pub id: u64,
    pub source_type: ReviewSourceType,
    pub processing_mode: ReviewProcessingMode,
    pub relative_path: String,
    pub file_name: String,
    pub source_file_size_bytes: Option<u64>,
    pub source_width: Option<u32>,
    pub source_height: Option<u32>,
    pub exif: GalleryExifData,
    pub preview_status: ReviewRenderStatus,
    pub thumbnail_url: Option<String>,
    pub preview_url: Option<String>,
    pub crop_source_url: Option<String>,
    pub crop_source_updated_at: String,
    pub full_url: Option<String>,
    pub preview_error: Option<String>,
    pub preview_duration_ms: Option<u64>,
    pub preview_retouch_pending: bool,
    pub preview_updated_at: String,
    pub selected_profile_index: usize,
    pub rating: u8,
    pub label: ReviewLabel,
    pub labels: Vec<ReviewLabel>,
    pub tags: Vec<String>,
    pub notes: String,
    pub rating_source: ReviewMetadataSource,
    pub tags_source: ReviewMetadataSource,
    pub notes_source: ReviewMetadataSource,
    pub codex: ReviewImageCodex,
    pub retouch: RetouchSettings,
    pub publish_profile_indexes: Vec<usize>,
    pub profile_bw_filters: Vec<ReviewProfileBwFilter>,
    pub profile_diffusion_settings: Vec<ReviewImageProfileDiffusionSetting>,
    pub profiles: Vec<ReviewProfileRender>,
    pub updated_at: String,
}

/// A panorama preview replaces its private cached path with a nullable public URL.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ReviewPanoramaPreview {
    pub matching_mode: PanoramaMatchingMode,
    pub projection: PanoramaProjection,
    pub status: ReviewPanoramaPreviewStatus,
    pub url: Option<String>,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
    pub updated_at: String,
}

/// Panorama project progress with a display filename instead of an output path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ReviewPanoramaProject {
    pub id: u64,
    pub name: String,
    pub status: ReviewPanoramaStatus,
    pub matching_mode: PanoramaMatchingMode,
    pub selected_projection: Option<PanoramaProjection>,
    pub output_file_name: Option<String>,
    pub result_image_id: Option<u64>,
    pub progress_stage: Option<String>,
    pub progress_completed: usize,
    pub progress_total: usize,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub image_ids: Vec<u64>,
    pub previews: Vec<ReviewPanoramaPreview>,
}

/// Whether the required panorama tools are available on the daemon host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct PanoramaCapability {
    pub available: bool,
    pub reason: Option<String>,
}

/// Feature availability advertised to the current client.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ReviewCapabilities {
    pub panorama: PanoramaCapability,
    pub sampler: bool,
    pub diffusion: bool,
}

/// Analysis configuration and queue counts for the review session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ReviewCodexSummary {
    pub enabled: bool,
    pub flags: Option<CodexAnalysisFlags>,
    pub model: Option<String>,
    pub queued: u64,
    pub processing: u64,
    pub done: u64,
    pub failed: u64,
}

/// Current panorama projects and the daemon-wide operation lock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ReviewPanoramaState {
    pub busy: bool,
    pub projects: Vec<ReviewPanoramaProject>,
}

/// Persisted review navigation shared between clients.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ReviewUiState {
    pub current_image_id: Option<u64>,
    pub min_rating: u8,
    pub labels: Vec<ReviewLabel>,
}

/// Complete untagged snapshot, retaining every existing top-level JSON field.
#[derive(Clone, Debug, PartialEq, Serialize, schemars::JsonSchema)]
pub struct ReviewStateSnapshot {
    pub version: String,
    pub invocation: Option<String>,
    pub profiles: Vec<ReviewProfile>,
    pub client_count: usize,
    pub codex: ReviewCodexSummary,
    pub publish_defaults: ReviewPublishDefaults,
    pub diffusion_default: DiffusionSettings,
    pub profile_diffusion_settings: Vec<ReviewProfileDiffusionSetting>,
    pub publish_jobs: Vec<ReviewPublishJob>,
    pub capabilities: ReviewCapabilities,
    pub panorama: ReviewPanoramaState,
    pub ui: ReviewUiState,
    pub bursts: Vec<ReviewBurst>,
    pub images: Vec<ReviewImage>,
    pub publish_root: String,
}

/// Ordinary JSON error responses; some routing errors intentionally remain plain text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ReviewError {
    pub error: String,
}

/// Existing SSE keepalive tag, distinct from ordinary state messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewKeepaliveType {
    Keepalive,
}

/// Payload of the named keepalive SSE event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ReviewKeepalive {
    #[serde(rename = "type")]
    pub kind: ReviewKeepaliveType,
    pub datetime: String,
    pub version: String,
}

/// Serialized ReviewProfile with only fields visible to review clients.
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, PartialEq)]
pub struct ReviewProfile {
    pub index: usize,
    pub identity: String,
    pub selector: String,
    pub stem: String,
    pub sampler_added: bool,
    pub enabled_by_default: bool,
    pub configured_from_cli: bool,
    pub retouch_base: BasicRetouchAdjustments,
    pub metadata: Option<ReviewProfileMetadata>,
}

/// Serialized ReviewProfileMetadata with only fields visible to review clients.
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, PartialEq)]
pub struct ReviewProfileMetadata {
    pub profile_name: String,
    pub profile_uuid: Option<String>,
    pub look_name: Option<String>,
    pub look_uuid: Option<String>,
    pub source_profile_name: Option<String>,
    pub source_profile_uuid: Option<String>,
    pub source_adjustments: ReviewProfileAdjustments,
    pub source_sharpening: ReviewProfileSharpening,
    pub emulation_adjustments: ReviewProfileAdjustments,
    pub emulation_sharpening: ReviewProfileSharpening,
    pub has_camera_raw_settings: bool,
    pub grain: Option<ReviewProfileGrain>,
    pub has_hald: bool,
    pub has_pp3: bool,
    pub pp3_name: Option<String>,
    pub pp3_adjustments: Vec<ReviewProfilePp3Section>,
}

/// Serialized ReviewProfilePp3Section with only fields visible to review clients.
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ReviewProfilePp3Section {
    pub source: String,
    pub section: String,
    pub entries: Vec<ReviewProfilePp3Entry>,
}

/// Serialized ReviewProfilePp3Entry with only fields visible to review clients.
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ReviewProfilePp3Entry {
    pub key: String,
    pub value: String,
}

/// Serialized ReviewProfileGrain with only fields visible to review clients.
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, PartialEq)]
pub struct ReviewProfileGrain {
    pub amount: u8,
    pub size: u8,
    pub frequency: u8,
}

/// Serialized ReviewProfileAdjustments with only fields visible to review clients.
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, PartialEq)]
pub struct ReviewProfileAdjustments {
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub saturation: f32,
    pub vibrance: f32,
    pub clarity: f32,
    pub parametric: ReviewProfileParametricTone,
    pub hsl: ReviewProfileHslAdjustments,
    pub calibration: ReviewProfileCalibration,
    pub tone_curve: ReviewProfileToneCurves,
}

/// Serialized ReviewProfileSharpening with only fields visible to review clients.
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, PartialEq)]
pub struct ReviewProfileSharpening {
    pub present: bool,
    pub amount: f32,
    pub radius: f32,
    pub detail: f32,
    pub masking: f32,
}

/// Serialized ReviewProfileParametricTone with only fields visible to review clients.
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, PartialEq)]
pub struct ReviewProfileParametricTone {
    pub shadows: f32,
    pub darks: f32,
    pub lights: f32,
    pub highlights: f32,
    pub shadow_split: f32,
    pub midtone_split: f32,
    pub highlight_split: f32,
}

/// Serialized ReviewProfileHslAdjustments with only fields visible to review clients.
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, PartialEq)]
pub struct ReviewProfileHslAdjustments {
    pub hue: Vec<f32>,
    pub saturation: Vec<f32>,
    pub luminance: Vec<f32>,
}

/// Serialized ReviewProfileCalibration with only fields visible to review clients.
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, PartialEq)]
pub struct ReviewProfileCalibration {
    pub red_hue: f32,
    pub red_saturation: f32,
    pub green_hue: f32,
    pub green_saturation: f32,
    pub blue_hue: f32,
    pub blue_saturation: f32,
}

/// Serialized ReviewProfileToneCurves with only fields visible to review clients.
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, PartialEq)]
pub struct ReviewProfileToneCurves {
    pub composite: Vec<[f32; 2]>,
    pub red: Vec<[f32; 2]>,
    pub green: Vec<[f32; 2]>,
    pub blue: Vec<[f32; 2]>,
}

/// Serialized ReviewBurst with only fields visible to review clients.
#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReviewBurst {
    pub id: String,
    pub image_ids: Vec<u64>,
    pub expanded: bool,
}

/// Serialized ReviewDiffusionDetailArea with only fields visible to review clients.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct ReviewDiffusionDetailArea {
    pub kind: ReviewDiffusionDetailAreaKind,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Serialized ReviewDiffusionJob with only fields visible to review clients.
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ReviewDiffusionJob {
    pub id: u64,
    pub status: ReviewDiffusionJobStatus,
    pub image_id: u64,
    pub profile_index: usize,
    pub settings: DiffusionSettings,
    pub before_url: Option<String>,
    pub after_url: Option<String>,
    pub preview_width: Option<u32>,
    pub preview_height: Option<u32>,
    pub focus_source: Option<ReviewDiffusionFocusSource>,
    pub detail_areas: Vec<ReviewDiffusionDetailArea>,
    pub error: Option<String>,
    // Legacy client aliases are accepted by validators but omitted by current producers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_updated_at: Option<String>,
}

/// Serialized ReviewPublishDefaults with only fields visible to review clients.
#[derive(schemars::JsonSchema, PartialEq, Clone, Debug, Serialize)]
pub struct ReviewPublishDefaults {
    pub album: String,
    pub output_format: String,
    pub jpg_quality: u8,
    pub resize: Option<String>,
    pub long_edge: Option<u32>,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    pub jpeg_subsampling: String,
    pub strip_metadata: bool,
    pub progressive_jpeg: bool,
    pub gallery: Option<String>,
    pub gallery_thumbnail_long_edge: u32,
    pub gallery_columns: u32,
    pub grain_engine: String,
    pub normalize_grain_mpix: Option<f64>,
}

/// Serialized ReviewPublishJob with only fields visible to review clients.
#[derive(schemars::JsonSchema, PartialEq, Clone, Debug, Serialize)]
pub struct ReviewPublishJob {
    pub id: u64,
    pub album: String,
    pub status: ReviewPublishJobStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub processed: u64,
    pub total: u64,
    pub step: String,
    pub current: Option<String>,
    pub linked: u64,
    pub skipped: u64,
    pub galleries: u64,
    pub gallery_urls: Vec<String>,
    pub error: Option<String>,
}

/// Serialized ReviewSamplerJobSnapshot with only fields visible to review clients.
#[derive(schemars::JsonSchema, PartialEq, Clone, Debug, Serialize)]
pub struct ReviewSamplerJobSnapshot {
    pub id: u64,
    pub image_id: u64,
    pub file_name: String,
    pub status: ReviewSamplerJobStatus,
    pub source_url: Option<String>,
    pub source_width: Option<u32>,
    pub source_height: Option<u32>,
    pub completed: usize,
    pub total: usize,
    pub failed: usize,
    pub workers: usize,
    pub error: Option<String>,
    pub entries: Vec<ReviewSamplerEntrySnapshot>,
}

/// Serialized ReviewSamplerEntrySnapshot with only fields visible to review clients.
#[derive(schemars::JsonSchema, PartialEq, Clone, Debug, Serialize)]
pub struct ReviewSamplerEntrySnapshot {
    pub key: String,
    pub name: String,
    pub filename: String,
    pub parts: Vec<String>,
    pub status: ReviewSamplerEntryStatus,
    pub thumbnail_url: Option<String>,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
    pub current_enabled: bool,
    pub all_enabled: bool,
    pub configured_from_cli: bool,
    pub selected: bool,
}

/// Serialized GalleryFocusRegion with only fields visible to review clients.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, PartialEq)]
pub struct GalleryFocusRegion {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub primary: bool,
}

/// Serialized GalleryExifData with only fields visible to review clients.
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, PartialEq)]
pub struct GalleryExifData {
    pub capture_timestamp: Option<i64>,
    pub capture_subsecond: Option<String>,
    pub rating: Option<u8>,
    pub file_size_bytes: Option<u64>,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub focus_frame_width: Option<u32>,
    pub focus_frame_height: Option<u32>,
    pub focus_regions: Vec<GalleryFocusRegion>,
    pub focal_length: Option<String>,
    pub aperture: Option<String>,
    pub shutter_speed: Option<String>,
    pub iso: Option<String>,
    pub auto_iso: Option<bool>,
    pub iso_auto_hi_limit: Option<String>,
    pub white_balance_mode: Option<String>,
    pub white_balance_temperature: Option<u32>,
    pub white_balance_offset: Option<i32>,
    pub camera_model: Option<String>,
    pub shutter_count: Option<u64>,
    pub shutter_mode: Option<String>,
    pub silent_photography: Option<bool>,
    pub release_mode: Option<String>,
    pub lens_model: Option<String>,
    pub shooting_mode: Option<String>,
    pub exposure_compensation: Option<String>,
    pub flash: Option<String>,
    pub active_d_lighting: Option<String>,
    pub tags: Vec<String>,
    pub note: Option<String>,
}
