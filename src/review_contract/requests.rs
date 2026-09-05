//! Accepted request bodies for every JSON-mutating review endpoint.
//! Serde defaults and permissive unknown fields preserve existing client compatibility.

use super::*;
use serde::Deserialize;

/// Accepted JSON body for ReviewUpdateRequest.
#[derive(schemars::JsonSchema, Clone, Debug, Deserialize)]
pub struct ReviewUpdateRequest {
    pub image_id: u64,
    pub rating: u8,
    #[serde(default)]
    pub label: ReviewLabel,
    #[serde(default)]
    pub labels: Vec<ReviewLabel>,
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub retouch: Option<RetouchSettings>,
    #[serde(default)]
    pub selected_profile_index: Option<usize>,
    #[serde(default)]
    pub publish_profile_indexes: Option<Vec<usize>>,
    #[serde(default)]
    pub enabled_profile_indexes: Option<Vec<usize>>,
    #[serde(default)]
    pub profile_bw_filters: Option<Vec<ReviewProfileBwFilter>>,
    #[serde(default)]
    pub advance_after_update: bool,
}

/// Accepted JSON body for ReviewUiUpdateRequest.
#[derive(schemars::JsonSchema, Clone, Debug, Deserialize)]
pub struct ReviewUiUpdateRequest {
    #[serde(default)]
    pub current_image_id: Option<u64>,
    #[serde(default)]
    pub min_rating: u8,
    #[serde(default)]
    pub labels: Vec<ReviewLabel>,
}

/// Accepted JSON body for ReviewBurstExpansionRequest.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Deserialize)]
pub struct ReviewBurstExpansionRequest {
    pub expanded: bool,
}

/// Accepted JSON body for PublishRequest.
#[derive(schemars::JsonSchema, Debug, Default, Deserialize)]
pub struct PublishRequest {
    #[serde(default)]
    pub min_rating: u8,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub labels: Vec<ReviewLabel>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub main_profile_only: bool,
    #[serde(default)]
    pub output_format: Option<String>,
    #[serde(default)]
    pub gallery: Option<String>,
    #[serde(default)]
    pub jpg_quality: Option<u8>,
    #[serde(default)]
    pub size_mode: Option<String>,
    #[serde(default)]
    pub resize: Option<String>,
    #[serde(default)]
    pub long_edge: Option<u32>,
    #[serde(default)]
    pub max_width: Option<u32>,
    #[serde(default)]
    pub max_height: Option<u32>,
    #[serde(default)]
    pub jpeg_subsampling: Option<String>,
    #[serde(default)]
    pub strip_metadata: Option<bool>,
    #[serde(default)]
    pub progressive_jpeg: Option<bool>,
    #[serde(default)]
    pub gallery_thumbnail_long_edge: Option<u32>,
    #[serde(default)]
    pub gallery_columns: Option<u32>,
    #[serde(default)]
    pub grain_engine: Option<String>,
    #[serde(default)]
    pub normalize_grain: Option<bool>,
    #[serde(default)]
    pub normalize_grain_mpix: Option<f64>,
}

/// Accepted JSON body for ReviewDiffusionJobRequest.
#[derive(schemars::JsonSchema, Clone, Debug, Deserialize)]
pub struct ReviewDiffusionJobRequest {
    pub image_id: u64,
    pub profile_index: usize,
    pub settings: DiffusionSettings,
}

/// Accepted JSON body for ReviewDiffusionSettingsRequest.
#[derive(schemars::JsonSchema, Clone, Debug, Deserialize)]
pub struct ReviewDiffusionSettingsRequest {
    pub scope: ReviewDiffusionScope,
    pub image_id: u64,
    pub profile_index: usize,
    pub settings: DiffusionSettings,
}

/// Accepted JSON body for ReviewDiffusionSettingsResetRequest.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Deserialize)]
pub struct ReviewDiffusionSettingsResetRequest {
    pub scope: ReviewDiffusionScope,
    pub image_id: u64,
    pub profile_index: usize,
}

/// Accepted JSON body for ReviewPanoramaCreateRequest.
#[derive(schemars::JsonSchema, Clone, Debug, Deserialize)]
pub struct ReviewPanoramaCreateRequest {
    pub image_ids: Vec<u64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub matching_mode: PanoramaMatchingMode,
}

/// Accepted JSON body for ReviewPanoramaUpdateRequest.
#[derive(schemars::JsonSchema, Clone, Debug, Default, Deserialize)]
pub struct ReviewPanoramaUpdateRequest {
    #[serde(default)]
    pub image_ids: Option<Vec<u64>>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub matching_mode: Option<PanoramaMatchingMode>,
    #[serde(default)]
    pub selected_projection: Option<PanoramaProjection>,
}

/// Accepted JSON body for ReviewPanoramaPreviewRequest.
#[derive(schemars::JsonSchema, Clone, Debug, Default, Deserialize)]
pub struct ReviewPanoramaPreviewRequest {
    #[serde(default)]
    pub image_ids: Option<Vec<u64>>,
    #[serde(default)]
    pub matching_mode: Option<PanoramaMatchingMode>,
}

/// Accepted JSON body for ReviewPanoramaRenderRequest.
#[derive(schemars::JsonSchema, Clone, Debug, Default, Deserialize)]
pub struct ReviewPanoramaRenderRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub projection: Option<PanoramaProjection>,
}

/// Accepted JSON body for ReviewSamplerStartRequest.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Deserialize)]
pub struct ReviewSamplerStartRequest {
    pub image_id: u64,
}

/// Accepted JSON body for ReviewSamplerPriorityRequest.
#[derive(schemars::JsonSchema, Clone, Debug, Default, Deserialize)]
pub struct ReviewSamplerPriorityRequest {
    #[serde(default)]
    pub visible_keys: Vec<String>,
    #[serde(default)]
    pub expanded_keys: Vec<String>,
}

/// Accepted JSON body for ReviewSamplerSelectionRequest.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Deserialize)]
pub struct ReviewSamplerSelectionRequest {
    pub scope: ReviewSamplerScope,
    pub enabled: bool,
}
