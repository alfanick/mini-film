use super::prelude::*;
use super::scheduler::ReviewRetouchScheduler;
use super::store::now_string;

#[derive(Clone, Debug)]
pub(crate) struct ReviewConfig {
    pub(crate) address: String,
    pub(crate) input_root: PathBuf,
    pub(crate) output_root: PathBuf,
    pub(crate) hald_dir: PathBuf,
    pub(crate) profiles_root: PathBuf,
    pub(crate) hald_level: u32,
    pub(crate) rawtherapee: PathBuf,
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
    pub(crate) codex: Option<CodexAnalysisFlags>,
    pub(crate) codex_binary: PathBuf,
    pub(crate) codex_model: String,
    pub(crate) codex_timeout: Duration,
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
    pub(crate) selector: String,
    pub(crate) stem: String,
    #[serde(default)]
    pub(crate) retouch_base: BasicRetouchAdjustments,
}

#[derive(Clone)]
pub(crate) struct ReviewHandle {
    pub(super) state: Arc<ArcSwap<ReviewStore>>,
    pub(super) subscribers: Arc<broadcast::Sender<String>>,
    pub(super) state_cache: Arc<ArcSwapOption<serde_json::Value>>,
    pub(super) state_path: PathBuf,
    pub(super) input_root: PathBuf,
    pub(super) output_root: PathBuf,
    pub(super) hald_dir: PathBuf,
    pub(super) profiles_root: PathBuf,
    pub(super) hald_level: u32,
    pub(super) rawtherapee: PathBuf,
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
    pub(super) publish_defaults: ReviewPublishDefaults,
    pub(super) publish_jobs: Arc<ArcSwap<Vec<ReviewPublishJob>>>,
    pub(super) next_publish_job_id: Arc<AtomicU64>,
    pub(super) retouch_scheduler: Arc<ReviewRetouchScheduler>,
    pub(super) codex: Option<ReviewCodexConfig>,
    pub(super) codex_scheduler: Arc<ReviewCodexScheduler>,
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
    pub(super) profiles: Vec<ReviewProfileRender>,
    pub(super) updated_at: String,
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
    pub(super) updated_at: String,
}

impl Default for ReviewPreview {
    fn default() -> Self {
        Self {
            status: ReviewRenderStatus::Missing,
            path: None,
            error: None,
            updated_at: now_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct ReviewProfileRender {
    pub(super) profile_index: usize,
    pub(super) profile_stem: String,
    pub(super) status: ReviewRenderStatus,
    pub(super) output_path: Option<PathBuf>,
    pub(super) error: Option<String>,
    pub(super) duration_ms: Option<u64>,
    #[serde(default)]
    pub(super) render_key: Option<String>,
    pub(super) updated_at: String,
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
    pub(super) write_metadata: bool,
}

pub(super) struct ReviewPublishOutput<'a> {
    pub(super) input_root: &'a Path,
    pub(super) source: &'a Path,
    pub(super) destination: &'a Path,
    pub(super) image: &'a ReviewImage,
    pub(super) render: &'a ReviewProfileRender,
    pub(super) profile: &'a ReviewProfile,
    pub(super) options: &'a ReviewPublishOptions,
}

#[derive(Clone, Debug)]
pub(super) struct ReviewPublishTask {
    pub(super) source: PathBuf,
    pub(super) destination: PathBuf,
    pub(super) image: ReviewImage,
    pub(super) render: ReviewProfileRender,
    pub(super) profile: ReviewProfile,
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
        }
    }
}
