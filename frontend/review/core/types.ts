// Wire types mirror the JSON projections in review/handle.rs and review/model.rs.
/** Supported color labels shared by camera metadata, review filters, and publish selection. */
export type ColorLabel = "red" | "yellow" | "green" | "blue" | "purple";
/** The unlabelled review value alongside the supported camera color labels. */
export type ReviewLabel = "none" | ColorLabel;
/** Monochrome filters accepted by the review endpoint. */
export type BwFilter = "none" | "yellow" | "orange" | "red" | "green";
/** Lifecycle states emitted for a single rendered profile. */
export type RenderStatus = "missing" | "queued" | "processing" | "done" | "failed";
/** Provenance used to keep manual and camera metadata authoritative. */
export type MetadataSource = "default" | "camera" | "codex" | "manual";
/** Inheritance source reported for a profile diffusion setting. */
export type DiffusionSource = "current" | "all" | "daemon";
/** Named diffusion algorithms supported by the daemon. */
export type DiffusionMethod = "multi-scale-mist" | "edge-aware-glow";
/** Target scope for applying or resetting profile settings. */
export type DiffusionScope = "current" | "all";
/** Source-matching strategies exposed by the panorama API. */
export type PanoramaMatching = "automatic" | "sequential" | "multi-row" | "flat-mosaic";
/** Projection choices available for panorama previews and final renders. */
export type PanoramaProjection = "rectilinear" | "cylindrical" | "equirectangular" | "panini";

/** A two-dimensional point used by normalized crop and focus geometry. */
export interface Point {
  x: number;
  y: number;
}
/** Image or viewport dimensions in the coordinate system of their owner. */
export interface Dimensions {
  width: number;
  height: number;
}
/** A rectangular crop combining its origin and dimensions. */
export interface CropRect extends Point, Dimensions {}
/** A media URL with an optional cache-version timestamp. */
export interface ImageSource {
  url: string | null;
  updatedAt?: string | null;
}
/** A camera focus rectangle with its primary-subject marker. */
export interface FocusRegion extends CropRect {
  primary: boolean;
}

/** Editable basic adjustments sent to the shared RAW rendering pipeline. */
export interface BasicRetouchAdjustments {
  exposure: number;
  contrast: number;
  highlights: number;
  shadows: number;
  whites: number;
  blacks: number;
  temperature: number;
  offset: number;
  clarity: number;
}

/** Non-destructive review adjustments, crop, and rotation stored by the server. */
export interface RetouchSettings {
  adjustments: BasicRetouchAdjustments;
  crop: CropRect | null;
  rotation_degrees: number;
}

/** Parametric tone settings projected from the configured profile. */
export interface ReviewProfileParametricTone {
  shadows: number;
  darks: number;
  lights: number;
  highlights: number;
  shadow_split: number;
  midtone_split: number;
  highlight_split: number;
}

/** Per-channel calibration values displayed in profile information. */
export interface ReviewProfileCalibration {
  red_hue: number;
  red_saturation: number;
  green_hue: number;
  green_saturation: number;
  blue_hue: number;
  blue_saturation: number;
}

/** Control points for the profile composite and individual channel curves. */
export interface ReviewProfileToneCurves {
  composite: [number, number][];
  red: [number, number][];
  green: [number, number][];
  blue: [number, number][];
}

/** Color and tonal profile metadata used by the information dialog. */
export interface ReviewProfileAdjustments {
  exposure: number;
  contrast: number;
  highlights: number;
  shadows: number;
  whites: number;
  blacks: number;
  saturation: number;
  vibrance: number;
  clarity: number;
  parametric: ReviewProfileParametricTone;
  hsl: { hue: number[]; saturation: number[]; luminance: number[] };
  calibration: ReviewProfileCalibration;
  tone_curve: ReviewProfileToneCurves;
}

/** Source or emulation sharpening metadata, including whether it was supplied. */
export interface ReviewProfileSharpening {
  present: boolean;
  amount: number;
  radius: number;
  detail: number;
  masking: number;
}

/** A literal key/value pair from a generated RawTherapee profile section. */
export interface ReviewProfilePp3Entry {
  key: string;
  value: string;
}
/** A named PP3 section and its source provenance for inspection. */
export interface ReviewProfilePp3Section {
  source: string;
  section: string;
  entries: ReviewProfilePp3Entry[];
}

/** Detailed configured-profile metadata projected by the Rust review server. */
export interface ReviewProfileMetadata {
  profile_name: string;
  profile_uuid: string | null;
  look_name: string | null;
  look_uuid: string | null;
  source_profile_name: string | null;
  source_profile_uuid: string | null;
  source_adjustments: ReviewProfileAdjustments;
  source_sharpening: ReviewProfileSharpening;
  emulation_adjustments: ReviewProfileAdjustments;
  emulation_sharpening: ReviewProfileSharpening;
  has_camera_raw_settings: boolean;
  grain: { amount: number; size: number; frequency: number } | null;
  has_hald: boolean;
  has_pp3: boolean;
  pp3_name: string | null;
  pp3_adjustments: ReviewProfilePp3Section[];
}

/** A configured creative profile available to the current review session. */
export interface ReviewProfile {
  index: number;
  identity: string;
  selector: string;
  stem: string;
  sampler_added: boolean;
  enabled_by_default: boolean;
  configured_from_cli: boolean;
  retouch_base: BasicRetouchAdjustments;
  metadata: ReviewProfileMetadata | null;
}

/** Normalized diffusion controls accepted by preview and settings endpoints. */
export interface DiffusionSettings {
  method: DiffusionMethod;
  softness: number;
  highlight_glow: number;
  softness_radius_percent: number;
  glow_radius_percent: number;
  intensity_percent: number;
  highlight_reach: number;
}

/** A profile-wide diffusion override applied across reviewed pictures. */
export interface ProfileDiffusionSetting {
  profile_index: number;
  settings: DiffusionSettings;
}
/** A picture-specific override of inherited profile diffusion. */
export interface ImageProfileDiffusionSetting extends ProfileDiffusionSetting {
  image_id: number;
}
/** A monochrome filter override bound to one profile identity. */
export interface ProfileBwFilter {
  profile_index: number;
  filter: BwFilter;
}

/** Per-picture profile availability, render progress, media, and effective settings. */
export interface ReviewProfileRender {
  profile_index: number;
  profile_stem: string;
  display_name: string | null;
  enabled: boolean;
  status: RenderStatus;
  url: string | null;
  base_url: string | null;
  error: string | null;
  duration_ms: number | null;
  file_size_bytes: number | null;
  width: number | null;
  height: number | null;
  retouch_pending: boolean;
  dcp_profile_filename: string | null;
  lcp_profile_filename: string | null;
  bw_filter_eligible: boolean;
  bw_filter: BwFilter;
  diffusion: { settings: DiffusionSettings; source: DiffusionSource };
  diffusion_settings: DiffusionSettings;
  diffusion_source: DiffusionSource;
  updated_at: string;
}

/** Camera and file metadata projected for review without inventing missing values. */
export interface ReviewExif {
  capture_timestamp: number | null;
  capture_subsecond: string | null;
  rating: number | null;
  file_size_bytes: number | null;
  image_width: number | null;
  image_height: number | null;
  focus_frame_width: number | null;
  focus_frame_height: number | null;
  focus_regions: FocusRegion[];
  focal_length: string | null;
  aperture: string | null;
  shutter_speed: string | null;
  iso: string | null;
  auto_iso: boolean | null;
  iso_auto_hi_limit: string | null;
  white_balance_mode: string | null;
  white_balance_temperature: number | null;
  white_balance_offset: number | null;
  camera_model: string | null;
  shutter_count: number | null;
  shutter_mode: string | null;
  silent_photography: boolean | null;
  release_mode: string | null;
  lens_model: string | null;
  shooting_mode: string | null;
  exposure_compensation: string | null;
  flash: string | null;
  active_d_lighting: string | null;
  tags: string[];
  note: string | null;
}

/** Analysis capabilities enabled by the current daemon invocation. */
export interface CodexFlags {
  tags: boolean;
  note: boolean;
  rating: boolean;
}
/** A reviewed picture with user-owned metadata and all available profile renders. */
export interface ReviewImage {
  id: number;
  capture_time?: string;
  source_type: "compressed" | "raw";
  processing_mode: "profiled" | "direct";
  relative_path: string;
  file_name: string;
  source_file_size_bytes: number | null;
  source_width: number | null;
  source_height: number | null;
  exif: ReviewExif;
  preview_status: RenderStatus;
  thumbnail_url: string | null;
  preview_url: string | null;
  crop_source_url: string | null;
  crop_source_updated_at: string;
  full_url: string | null;
  preview_error: string | null;
  preview_duration_ms: number | null;
  preview_retouch_pending: boolean;
  preview_updated_at: string;
  selected_profile_index: number;
  rating: number;
  label: ReviewLabel;
  labels: ReviewLabel[];
  tags: string[];
  notes: string;
  rating_source: MetadataSource;
  tags_source: MetadataSource;
  notes_source: MetadataSource;
  codex: {
    status: RenderStatus | "skipped";
    flags: CodexFlags;
    model: string;
    error: string | null;
    updated_at: string;
  };
  retouch: RetouchSettings;
  publish_profile_indexes: number[];
  profile_bw_filters: ProfileBwFilter[];
  profile_diffusion_settings: ImageProfileDiffusionSetting[];
  profiles: ReviewProfileRender[];
  updated_at: string;
}

/** One panorama projection preview and its processing outcome. */
export interface ReviewPanoramaPreview {
  matching_mode: PanoramaMatching;
  projection: PanoramaProjection;
  status: "queued" | "processing" | "done" | "failed" | "cancelled";
  url: string | null;
  duration_ms: number | null;
  error: string | null;
  updated_at: string;
}

/** Server-owned panorama sources, choices, progress, and final-image identity. */
export interface ReviewPanoramaProject {
  id: number;
  name: string;
  status: "draft" | "previewing" | "ready" | "rendering" | "complete" | "failed" | "interrupted" | "cancelled";
  matching_mode: PanoramaMatching;
  selected_projection: PanoramaProjection | null;
  output_file_name: string | null;
  result_image_id: number | null;
  progress_stage: string | null;
  progress_completed: number;
  progress_total: number;
  error: string | null;
  created_at: string;
  updated_at: string;
  image_ids: number[];
  previews: ReviewPanoramaPreview[];
}

/** Daemon export defaults used to initialize each publish form. */
export interface ReviewPublishDefaults {
  album: string;
  output_format: string;
  jpg_quality: number;
  resize: string | null;
  long_edge: number | null;
  max_width: number | null;
  max_height: number | null;
  jpeg_subsampling: string;
  strip_metadata: boolean;
  progressive_jpeg: boolean;
  gallery: string | null;
  gallery_thumbnail_long_edge: number;
  gallery_columns: number;
  grain_engine: string;
  normalize_grain_mpix: number | null;
}

/** Progress and output links for an asynchronous publish operation. */
export interface ReviewPublishJob {
  id: number;
  album: string;
  status: "running" | "done" | "failed";
  started_at: string;
  finished_at: string | null;
  processed: number;
  total: number;
  step: string;
  current: string | null;
  linked: number;
  skipped: number;
  galleries: number;
  gallery_urls: string[];
  error: string | null;
}

/** A server-grouped sequence of picture identities and its shared expansion state. */
export interface ReviewBurst {
  id: string;
  image_ids: number[];
  expanded: boolean;
}
/** Navigation and filtering choices synchronized across connected review clients. */
export interface ReviewUiState {
  current_image_id: number | null;
  min_rating: number;
  labels: ReviewLabel[];
}
/** The complete review JSON snapshot emitted by the state endpoint and SSE. */
export interface ReviewStateData {
  version: string;
  invocation: string | null;
  profiles: ReviewProfile[];
  client_count: number;
  codex: {
    enabled: boolean;
    flags: CodexFlags | null;
    model: string | null;
    queued: number;
    processing: number;
    done: number;
    failed: number;
  };
  publish_defaults: ReviewPublishDefaults;
  diffusion_default: DiffusionSettings;
  profile_diffusion_settings: ProfileDiffusionSetting[];
  publish_jobs: ReviewPublishJob[];
  capabilities: { panorama: { available: boolean; reason: string | null }; sampler: boolean; diffusion: boolean };
  panorama: { busy: boolean; projects: ReviewPanoramaProject[] };
  ui: ReviewUiState;
  bursts: ReviewBurst[];
  images: ReviewImage[];
  publish_root: string;
}

/** An incremental SSE projection with explicit image ordering and removals. */
export interface ReviewStatePatch extends Partial<ReviewStateData> {
  type: "patch";
  version: string;
  image_ids?: number[];
  removed_image_ids?: number[];
}
/** Either a complete state replacement or an incremental server update. */
export type ReviewStateMessage = ReviewStateData | ReviewStatePatch;

/** The review write contract; optional fields preserve server-side omission semantics. */
export interface ReviewUpdateRequest {
  image_id: number;
  rating: number;
  label: ReviewLabel;
  labels: ReviewLabel[];
  tags: string[];
  notes: string;
  retouch?: RetouchSettings;
  selected_profile_index?: number;
  publish_profile_indexes?: number[];
  enabled_profile_indexes?: number[];
  profile_bw_filters?: ProfileBwFilter[];
  advance_after_update?: boolean;
}

/** Export selection and encoding options sent by the publish wizard. */
export interface PublishRequest {
  min_rating: number;
  labels: ReviewLabel[];
  tags: string[];
  main_profile_only: boolean;
  album?: string;
  output_format?: string;
  gallery?: string;
  jpg_quality?: number;
  size_mode?: string;
  resize?: string;
  long_edge?: number | null;
  max_width?: number | null;
  max_height?: number | null;
  jpeg_subsampling?: string;
  strip_metadata?: boolean;
  progressive_jpeg?: boolean;
  gallery_thumbnail_long_edge?: number;
  gallery_columns?: number;
  grain_engine?: string;
  normalize_grain?: boolean;
  normalize_grain_mpix?: number | null;
}

/** One catalog profile preview and its current/global enablement state. */
export interface SamplerEntry {
  key: string;
  name: string;
  filename: string;
  parts: string[];
  status: "queued" | "rendering" | "done" | "failed";
  thumbnail_url: string | null;
  duration_ms: number | null;
  error: string | null;
  current_enabled: boolean;
  all_enabled: boolean;
  configured_from_cli: boolean;
  selected: boolean;
}

/** A sampler catalog with asynchronous rendering progress for its entries. */
export interface SamplerJob {
  id: number;
  image_id: number;
  file_name: string;
  status: "preparing" | "rendering" | "done" | "failed";
  source_url: string | null;
  source_width: number | null;
  source_height: number | null;
  completed: number;
  total: number;
  failed: number;
  workers: number;
  error: string | null;
  entries: SamplerEntry[];
}

/** The subject or highlight category used to explain a preview detail crop. */
export type DiffusionDetailKind = "focus" | "high-contrast-highlight" | "broad-highlight";
/** Whether preview subject framing came from the camera or center fallback. */
export type DiffusionFocusSource = "camera-focus" | "center-fallback";
/** A normalized preview-detail crop carrying its selection category. */
export interface DiffusionDetailArea extends CropRect {
  kind: DiffusionDetailKind;
}
/** Asynchronous preview output, with legacy response aliases retained for compatibility. */
export interface DiffusionJob {
  id: number;
  status: "queued" | "processing" | "done" | "failed" | "cancelled";
  image_id: number;
  profile_index: number;
  settings: DiffusionSettings;
  before_url: string | null;
  after_url: string | null;
  preview_width: number | null;
  preview_height: number | null;
  focus_source: DiffusionFocusSource | null;
  detail_areas: DiffusionDetailArea[];
  error: string | null;
  // Preserve the existing client's accepted preview response aliases.
  source_url?: string | null;
  source_width?: number | null;
  source_height?: number | null;
  preview_url?: string | null;
  result_url?: string | null;
  updated_at?: string | null;
  before_updated_at?: string | null;
  after_updated_at?: string | null;
}

/** Remembered preview geometry that prevents layout shifts between slider changes. */
export interface DiffusionPreviewContext extends Dimensions {
  focusSource: DiffusionFocusSource | null;
  areas: DiffusionDetailArea[];
}
/** On-demand PP3 text and loading outcome for the currently inspected profile. */
export interface ProfileInfoPp3 {
  key: string | null;
  text: string | null;
  error: string | null;
  loading: boolean;
}

/** Reactive session and tool state; timers, requests, and gestures stay in their owning hooks. */
export interface ReviewState {
  data: ReviewStateData | null;
  currentId: number | null;
  labelFilters: Set<ReviewLabel>;
  cropEditing: boolean;
  localRetouchDirty: boolean;
  mobileDrawer: string | null;
  pendingProfileSelections: Map<number, number>;
  profileInfoProfileIndex: number | null;
  profileInfoPp3: ProfileInfoPp3;
  commandInvocationOpen: boolean;
  histogramOpen: boolean;
  informationOpen: boolean;
  panoramaOpen: boolean;
  panoramaProjectId: number | null;
  panoramaImageIds: number[];
  panoramaName: string;
  panoramaMatching: PanoramaMatching;
  panoramaProjection: PanoramaProjection;
  panoramaMessage: string;
  samplerOpen: boolean;
  samplerLoading: boolean;
  samplerError: string;
  samplerJob: SamplerJob | null;
  samplerExpandedSections: Set<string>;
  samplerKnownEnabledKeys: Set<string>;
  samplerSelectedKey: string | null;
  samplerPendingSelections: Set<string>;
  diffusionOpen: boolean;
  diffusionLoading: boolean;
  diffusionSaving: boolean;
  diffusionError: string;
  diffusionErrorKind: "preview" | "save" | null;
  diffusionMessage: string;
  diffusionJob: DiffusionJob | null;
  diffusionBefore: ImageSource | null;
  diffusionPreviewContext: DiffusionPreviewContext | null;
  diffusionImageId: number | null;
  diffusionProfileIndex: number | null;
  diffusionSettings: DiffusionSettings | null;
  diffusionSource: DiffusionSource | null;
}
