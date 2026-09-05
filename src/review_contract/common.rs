//! Primitive wire values shared by request and response schemas.
//! Keep rendering and CLI behavior in application adapters, not in this build-time module.

use serde::{Deserialize, Serialize};

/// JSON representation of BwFilter; serialized spelling is part of the review protocol.
#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash,
)]
#[serde(rename_all = "kebab-case")]
pub enum BwFilter {
    #[default]
    None,
    Yellow,
    Orange,
    Red,
    Green,
}

/// JSON representation of BasicRetouchAdjustments; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct BasicRetouchAdjustments {
    #[serde(default)]
    pub exposure: f32,
    #[serde(default)]
    pub contrast: f32,
    #[serde(default)]
    pub highlights: f32,
    #[serde(default)]
    pub shadows: f32,
    #[serde(default)]
    pub whites: f32,
    #[serde(default)]
    pub blacks: f32,
    #[serde(default)]
    pub temperature: f32,
    #[serde(default)]
    pub offset: f32,
    #[serde(default)]
    pub clarity: f32,
}

/// JSON representation of RetouchCrop; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct RetouchCrop {
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default = "full_extent")]
    pub width: f32,
    #[serde(default = "full_extent")]
    pub height: f32,
}

/// JSON representation of RetouchSettings; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct RetouchSettings {
    #[serde(default)]
    pub adjustments: BasicRetouchAdjustments,
    #[serde(default)]
    pub crop: Option<RetouchCrop>,
    #[serde(default)]
    pub rotation_degrees: f32,
}

/// JSON representation of DiffusionMethod; serialized spelling is part of the review protocol.
#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum DiffusionMethod {
    /// Layered optical mist with a broad, resolution-normalized tail.
    #[default]
    MultiScaleMist,
    /// Edge-aware fine-detail reduction followed by neutral highlight glare.
    EdgeAwareGlow,
}

/// JSON representation of DiffusionSettings; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiffusionSettings {
    pub method: DiffusionMethod,
    pub softness: u8,
    pub highlight_glow: u8,
    pub softness_radius_percent: u16,
    pub glow_radius_percent: u16,
    pub intensity_percent: u16,
    pub highlight_reach: u8,
}

/// JSON representation of CodexAnalysisFlags; serialized spelling is part of the review protocol.
#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct CodexAnalysisFlags {
    pub tags: bool,
    pub note: bool,
    pub rating: bool,
}

/// JSON representation of PanoramaMatchingMode; serialized spelling is part of the review protocol.
#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum PanoramaMatchingMode {
    #[default]
    Automatic,
    Sequential,
    MultiRow,
    FlatMosaic,
}

/// JSON representation of PanoramaProjection; serialized spelling is part of the review protocol.
#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum PanoramaProjection {
    Rectilinear,
    #[default]
    Cylindrical,
    Equirectangular,
    Panini,
}

/// JSON representation of ReviewLabel; serialized spelling is part of the review protocol.
#[derive(
    schemars::JsonSchema,
    Clone,
    Copy,
    Debug,
    Default,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewLabel {
    #[default]
    None,
    Red,
    Yellow,
    Green,
    Blue,
    Purple,
}

/// JSON representation of ReviewMetadataSource; serialized spelling is part of the review protocol.
#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq,
)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewMetadataSource {
    #[default]
    Default,
    Camera,
    Codex,
    Manual,
}

/// JSON representation of ReviewRenderStatus; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewRenderStatus {
    Missing,
    Queued,
    Processing,
    Done,
    Failed,
}

/// JSON representation of ReviewCodexStatus; serialized spelling is part of the review protocol.
#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq,
)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewCodexStatus {
    #[default]
    Missing,
    Queued,
    Processing,
    Done,
    Failed,
    Skipped,
}

/// JSON representation of ReviewDiffusionSettingSource; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewDiffusionSettingSource {
    Current,
    All,
    Daemon,
}

/// JSON representation of ReviewDiffusionScope; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewDiffusionScope {
    Current,
    All,
}

/// JSON representation of ReviewDiffusionJobStatus; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewDiffusionJobStatus {
    Queued,
    Processing,
    Done,
    Failed,
    Cancelled,
}

/// JSON representation of ReviewDiffusionFocusSource; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewDiffusionFocusSource {
    CameraFocus,
    CenterFallback,
}

/// JSON representation of ReviewDiffusionDetailAreaKind; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewDiffusionDetailAreaKind {
    Focus,
    HighContrastHighlight,
    BroadHighlight,
}

/// JSON representation of ReviewPanoramaStatus; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewPanoramaStatus {
    Draft,
    Previewing,
    Ready,
    Rendering,
    Complete,
    Failed,
    Interrupted,
    Cancelled,
}

/// JSON representation of ReviewPanoramaPreviewStatus; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewPanoramaPreviewStatus {
    Queued,
    Processing,
    Done,
    Failed,
    Cancelled,
}

/// JSON representation of ReviewPublishJobStatus; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewPublishJobStatus {
    Running,
    Done,
    Failed,
}

/// JSON representation of ReviewProfileBwFilter; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewProfileBwFilter {
    pub profile_index: usize,
    #[serde(default)]
    pub filter: BwFilter,
}

/// JSON representation of ReviewProfileDiffusionSetting; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewProfileDiffusionSetting {
    pub profile_index: usize,
    pub settings: DiffusionSettings,
}

/// JSON representation of ReviewImageProfileDiffusionSetting; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewImageProfileDiffusionSetting {
    pub image_id: u64,
    pub profile_index: usize,
    pub settings: DiffusionSettings,
}

/// JSON representation of ReviewSamplerJobStatus; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewSamplerJobStatus {
    Preparing,
    Rendering,
    Done,
    Failed,
}

/// JSON representation of ReviewSamplerEntryStatus; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewSamplerEntryStatus {
    Queued,
    Rendering,
    Done,
    Failed,
}

/// JSON representation of ReviewSamplerScope; serialized spelling is part of the review protocol.
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewSamplerScope {
    Current,
    All,
}

/// Crops omitted from a partial request span the complete source dimension.
fn full_extent() -> f32 {
    1.0
}

/// Match the renderer defaults without importing its image-processing dependency tree.
impl Default for DiffusionSettings {
    fn default() -> Self {
        Self {
            method: DiffusionMethod::default(),
            softness: 0,
            highlight_glow: 0,
            softness_radius_percent: 100,
            glow_radius_percent: 100,
            intensity_percent: 100,
            highlight_reach: 50,
        }
    }
}
