use super::prelude::*;
use super::scheduler::{ReviewMediaScheduler, ReviewRetouchScheduler};
use super::store::now_string;
use crate::app::dng::DngFallbackConfig;
use crate::app::profile::{Pp3AdjustmentSection, ResolvedProfileMetadata};

use mini_film::{
    CalibrationAdjustments, DEFAULT_GRAIN_REFERENCE_MPIX, GrainEngine, GrainSettings,
    HslAdjustments, ParametricTone, ProfileAdjustments, SharpeningSettings, ToneCurves,
};

pub(crate) const SOOC_PROFILE_INDEX: usize = 1_000_000_000;
pub(crate) const SAMPLER_PROFILE_INDEX_BASE: usize = 500_000_000;
pub(crate) const SOOC_PROFILE_STEM: &str = "sooc";
pub(super) const SOOC_PROFILE_DISPLAY_NAME: &str = "straight out of camera";

#[derive(Clone, Debug)]
pub(crate) struct ReviewConfig {
    pub(crate) address: String,
    pub(crate) input_root: PathBuf,
    pub(crate) output_root: PathBuf,
    pub(crate) hald_dir: PathBuf,
    pub(crate) profiles_root: PathBuf,
    pub(crate) hald_level: u32,
    pub(crate) rawtherapee: PathBuf,
    pub(crate) dng_fallback: DngFallbackConfig,
    pub(crate) output_format: BatchOutputFormat,
    pub(crate) profiles: Vec<ReviewProfile>,
    pub(crate) gallery: Option<ReviewGalleryConfig>,
    pub(crate) convert: PathBuf,
    pub(crate) export: ExportOptions,
    pub(crate) jobs: usize,
    pub(crate) publish_album: String,
    pub(crate) no_grain: bool,
    pub(crate) lcp_root: Option<PathBuf>,
    pub(crate) color_noise_iso_threshold: u32,
    pub(crate) lens_corrections: LensCorrections,
    pub(crate) grain: Option<String>,
    pub(crate) grain_preset: Option<String>,
    pub(crate) grain_seed: Option<u64>,
    pub(crate) grain_engine: GrainEngine,
    pub(crate) normalize_grain_mpix: Option<f64>,
    pub(crate) codex: Option<CodexAnalysisFlags>,
    pub(crate) codex_binary: PathBuf,
    pub(crate) codex_model: String,
    pub(crate) codex_timeout: Duration,
    pub(crate) invocation: Option<String>,
    pub(crate) hugin_bin_dir: Option<PathBuf>,
    pub(crate) trusted_input_sender: Option<std::sync::mpsc::Sender<PathBuf>>,
    pub(crate) converted_input_sender: Option<std::sync::mpsc::Sender<PathBuf>>,
}

#[derive(Clone, Debug)]
pub(crate) struct ReviewGalleryConfig {
    pub(crate) template: GalleryTemplate,
    pub(crate) columns: u32,
    pub(crate) thumbnail_long_edge: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct ReviewProfile {
    pub(crate) index: usize,
    #[serde(default)]
    pub(crate) identity: String,
    pub(crate) selector: String,
    pub(crate) stem: String,
    #[serde(default)]
    pub(crate) sampler_added: bool,
    #[serde(default = "review_default_true")]
    pub(crate) enabled_by_default: bool,
    #[serde(default)]
    pub(crate) configured_from_cli: bool,
    #[serde(default)]
    pub(crate) retouch_base: BasicRetouchAdjustments,
    pub(crate) metadata: Option<ReviewProfileMetadata>,
    #[serde(skip)]
    pub(crate) hald_path: Option<PathBuf>,
}

pub(crate) fn review_profile_identity(
    selector: &str,
    metadata: Option<&ReviewProfileMetadata>,
) -> String {
    if let Some(uuid) = metadata
        .and_then(|metadata| metadata.profile_uuid.as_deref())
        .map(str::trim)
        .filter(|uuid| !uuid.is_empty())
    {
        return format!("xmp:{}", uuid.to_ascii_lowercase());
    }
    format!("selector:{}", selector.trim())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct ReviewProfileMetadata {
    #[serde(default)]
    pub(crate) profile_name: String,
    #[serde(default)]
    pub(crate) profile_uuid: Option<String>,
    #[serde(default)]
    pub(crate) look_name: Option<String>,
    #[serde(default)]
    pub(crate) look_uuid: Option<String>,
    #[serde(default)]
    pub(crate) source_profile_name: Option<String>,
    #[serde(default)]
    pub(crate) source_profile_uuid: Option<String>,
    #[serde(default)]
    pub(crate) source_adjustments: ReviewProfileAdjustments,
    #[serde(default)]
    pub(crate) source_sharpening: ReviewProfileSharpening,
    #[serde(default)]
    pub(crate) emulation_adjustments: ReviewProfileAdjustments,
    #[serde(default)]
    pub(crate) emulation_sharpening: ReviewProfileSharpening,
    #[serde(default)]
    pub(crate) has_camera_raw_settings: bool,
    #[serde(default)]
    pub(crate) grain: Option<ReviewProfileGrain>,
    #[serde(default)]
    pub(crate) has_hald: bool,
    #[serde(default)]
    pub(crate) has_pp3: bool,
    #[serde(default)]
    pub(crate) pp3_name: Option<String>,
    #[serde(default)]
    pub(crate) pp3_adjustments: Vec<ReviewProfilePp3Section>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) struct ReviewProfilePp3Section {
    #[serde(default)]
    pub(crate) source: String,
    #[serde(default)]
    pub(crate) section: String,
    #[serde(default)]
    pub(crate) entries: Vec<ReviewProfilePp3Entry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) struct ReviewProfilePp3Entry {
    #[serde(default)]
    pub(crate) key: String,
    #[serde(default)]
    pub(crate) value: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct ReviewProfileGrain {
    #[serde(default)]
    pub(crate) amount: u8,
    #[serde(default)]
    pub(crate) size: u8,
    #[serde(default)]
    pub(crate) frequency: u8,
}

impl Default for ReviewProfileGrain {
    fn default() -> Self {
        Self::from(GrainSettings::default())
    }
}

impl From<GrainSettings> for ReviewProfileGrain {
    fn from(grain: GrainSettings) -> Self {
        Self {
            amount: grain.amount,
            size: grain.size,
            frequency: grain.frequency,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct ReviewProfileAdjustments {
    #[serde(default)]
    pub(crate) exposure: f32,
    #[serde(default)]
    pub(crate) contrast: f32,
    #[serde(default)]
    pub(crate) highlights: f32,
    #[serde(default)]
    pub(crate) shadows: f32,
    #[serde(default)]
    pub(crate) whites: f32,
    #[serde(default)]
    pub(crate) blacks: f32,
    #[serde(default)]
    pub(crate) saturation: f32,
    #[serde(default)]
    pub(crate) vibrance: f32,
    #[serde(default)]
    pub(crate) clarity: f32,
    #[serde(default)]
    pub(crate) parametric: ReviewProfileParametricTone,
    #[serde(default)]
    pub(crate) hsl: ReviewProfileHslAdjustments,
    #[serde(default)]
    pub(crate) calibration: ReviewProfileCalibration,
    #[serde(default)]
    pub(crate) tone_curve: ReviewProfileToneCurves,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct ReviewProfileSharpening {
    #[serde(default)]
    pub(crate) present: bool,
    #[serde(default)]
    pub(crate) amount: f32,
    #[serde(default)]
    pub(crate) radius: f32,
    #[serde(default)]
    pub(crate) detail: f32,
    #[serde(default)]
    pub(crate) masking: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct ReviewProfileParametricTone {
    #[serde(default)]
    pub(crate) shadows: f32,
    #[serde(default)]
    pub(crate) darks: f32,
    #[serde(default)]
    pub(crate) lights: f32,
    #[serde(default)]
    pub(crate) highlights: f32,
    #[serde(default)]
    pub(crate) shadow_split: f32,
    #[serde(default)]
    pub(crate) midtone_split: f32,
    #[serde(default)]
    pub(crate) highlight_split: f32,
}

impl Default for ReviewProfileParametricTone {
    fn default() -> Self {
        Self {
            shadows: 0.0,
            darks: 0.0,
            lights: 0.0,
            highlights: 0.0,
            shadow_split: 25.0,
            midtone_split: 50.0,
            highlight_split: 75.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct ReviewProfileHslAdjustments {
    #[serde(default)]
    pub(crate) hue: Vec<f32>,
    #[serde(default)]
    pub(crate) saturation: Vec<f32>,
    #[serde(default)]
    pub(crate) luminance: Vec<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct ReviewProfileCalibration {
    #[serde(default)]
    pub(crate) red_hue: f32,
    #[serde(default)]
    pub(crate) red_saturation: f32,
    #[serde(default)]
    pub(crate) green_hue: f32,
    #[serde(default)]
    pub(crate) green_saturation: f32,
    #[serde(default)]
    pub(crate) blue_hue: f32,
    #[serde(default)]
    pub(crate) blue_saturation: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct ReviewProfileToneCurves {
    #[serde(default)]
    pub(crate) composite: Vec<[f32; 2]>,
    #[serde(default)]
    pub(crate) red: Vec<[f32; 2]>,
    #[serde(default)]
    pub(crate) green: Vec<[f32; 2]>,
    #[serde(default)]
    pub(crate) blue: Vec<[f32; 2]>,
}

impl From<&ResolvedProfileMetadata> for ReviewProfileMetadata {
    fn from(metadata: &ResolvedProfileMetadata) -> Self {
        Self {
            profile_name: metadata.profile_name.clone(),
            profile_uuid: metadata.profile_uuid.clone(),
            look_name: metadata.look_name.clone(),
            look_uuid: metadata.look_uuid.clone(),
            source_profile_name: metadata.source_profile_name.clone(),
            source_profile_uuid: metadata.source_profile_uuid.clone(),
            source_adjustments: ReviewProfileAdjustments::from(&metadata.source_adjustments),
            source_sharpening: ReviewProfileSharpening::from(&metadata.source_sharpening),
            emulation_adjustments: ReviewProfileAdjustments::from(&metadata.emulation_adjustments),
            emulation_sharpening: ReviewProfileSharpening::from(&metadata.emulation_sharpening),
            has_camera_raw_settings: metadata.has_camera_raw_settings,
            grain: (metadata.grain.is_enabled()).then_some(metadata.grain.into()),
            has_hald: metadata.hald_path.is_some(),
            has_pp3: metadata.pp3_path.is_some(),
            pp3_name: metadata.pp3_path.as_ref().and_then(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(String::from)
            }),
            pp3_adjustments: metadata
                .pp3_adjustments
                .iter()
                .map(ReviewProfilePp3Section::from)
                .collect(),
        }
    }
}

impl From<&Pp3AdjustmentSection> for ReviewProfilePp3Section {
    fn from(section: &Pp3AdjustmentSection) -> Self {
        Self {
            source: section.source.clone(),
            section: section.section.clone(),
            entries: section
                .entries
                .iter()
                .map(|entry| ReviewProfilePp3Entry {
                    key: entry.key.clone(),
                    value: entry.value.clone(),
                })
                .collect(),
        }
    }
}

impl From<&ProfileAdjustments> for ReviewProfileAdjustments {
    fn from(adjustments: &ProfileAdjustments) -> Self {
        Self {
            exposure: adjustments.exposure,
            contrast: adjustments.contrast,
            highlights: adjustments.highlights,
            shadows: adjustments.shadows,
            whites: adjustments.whites,
            blacks: adjustments.blacks,
            saturation: adjustments.saturation,
            vibrance: adjustments.vibrance,
            clarity: adjustments.clarity,
            parametric: ReviewProfileParametricTone::from(&adjustments.parametric),
            hsl: ReviewProfileHslAdjustments::from(&adjustments.hsl),
            calibration: ReviewProfileCalibration::from(&adjustments.calibration),
            tone_curve: ReviewProfileToneCurves::from(&adjustments.tone_curve),
        }
    }
}

impl From<&SharpeningSettings> for ReviewProfileSharpening {
    fn from(sharpening: &SharpeningSettings) -> Self {
        Self {
            present: sharpening.present,
            amount: sharpening.amount,
            radius: sharpening.radius,
            detail: sharpening.detail,
            masking: sharpening.masking,
        }
    }
}

impl From<&ParametricTone> for ReviewProfileParametricTone {
    fn from(parametric: &ParametricTone) -> Self {
        Self {
            shadows: parametric.shadows,
            darks: parametric.darks,
            lights: parametric.lights,
            highlights: parametric.highlights,
            shadow_split: parametric.shadow_split,
            midtone_split: parametric.midtone_split,
            highlight_split: parametric.highlight_split,
        }
    }
}

impl From<&HslAdjustments> for ReviewProfileHslAdjustments {
    fn from(hsl: &HslAdjustments) -> Self {
        Self {
            hue: hsl.hue.to_vec(),
            saturation: hsl.saturation.to_vec(),
            luminance: hsl.luminance.to_vec(),
        }
    }
}

impl From<&CalibrationAdjustments> for ReviewProfileCalibration {
    fn from(calibration: &CalibrationAdjustments) -> Self {
        Self {
            red_hue: calibration.red_hue,
            red_saturation: calibration.red_saturation,
            green_hue: calibration.green_hue,
            green_saturation: calibration.green_saturation,
            blue_hue: calibration.blue_hue,
            blue_saturation: calibration.blue_saturation,
        }
    }
}

impl From<&ToneCurves> for ReviewProfileToneCurves {
    fn from(curves: &ToneCurves) -> Self {
        Self {
            composite: curves.composite.iter().map(|(x, y)| [*x, *y]).collect(),
            red: curves.red.iter().map(|(x, y)| [*x, *y]).collect(),
            green: curves.green.iter().map(|(x, y)| [*x, *y]).collect(),
            blue: curves.blue.iter().map(|(x, y)| [*x, *y]).collect(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct ReviewHandle {
    pub(super) state: Arc<ArcSwap<ReviewStore>>,
    pub(super) subscribers: Arc<broadcast::Sender<String>>,
    pub(super) state_cache: Arc<ArcSwapOption<serde_json::Value>>,
    pub(super) state_path: PathBuf,
    pub(super) database: super::db::ReviewDatabase,
    pub(super) database_runtime: Arc<tokio::runtime::Runtime>,
    pub(super) input_root: PathBuf,
    pub(super) output_root: PathBuf,
    pub(super) cache_root: PathBuf,
    pub(super) hald_dir: PathBuf,
    pub(super) profiles_root: PathBuf,
    pub(super) hald_level: u32,
    pub(super) rawtherapee: PathBuf,
    pub(super) dng_fallback: DngFallbackConfig,
    pub(super) output_format: BatchOutputFormat,
    pub(super) gallery: Option<ReviewGalleryConfig>,
    pub(super) convert: PathBuf,
    pub(super) export: ExportOptions,
    pub(super) jobs: usize,
    pub(super) no_grain: bool,
    pub(super) lcp_root: Option<PathBuf>,
    pub(super) color_noise_iso_threshold: u32,
    pub(super) lens_corrections: LensCorrections,
    pub(super) grain: Option<String>,
    pub(super) grain_preset: Option<String>,
    pub(super) grain_seed: Option<u64>,
    pub(super) grain_engine: GrainEngine,
    pub(super) normalize_grain_mpix: Option<f64>,
    pub(super) publish_defaults: ReviewPublishDefaults,
    pub(super) publish_jobs: Arc<ArcSwap<Vec<ReviewPublishJob>>>,
    pub(super) next_publish_job_id: Arc<AtomicU64>,
    pub(super) media_scheduler: Arc<ReviewMediaScheduler>,
    pub(super) retouch_scheduler: Arc<ReviewRetouchScheduler>,
    pub(super) codex: Option<ReviewCodexConfig>,
    pub(super) codex_scheduler: Arc<ReviewCodexScheduler>,
    pub(super) invocation: Option<String>,
    pub(super) panorama_config: crate::app::panorama::PanoramaConfig,
    pub(super) panorama_capability: crate::app::panorama::PanoramaCapability,
    pub(super) panorama_projects: Arc<ArcSwap<Vec<ReviewPanoramaProject>>>,
    pub(super) panorama_operation: Arc<std::sync::atomic::AtomicBool>,
    pub(super) sampler_registry: Arc<super::sampler::ReviewSamplerRegistry>,
    pub(super) trusted_input_sender: Option<std::sync::mpsc::Sender<PathBuf>>,
    pub(super) converted_input_sender: Option<std::sync::mpsc::Sender<PathBuf>>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ReviewPanoramaStatus {
    Draft,
    Previewing,
    Ready,
    Rendering,
    Complete,
    Failed,
    Interrupted,
    Cancelled,
}

impl ReviewPanoramaStatus {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Previewing => "previewing",
            Self::Ready => "ready",
            Self::Rendering => "rendering",
            Self::Complete => "complete",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::Cancelled => "cancelled",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self> {
        match value {
            "draft" => Ok(Self::Draft),
            "previewing" => Ok(Self::Previewing),
            "ready" => Ok(Self::Ready),
            "rendering" => Ok(Self::Rendering),
            "complete" => Ok(Self::Complete),
            "failed" => Ok(Self::Failed),
            "interrupted" => Ok(Self::Interrupted),
            "cancelled" => Ok(Self::Cancelled),
            _ => bail!("invalid panorama project status {value:?}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ReviewPanoramaPreviewStatus {
    Queued,
    Processing,
    Done,
    Failed,
    Cancelled,
}

impl ReviewPanoramaPreviewStatus {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Processing => "processing",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "processing" => Ok(Self::Processing),
            "done" => Ok(Self::Done),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => bail!("invalid panorama preview status {value:?}"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct ReviewPanoramaPreview {
    pub(super) matching_mode: PanoramaMatchingMode,
    pub(super) projection: PanoramaProjection,
    pub(super) status: ReviewPanoramaPreviewStatus,
    #[serde(skip)]
    pub(super) path: Option<PathBuf>,
    pub(super) cache_key: Option<String>,
    pub(super) duration_ms: Option<u64>,
    pub(super) error: Option<String>,
    pub(super) updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct ReviewPanoramaProject {
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) status: ReviewPanoramaStatus,
    pub(super) matching_mode: PanoramaMatchingMode,
    pub(super) selected_projection: Option<PanoramaProjection>,
    #[serde(skip)]
    pub(super) output_path: Option<PathBuf>,
    pub(super) result_image_id: Option<u64>,
    pub(super) progress_stage: Option<String>,
    pub(super) progress_completed: usize,
    pub(super) progress_total: usize,
    pub(super) error: Option<String>,
    pub(super) created_at: String,
    pub(super) updated_at: String,
    pub(super) image_ids: Vec<u64>,
    pub(super) previews: Vec<ReviewPanoramaPreview>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct ReviewPanoramaCreateRequest {
    pub(super) image_ids: Vec<u64>,
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) matching_mode: PanoramaMatchingMode,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct ReviewPanoramaUpdateRequest {
    #[serde(default)]
    pub(super) image_ids: Option<Vec<u64>>,
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) matching_mode: Option<PanoramaMatchingMode>,
    #[serde(default)]
    pub(super) selected_projection: Option<PanoramaProjection>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct ReviewPanoramaPreviewRequest {
    #[serde(default)]
    pub(super) image_ids: Option<Vec<u64>>,
    #[serde(default)]
    pub(super) matching_mode: Option<PanoramaMatchingMode>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct ReviewPanoramaRenderRequest {
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) projection: Option<PanoramaProjection>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct ReviewStore {
    pub(super) next_id: u64,
    pub(super) profiles: Vec<ReviewProfile>,
    pub(super) images: Vec<ReviewImage>,
    #[serde(default)]
    pub(super) ui: ReviewUiState,
    #[serde(default)]
    pub(super) exif_schema_version: u32,
    #[serde(skip, default = "default_review_normalize_grain_mpix")]
    pub(super) normalize_grain_mpix: Option<f64>,
    #[serde(skip)]
    pub(super) render_export: ExportOptions,
}

pub(super) const fn default_review_normalize_grain_mpix() -> Option<f64> {
    Some(DEFAULT_GRAIN_REFERENCE_MPIX)
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(super) struct ReviewUiState {
    pub(super) current_image_id: Option<u64>,
    pub(super) min_rating: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct ReviewImage {
    pub(super) id: u64,
    pub(super) raw_path: PathBuf,
    #[serde(default)]
    pub(super) sooc_sidecar_path: Option<PathBuf>,
    pub(super) relative_path: String,
    pub(super) file_name: String,
    #[serde(default)]
    pub(super) exif: GalleryExifData,
    #[serde(default)]
    pub(super) preview: ReviewPreview,
    #[serde(default)]
    pub(super) selected_profile_index: usize,
    #[serde(default)]
    pub(super) rating: u8,
    #[serde(default)]
    pub(super) label: ReviewLabel,
    #[serde(default)]
    pub(super) labels: Vec<ReviewLabel>,
    #[serde(default)]
    pub(super) tags: Vec<String>,
    #[serde(default)]
    pub(super) notes: String,
    #[serde(default)]
    pub(super) rating_source: ReviewMetadataSource,
    #[serde(default)]
    pub(super) tags_source: ReviewMetadataSource,
    #[serde(default)]
    pub(super) notes_source: ReviewMetadataSource,
    #[serde(default)]
    pub(super) codex: ReviewCodexAnalysis,
    #[serde(default)]
    pub(super) retouch: RetouchSettings,
    #[serde(default)]
    pub(super) publish_profile_indexes: Option<Vec<usize>>,
    #[serde(default)]
    pub(super) profile_bw_filters: Vec<ReviewProfileBwFilter>,
    pub(super) profiles: Vec<ReviewProfileRender>,
    pub(super) updated_at: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct ReviewProfileBwFilter {
    pub(super) profile_index: usize,
    #[serde(default)]
    pub(super) filter: BwFilter,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ReviewMetadataSource {
    #[default]
    Default,
    Camera,
    Codex,
    Manual,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct ReviewCodexAnalysis {
    pub(super) status: ReviewCodexStatus,
    #[serde(default)]
    pub(super) flags: CodexAnalysisFlags,
    #[serde(default)]
    pub(super) model: String,
    #[serde(default)]
    pub(super) analysis_key: Option<String>,
    #[serde(default)]
    pub(super) error: Option<String>,
    pub(super) updated_at: String,
}

impl Default for ReviewCodexAnalysis {
    fn default() -> Self {
        Self {
            status: ReviewCodexStatus::Missing,
            flags: CodexAnalysisFlags::none(),
            model: String::new(),
            analysis_key: None,
            error: None,
            updated_at: now_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ReviewCodexStatus {
    #[default]
    Missing,
    Queued,
    Processing,
    Done,
    Failed,
    Skipped,
}

#[derive(Clone, Debug)]
pub(super) struct ReviewCodexConfig {
    pub(super) flags: CodexAnalysisFlags,
    pub(super) codex_binary: PathBuf,
    pub(super) model: String,
    pub(super) timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct ReviewCodexJobKey {
    pub(super) raw: PathBuf,
}

#[derive(Clone, Debug)]
pub(super) struct ScheduledCodexJob {
    pub(super) raw: PathBuf,
    pub(super) analysis_key: String,
}

pub(super) struct ReviewCodexScheduler {
    pub(super) pending: ArcSwap<HashMap<ReviewCodexJobKey, ScheduledCodexJob>>,
}

impl Default for ReviewCodexScheduler {
    fn default() -> Self {
        Self {
            pending: ArcSwap::from_pointee(HashMap::new()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ReviewLabel {
    #[default]
    None,
    Red,
    Yellow,
    Green,
    Blue,
    Purple,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct ReviewPreview {
    pub(super) status: ReviewRenderStatus,
    pub(super) path: Option<PathBuf>,
    pub(super) error: Option<String>,
    #[serde(default)]
    pub(super) duration_ms: Option<u64>,
    #[serde(default)]
    pub(super) render_key: Option<String>,
    pub(super) updated_at: String,
}

impl Default for ReviewPreview {
    fn default() -> Self {
        Self {
            status: ReviewRenderStatus::Missing,
            path: None,
            error: None,
            duration_ms: None,
            render_key: None,
            updated_at: now_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct ReviewProfileRender {
    pub(super) profile_index: usize,
    pub(super) profile_stem: String,
    #[serde(default)]
    pub(super) display_name: Option<String>,
    #[serde(default = "review_default_true")]
    pub(super) enabled: bool,
    pub(super) status: ReviewRenderStatus,
    pub(super) output_path: Option<PathBuf>,
    pub(super) error: Option<String>,
    pub(super) duration_ms: Option<u64>,
    #[serde(default)]
    pub(super) render_key: Option<String>,
    #[serde(default)]
    pub(super) processing_key: Option<String>,
    #[serde(default)]
    pub(super) dcp_profile_filename: Option<String>,
    #[serde(default)]
    pub(super) width: Option<u32>,
    #[serde(default)]
    pub(super) height: Option<u32>,
    pub(super) updated_at: String,
}

const fn review_default_true() -> bool {
    true
}

pub(super) fn image_sooc_source(image: &ReviewImage) -> Option<&Path> {
    image
        .sooc_sidecar_path
        .as_deref()
        .or_else(|| is_rendered_input_file(&image.raw_path).then_some(image.raw_path.as_path()))
}

pub(super) fn review_profile_bw_filter_eligible(profile: &ReviewProfile) -> bool {
    profile.metadata.as_ref().is_some_and(|metadata| {
        metadata.source_adjustments.saturation + metadata.emulation_adjustments.saturation <= -99.0
    })
}

pub(super) fn bw_filter_for_profile_index(image: &ReviewImage, profile_index: usize) -> BwFilter {
    image
        .profile_bw_filters
        .iter()
        .find(|entry| entry.profile_index == profile_index)
        .map(|entry| entry.filter)
        .unwrap_or_default()
}

pub(super) fn effective_bw_filter_for_profile(
    image: &ReviewImage,
    profile: &ReviewProfile,
) -> BwFilter {
    if review_profile_bw_filter_eligible(profile) {
        bw_filter_for_profile_index(image, profile.index)
    } else {
        BwFilter::None
    }
}

pub(super) fn normalize_profile_bw_filters(
    filters: &[ReviewProfileBwFilter],
    renders: &[ReviewProfileRender],
) -> Vec<ReviewProfileBwFilter> {
    renders
        .iter()
        .filter_map(|render| {
            filters
                .iter()
                .rev()
                .find(|entry| entry.profile_index == render.profile_index)
                .and_then(|entry| {
                    (entry.filter != BwFilter::None).then_some(ReviewProfileBwFilter {
                        profile_index: render.profile_index,
                        filter: entry.filter,
                    })
                })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ReviewRenderStatus {
    Missing,
    Queued,
    Processing,
    Done,
    Failed,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct ReviewUpdateRequest {
    pub(super) image_id: u64,
    pub(super) rating: u8,
    #[serde(default)]
    pub(super) label: ReviewLabel,
    #[serde(default)]
    pub(super) labels: Vec<ReviewLabel>,
    pub(super) tags: Vec<String>,
    #[serde(default)]
    pub(super) notes: String,
    #[serde(default)]
    pub(super) retouch: Option<RetouchSettings>,
    #[serde(default)]
    pub(super) selected_profile_index: Option<usize>,
    #[serde(default)]
    pub(super) publish_profile_indexes: Option<Vec<usize>>,
    #[serde(default)]
    pub(super) enabled_profile_indexes: Option<Vec<usize>>,
    #[serde(default)]
    pub(super) profile_bw_filters: Option<Vec<ReviewProfileBwFilter>>,
    #[serde(default)]
    pub(super) advance_after_update: bool,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub(super) struct ReviewUiUpdateRequest {
    #[serde(default)]
    pub(super) current_image_id: Option<u64>,
    #[serde(default)]
    pub(super) min_rating: u8,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct PublishRequest {
    #[serde(default)]
    pub(super) min_rating: u8,
    #[serde(default)]
    pub(super) album: Option<String>,
    #[serde(default)]
    pub(super) labels: Vec<ReviewLabel>,
    #[serde(default)]
    pub(super) tags: Vec<String>,
    #[serde(default)]
    pub(super) output_format: Option<String>,
    #[serde(default)]
    pub(super) gallery: Option<String>,
    #[serde(default)]
    pub(super) jpg_quality: Option<u8>,
    #[serde(default)]
    pub(super) size_mode: Option<String>,
    #[serde(default)]
    pub(super) resize: Option<String>,
    #[serde(default)]
    pub(super) long_edge: Option<u32>,
    #[serde(default)]
    pub(super) max_width: Option<u32>,
    #[serde(default)]
    pub(super) max_height: Option<u32>,
    #[serde(default)]
    pub(super) jpeg_subsampling: Option<String>,
    #[serde(default)]
    pub(super) strip_metadata: Option<bool>,
    #[serde(default)]
    pub(super) progressive_jpeg: Option<bool>,
    #[serde(default)]
    pub(super) gallery_thumbnail_long_edge: Option<u32>,
    #[serde(default)]
    pub(super) gallery_columns: Option<u32>,
    #[serde(default)]
    pub(super) grain_engine: Option<String>,
    #[serde(default)]
    pub(super) normalize_grain: Option<bool>,
    #[serde(default)]
    pub(super) normalize_grain_mpix: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PublishReport {
    pub(crate) linked: u64,
    pub(crate) skipped: u64,
    pub(crate) min_rating: u8,
    pub(crate) galleries: u64,
    pub(super) gallery_roots: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub(crate) struct ReviewPublishCommandArgs {
    pub(crate) state: PathBuf,
    pub(crate) input_root: PathBuf,
    pub(crate) output_root: PathBuf,
    pub(crate) album: String,
    pub(crate) min_rating: u8,
    pub(crate) labels: Vec<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) output_format: BatchOutputFormat,
    pub(crate) hald_dir: PathBuf,
    pub(crate) profiles_root: PathBuf,
    pub(crate) hald_level: u32,
    pub(crate) rawtherapee: PathBuf,
    pub(crate) dng_fallback: DngFallbackConfig,
    pub(crate) lcp_root: Option<PathBuf>,
    pub(crate) convert: PathBuf,
    pub(crate) jobs: usize,
    pub(crate) gallery: Option<GalleryTemplate>,
    pub(crate) gallery_thumbnail_long_edge: u32,
    pub(crate) gallery_columns: u32,
    pub(crate) export: ExportOptions,
    pub(crate) rerender_raw: bool,
    pub(crate) no_grain: bool,
    pub(crate) color_noise_iso_threshold: u32,
    pub(crate) lens_corrections: LensCorrections,
    pub(crate) grain: Option<String>,
    pub(crate) grain_preset: Option<String>,
    pub(crate) grain_seed: Option<u64>,
    pub(crate) grain_engine: GrainEngine,
    pub(crate) normalize_grain_mpix: Option<f64>,
    pub(crate) progress_events: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct ReviewPublishDefaults {
    pub(super) album: String,
    pub(super) output_format: String,
    pub(super) jpg_quality: u8,
    pub(super) resize: Option<String>,
    pub(super) long_edge: Option<u32>,
    pub(super) max_width: Option<u32>,
    pub(super) max_height: Option<u32>,
    pub(super) jpeg_subsampling: String,
    pub(super) strip_metadata: bool,
    pub(super) progressive_jpeg: bool,
    pub(super) gallery: Option<String>,
    pub(super) gallery_thumbnail_long_edge: u32,
    pub(super) gallery_columns: u32,
    pub(super) grain_engine: String,
    pub(super) normalize_grain_mpix: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct ReviewPublishJob {
    pub(super) id: u64,
    pub(super) album: String,
    pub(super) status: ReviewPublishJobStatus,
    pub(super) started_at: String,
    pub(super) finished_at: Option<String>,
    pub(super) processed: u64,
    pub(super) total: u64,
    pub(super) step: String,
    pub(super) current: Option<String>,
    pub(super) linked: u64,
    pub(super) skipped: u64,
    pub(super) galleries: u64,
    #[serde(default)]
    pub(super) gallery_urls: Vec<String>,
    pub(super) error: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ReviewPublishJobStatus {
    Running,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ReviewGalleryDefaults {
    pub(super) template: Option<GalleryTemplate>,
    pub(super) thumbnail_long_edge: u32,
    pub(super) columns: u32,
}

#[derive(Clone, Debug)]
pub(super) struct ReviewPublishOptions {
    pub(super) album: PathBuf,
    pub(super) min_rating: u8,
    pub(super) labels: HashSet<ReviewLabel>,
    pub(super) tags: HashSet<String>,
    pub(super) output_format: BatchOutputFormat,
    pub(super) hald_dir: PathBuf,
    pub(super) profiles_root: PathBuf,
    pub(super) hald_level: u32,
    pub(super) rawtherapee: PathBuf,
    pub(super) dng_fallback: DngFallbackConfig,
    pub(super) convert: PathBuf,
    pub(super) jobs: usize,
    pub(super) export: ExportOptions,
    pub(super) rerender_raw: bool,
    pub(super) no_grain: bool,
    pub(super) lcp_root: Option<PathBuf>,
    pub(super) color_noise_iso_threshold: u32,
    pub(super) lens_corrections: LensCorrections,
    pub(super) grain: Option<String>,
    pub(super) grain_preset: Option<String>,
    pub(super) grain_seed: Option<u64>,
    pub(super) grain_engine: GrainEngine,
    pub(super) normalize_grain_mpix: Option<f64>,
    pub(super) write_metadata: bool,
}

pub(super) struct ReviewPublishOutput<'a> {
    pub(super) input_root: &'a Path,
    pub(super) source: &'a Path,
    pub(super) destination: &'a Path,
    pub(super) image: &'a ReviewImage,
    pub(super) render: Option<&'a ReviewProfileRender>,
    pub(super) profile: Option<&'a ReviewProfile>,
    pub(super) options: &'a ReviewPublishOptions,
}

#[derive(Clone, Debug)]
pub(super) struct ReviewPublishTask {
    pub(super) source: PathBuf,
    pub(super) destination: PathBuf,
    pub(super) image: ReviewImage,
    pub(super) render: Option<ReviewProfileRender>,
    pub(super) profile: Option<ReviewProfile>,
    pub(super) current: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(super) struct ReviewPublishProgress {
    pub(super) processed: u64,
    pub(super) total: u64,
    pub(super) linked: u64,
    pub(super) skipped: u64,
    pub(super) galleries: u64,
    pub(super) step: String,
    pub(super) current: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(super) enum ReviewPublishEvent {
    Progress { progress: ReviewPublishProgress },
    Report { report: PublishReport },
}

pub(super) type ReviewPublishProgressSink<'a> = &'a (dyn Fn(ReviewPublishProgress) + Sync);

impl ReviewPublishDefaults {
    pub(super) fn new(
        album: String,
        output_format: BatchOutputFormat,
        export: &ExportOptions,
        gallery: ReviewGalleryDefaults,
        grain_engine: GrainEngine,
        normalize_grain_mpix: Option<f64>,
    ) -> Self {
        Self {
            album,
            output_format: output_format.to_string(),
            jpg_quality: export.jpg_quality,
            resize: export.resize.clone(),
            long_edge: export.long_edge,
            max_width: export.max_width,
            max_height: export.max_height,
            jpeg_subsampling: export.jpeg_subsampling.to_string(),
            strip_metadata: export.strip_metadata,
            progressive_jpeg: export.progressive_jpeg,
            gallery: gallery.template.map(|template| template.to_string()),
            gallery_thumbnail_long_edge: gallery.thumbnail_long_edge,
            gallery_columns: gallery.columns,
            grain_engine: grain_engine.to_string(),
            normalize_grain_mpix,
        }
    }
}
