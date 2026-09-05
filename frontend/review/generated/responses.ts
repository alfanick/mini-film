/** Generated from Rust wire contracts; regenerate with npm run contracts:generate. */

/**
 * JSON representation of ReviewDiffusionDetailAreaKind; serialized spelling is part of the review protocol.
 */
export type ReviewDiffusionDetailAreaKind = "focus" | "high-contrast-highlight" | "broad-highlight";
/**
 * JSON representation of ReviewDiffusionFocusSource; serialized spelling is part of the review protocol.
 */
export type ReviewDiffusionFocusSource = "camera-focus" | "center-fallback";
/**
 * JSON representation of DiffusionMethod; serialized spelling is part of the review protocol.
 */
export type DiffusionMethod = "multi-scale-mist" | "edge-aware-glow";
/**
 * JSON representation of ReviewDiffusionJobStatus; serialized spelling is part of the review protocol.
 */
export type ReviewDiffusionJobStatus = "queued" | "processing" | "done" | "failed" | "cancelled";
/**
 * Existing SSE keepalive tag, distinct from ordinary state messages.
 */
export type ReviewKeepaliveType = "keepalive";
/**
 * Ordinary SSE data preserves the historical untagged snapshot/tagged patch union.
 */
export type ReviewStateMessage = ReviewStateSnapshot | ReviewStatePatch;
/**
 * JSON representation of ReviewCodexStatus; serialized spelling is part of the review protocol.
 */
export type ReviewCodexStatus = "missing" | "queued" | "processing" | "done" | "failed" | "skipped";
/**
 * JSON representation of ReviewLabel; serialized spelling is part of the review protocol.
 */
export type ReviewLabel = "none" | "red" | "yellow" | "green" | "blue" | "purple";
/**
 * JSON representation of ReviewMetadataSource; serialized spelling is part of the review protocol.
 */
export type ReviewMetadataSource = "default" | "camera" | "codex" | "manual";
/**
 * JSON representation of ReviewRenderStatus; serialized spelling is part of the review protocol.
 */
export type ReviewRenderStatus = "missing" | "queued" | "processing" | "done" | "failed";
/**
 * Whether review renders configured profiles or directly displays compressed input.
 */
export type ReviewProcessingMode = "profiled" | "direct";
/**
 * JSON representation of BwFilter; serialized spelling is part of the review protocol.
 */
export type BwFilter = "none" | "yellow" | "orange" | "red" | "green";
/**
 * JSON representation of ReviewDiffusionSettingSource; serialized spelling is part of the review protocol.
 */
export type ReviewDiffusionSettingSource = "current" | "all" | "daemon";
/**
 * The two source categories emitted by the current review projection.
 */
export type ReviewSourceType = "raw" | "compressed";
/**
 * JSON representation of PanoramaMatchingMode; serialized spelling is part of the review protocol.
 */
export type PanoramaMatchingMode = "automatic" | "sequential" | "multi-row" | "flat-mosaic";
/**
 * JSON representation of PanoramaProjection; serialized spelling is part of the review protocol.
 */
export type PanoramaProjection = "rectilinear" | "cylindrical" | "equirectangular" | "panini";
/**
 * JSON representation of ReviewPanoramaPreviewStatus; serialized spelling is part of the review protocol.
 */
export type ReviewPanoramaPreviewStatus = "queued" | "processing" | "done" | "failed" | "cancelled";
/**
 * JSON representation of ReviewPanoramaStatus; serialized spelling is part of the review protocol.
 */
export type ReviewPanoramaStatus =
  "draft" | "previewing" | "ready" | "rendering" | "complete" | "failed" | "interrupted" | "cancelled";
/**
 * JSON representation of ReviewPublishJobStatus; serialized spelling is part of the review protocol.
 */
export type ReviewPublishJobStatus = "running" | "done" | "failed";
export type PatchField_ArrayOf_ReviewBurst = ReviewBurst[];
export type PatchFieldUint = number;
export type PatchField_ArrayOfUint64 = number[];
export type PatchField_ArrayOf_ReviewImage = ReviewImage[];
export type PatchField_NullableString = string | null;
export type PatchField_ArrayOf_ReviewProfileDiffusionSetting = ReviewProfileDiffusionSetting[];
export type PatchField_ArrayOf_ReviewProfile = ReviewProfile[];
export type PatchField_ArrayOf_ReviewPublishJob = ReviewPublishJob[];
export type PatchFieldString = string;
/**
 * The existing incremental-state tag.
 */
export type ReviewPatchType = "patch";
/**
 * JSON representation of ReviewSamplerEntryStatus; serialized spelling is part of the review protocol.
 */
export type ReviewSamplerEntryStatus = "queued" | "rendering" | "done" | "failed";
/**
 * JSON representation of ReviewSamplerJobStatus; serialized spelling is part of the review protocol.
 */
export type ReviewSamplerJobStatus = "preparing" | "rendering" | "done" | "failed";

/**
 * Response schema catalog keeps complete output properties distinct from defaulted input properties.
 */
export interface ResponseContracts {
  diffusion_job: ReviewDiffusionJob;
  error: ReviewError;
  keepalive: ReviewKeepalive;
  message: ReviewStateMessage;
  patch: ReviewStatePatch;
  sampler_job: ReviewSamplerJobSnapshot;
  state: ReviewStateSnapshot;
}
/**
 * Serialized ReviewDiffusionJob with only fields visible to review clients.
 */
export interface ReviewDiffusionJob {
  after_updated_at?: string | null;
  after_url: string | null;
  before_updated_at?: string | null;
  before_url: string | null;
  detail_areas: ReviewDiffusionDetailArea[];
  error: string | null;
  focus_source: ReviewDiffusionFocusSource | null;
  id: number;
  image_id: number;
  preview_height: number | null;
  preview_url?: string | null;
  preview_width: number | null;
  profile_index: number;
  result_url?: string | null;
  settings: DiffusionSettings;
  source_height?: number | null;
  source_url?: string | null;
  source_width?: number | null;
  status: ReviewDiffusionJobStatus;
  updated_at?: string | null;
}
/**
 * Serialized ReviewDiffusionDetailArea with only fields visible to review clients.
 */
export interface ReviewDiffusionDetailArea {
  height: number;
  kind: ReviewDiffusionDetailAreaKind;
  width: number;
  x: number;
  y: number;
}
/**
 * JSON representation of DiffusionSettings; serialized spelling is part of the review protocol.
 */
export interface DiffusionSettings {
  glow_radius_percent: number;
  highlight_glow: number;
  highlight_reach: number;
  intensity_percent: number;
  method: DiffusionMethod;
  softness: number;
  softness_radius_percent: number;
}
/**
 * Ordinary JSON error responses; some routing errors intentionally remain plain text.
 */
export interface ReviewError {
  error: string;
}
/**
 * Payload of the named keepalive SSE event.
 */
export interface ReviewKeepalive {
  datetime: string;
  type: ReviewKeepaliveType;
  version: string;
}
/**
 * Complete untagged snapshot, retaining every existing top-level JSON field.
 */
export interface ReviewStateSnapshot {
  bursts: ReviewBurst[];
  capabilities: ReviewCapabilities;
  client_count: number;
  codex: ReviewCodexSummary;
  diffusion_default: DiffusionSettings;
  images: ReviewImage[];
  invocation: string | null;
  panorama: ReviewPanoramaState;
  profile_diffusion_settings: ReviewProfileDiffusionSetting[];
  profiles: ReviewProfile[];
  publish_defaults: ReviewPublishDefaults;
  publish_jobs: ReviewPublishJob[];
  publish_root: string;
  ui: ReviewUiState;
  version: string;
}
/**
 * Serialized ReviewBurst with only fields visible to review clients.
 */
export interface ReviewBurst {
  expanded: boolean;
  id: string;
  image_ids: number[];
}
/**
 * Feature availability advertised to the current client.
 */
export interface ReviewCapabilities {
  diffusion: boolean;
  panorama: PanoramaCapability;
  sampler: boolean;
}
/**
 * Whether the required panorama tools are available on the daemon host.
 */
export interface PanoramaCapability {
  available: boolean;
  reason: string | null;
}
/**
 * Analysis configuration and queue counts for the review session.
 */
export interface ReviewCodexSummary {
  done: number;
  enabled: boolean;
  failed: number;
  flags: CodexAnalysisFlags | null;
  model: string | null;
  processing: number;
  queued: number;
}
/**
 * JSON representation of CodexAnalysisFlags; serialized spelling is part of the review protocol.
 */
export interface CodexAnalysisFlags {
  note: boolean;
  rating: boolean;
  tags: boolean;
}
/**
 * One complete public image record, including legacy parallel label/profile fields.
 */
export interface ReviewImage {
  codex: ReviewImageCodex;
  crop_source_updated_at: string;
  crop_source_url: string | null;
  exif: GalleryExifData;
  file_name: string;
  full_url: string | null;
  id: number;
  label: ReviewLabel;
  labels: ReviewLabel[];
  notes: string;
  notes_source: ReviewMetadataSource;
  preview_duration_ms: number | null;
  preview_error: string | null;
  preview_retouch_pending: boolean;
  preview_status: ReviewRenderStatus;
  preview_updated_at: string;
  preview_url: string | null;
  processing_mode: ReviewProcessingMode;
  profile_bw_filters: ReviewProfileBwFilter[];
  profile_diffusion_settings: ReviewImageProfileDiffusionSetting[];
  profiles: ReviewProfileRender[];
  publish_profile_indexes: number[];
  rating: number;
  rating_source: ReviewMetadataSource;
  relative_path: string;
  retouch: RetouchSettings;
  selected_profile_index: number;
  source_file_size_bytes: number | null;
  source_height: number | null;
  source_type: ReviewSourceType;
  source_width: number | null;
  tags: string[];
  tags_source: ReviewMetadataSource;
  thumbnail_url: string | null;
  updated_at: string;
}
/**
 * User-visible analysis progress; internal analysis cache keys remain private.
 */
export interface ReviewImageCodex {
  error: string | null;
  flags: CodexAnalysisFlags;
  model: string;
  status: ReviewCodexStatus;
  updated_at: string;
}
/**
 * Serialized GalleryExifData with only fields visible to review clients.
 */
export interface GalleryExifData {
  active_d_lighting: string | null;
  aperture: string | null;
  auto_iso: boolean | null;
  camera_model: string | null;
  capture_subsecond: string | null;
  capture_timestamp: number | null;
  exposure_compensation: string | null;
  file_size_bytes: number | null;
  flash: string | null;
  focal_length: string | null;
  focus_frame_height: number | null;
  focus_frame_width: number | null;
  focus_regions: GalleryFocusRegion[];
  image_height: number | null;
  image_width: number | null;
  iso: string | null;
  iso_auto_hi_limit: string | null;
  lens_model: string | null;
  note: string | null;
  rating: number | null;
  release_mode: string | null;
  shooting_mode: string | null;
  shutter_count: number | null;
  shutter_mode: string | null;
  shutter_speed: string | null;
  silent_photography: boolean | null;
  tags: string[];
  white_balance_mode: string | null;
  white_balance_offset: number | null;
  white_balance_temperature: number | null;
}
/**
 * Serialized GalleryFocusRegion with only fields visible to review clients.
 */
export interface GalleryFocusRegion {
  height: number;
  primary: boolean;
  width: number;
  x: number;
  y: number;
}
/**
 * JSON representation of ReviewProfileBwFilter; serialized spelling is part of the review protocol.
 */
export interface ReviewProfileBwFilter {
  filter: BwFilter;
  profile_index: number;
}
/**
 * JSON representation of ReviewImageProfileDiffusionSetting; serialized spelling is part of the review protocol.
 */
export interface ReviewImageProfileDiffusionSetting {
  image_id: number;
  profile_index: number;
  settings: DiffusionSettings;
}
/**
 * A profile render's public progress, metadata, and currently available media URLs.
 */
export interface ReviewProfileRender {
  base_url: string | null;
  bw_filter: BwFilter;
  bw_filter_eligible: boolean;
  dcp_profile_filename: string | null;
  diffusion: ReviewEffectiveDiffusion;
  diffusion_settings: DiffusionSettings;
  diffusion_source: ReviewDiffusionSettingSource;
  display_name: string | null;
  duration_ms: number | null;
  enabled: boolean;
  error: string | null;
  file_size_bytes: number | null;
  height: number | null;
  lcp_profile_filename: string | null;
  profile_index: number;
  profile_stem: string;
  retouch_pending: boolean;
  status: ReviewRenderStatus;
  updated_at: string;
  url: string | null;
  width: number | null;
}
/**
 * Effective diffusion controls and their inheritance source.
 */
export interface ReviewEffectiveDiffusion {
  settings: DiffusionSettings;
  source: ReviewDiffusionSettingSource;
}
/**
 * JSON representation of RetouchSettings; serialized spelling is part of the review protocol.
 */
export interface RetouchSettings {
  adjustments: BasicRetouchAdjustments;
  crop: RetouchCrop | null;
  rotation_degrees: number;
}
/**
 * JSON representation of BasicRetouchAdjustments; serialized spelling is part of the review protocol.
 */
export interface BasicRetouchAdjustments {
  blacks: number;
  clarity: number;
  contrast: number;
  exposure: number;
  highlights: number;
  offset: number;
  shadows: number;
  temperature: number;
  whites: number;
}
/**
 * JSON representation of RetouchCrop; serialized spelling is part of the review protocol.
 */
export interface RetouchCrop {
  height: number;
  width: number;
  x: number;
  y: number;
}
/**
 * Current panorama projects and the daemon-wide operation lock.
 */
export interface ReviewPanoramaState {
  busy: boolean;
  projects: ReviewPanoramaProject[];
}
/**
 * Panorama project progress with a display filename instead of an output path.
 */
export interface ReviewPanoramaProject {
  created_at: string;
  error: string | null;
  id: number;
  image_ids: number[];
  matching_mode: PanoramaMatchingMode;
  name: string;
  output_file_name: string | null;
  previews: ReviewPanoramaPreview[];
  progress_completed: number;
  progress_stage: string | null;
  progress_total: number;
  result_image_id: number | null;
  selected_projection: PanoramaProjection | null;
  status: ReviewPanoramaStatus;
  updated_at: string;
}
/**
 * A panorama preview replaces its private cached path with a nullable public URL.
 */
export interface ReviewPanoramaPreview {
  duration_ms: number | null;
  error: string | null;
  matching_mode: PanoramaMatchingMode;
  projection: PanoramaProjection;
  status: ReviewPanoramaPreviewStatus;
  updated_at: string;
  url: string | null;
}
/**
 * JSON representation of ReviewProfileDiffusionSetting; serialized spelling is part of the review protocol.
 */
export interface ReviewProfileDiffusionSetting {
  profile_index: number;
  settings: DiffusionSettings;
}
/**
 * Serialized ReviewProfile with only fields visible to review clients.
 */
export interface ReviewProfile {
  configured_from_cli: boolean;
  enabled_by_default: boolean;
  identity: string;
  index: number;
  metadata: ReviewProfileMetadata | null;
  retouch_base: BasicRetouchAdjustments;
  sampler_added: boolean;
  selector: string;
  stem: string;
}
/**
 * Serialized ReviewProfileMetadata with only fields visible to review clients.
 */
export interface ReviewProfileMetadata {
  emulation_adjustments: ReviewProfileAdjustments;
  emulation_sharpening: ReviewProfileSharpening;
  grain: ReviewProfileGrain | null;
  has_camera_raw_settings: boolean;
  has_hald: boolean;
  has_pp3: boolean;
  look_name: string | null;
  look_uuid: string | null;
  pp3_adjustments: ReviewProfilePp3Section[];
  pp3_name: string | null;
  profile_name: string;
  profile_uuid: string | null;
  source_adjustments: ReviewProfileAdjustments;
  source_profile_name: string | null;
  source_profile_uuid: string | null;
  source_sharpening: ReviewProfileSharpening;
}
/**
 * Serialized ReviewProfileAdjustments with only fields visible to review clients.
 */
export interface ReviewProfileAdjustments {
  blacks: number;
  calibration: ReviewProfileCalibration;
  clarity: number;
  contrast: number;
  exposure: number;
  highlights: number;
  hsl: ReviewProfileHslAdjustments;
  parametric: ReviewProfileParametricTone;
  saturation: number;
  shadows: number;
  tone_curve: ReviewProfileToneCurves;
  vibrance: number;
  whites: number;
}
/**
 * Serialized ReviewProfileCalibration with only fields visible to review clients.
 */
export interface ReviewProfileCalibration {
  blue_hue: number;
  blue_saturation: number;
  green_hue: number;
  green_saturation: number;
  red_hue: number;
  red_saturation: number;
}
/**
 * Serialized ReviewProfileHslAdjustments with only fields visible to review clients.
 */
export interface ReviewProfileHslAdjustments {
  hue: number[];
  luminance: number[];
  saturation: number[];
}
/**
 * Serialized ReviewProfileParametricTone with only fields visible to review clients.
 */
export interface ReviewProfileParametricTone {
  darks: number;
  highlight_split: number;
  highlights: number;
  lights: number;
  midtone_split: number;
  shadow_split: number;
  shadows: number;
}
/**
 * Serialized ReviewProfileToneCurves with only fields visible to review clients.
 */
export interface ReviewProfileToneCurves {
  blue: [number, number][];
  composite: [number, number][];
  green: [number, number][];
  red: [number, number][];
}
/**
 * Serialized ReviewProfileSharpening with only fields visible to review clients.
 */
export interface ReviewProfileSharpening {
  amount: number;
  detail: number;
  masking: number;
  present: boolean;
  radius: number;
}
/**
 * Serialized ReviewProfileGrain with only fields visible to review clients.
 */
export interface ReviewProfileGrain {
  amount: number;
  frequency: number;
  size: number;
}
/**
 * Serialized ReviewProfilePp3Section with only fields visible to review clients.
 */
export interface ReviewProfilePp3Section {
  entries: ReviewProfilePp3Entry[];
  section: string;
  source: string;
}
/**
 * Serialized ReviewProfilePp3Entry with only fields visible to review clients.
 */
export interface ReviewProfilePp3Entry {
  key: string;
  value: string;
}
/**
 * Serialized ReviewPublishDefaults with only fields visible to review clients.
 */
export interface ReviewPublishDefaults {
  album: string;
  gallery: string | null;
  gallery_columns: number;
  gallery_thumbnail_long_edge: number;
  grain_engine: string;
  jpeg_subsampling: string;
  jpg_quality: number;
  long_edge: number | null;
  max_height: number | null;
  max_width: number | null;
  normalize_grain_mpix: number | null;
  output_format: string;
  progressive_jpeg: boolean;
  resize: string | null;
  strip_metadata: boolean;
}
/**
 * Serialized ReviewPublishJob with only fields visible to review clients.
 */
export interface ReviewPublishJob {
  album: string;
  current: string | null;
  error: string | null;
  finished_at: string | null;
  galleries: number;
  gallery_urls: string[];
  id: number;
  linked: number;
  processed: number;
  skipped: number;
  started_at: string;
  status: ReviewPublishJobStatus;
  step: string;
  total: number;
}
/**
 * Persisted review navigation shared between clients.
 */
export interface ReviewUiState {
  current_image_id: number | null;
  labels: ReviewLabel[];
  min_rating: number;
}
/**
 * A server patch has complete changed images and optional top-level replacements.
 */
export interface ReviewStatePatch {
  bursts?: PatchField_ArrayOf_ReviewBurst;
  capabilities?: ReviewCapabilities;
  client_count?: PatchFieldUint;
  codex?: ReviewCodexSummary;
  diffusion_default?: DiffusionSettings;
  image_ids?: PatchField_ArrayOfUint64;
  images?: PatchField_ArrayOf_ReviewImage;
  invocation?: PatchField_NullableString;
  panorama?: ReviewPanoramaState;
  profile_diffusion_settings?: PatchField_ArrayOf_ReviewProfileDiffusionSetting;
  profiles?: PatchField_ArrayOf_ReviewProfile;
  publish_defaults?: ReviewPublishDefaults;
  publish_jobs?: PatchField_ArrayOf_ReviewPublishJob;
  publish_root?: PatchFieldString;
  removed_image_ids?: PatchField_ArrayOfUint64;
  type: ReviewPatchType;
  ui?: ReviewUiState;
  version: string;
}
/**
 * Serialized ReviewSamplerJobSnapshot with only fields visible to review clients.
 */
export interface ReviewSamplerJobSnapshot {
  completed: number;
  entries: ReviewSamplerEntrySnapshot[];
  error: string | null;
  failed: number;
  file_name: string;
  id: number;
  image_id: number;
  source_height: number | null;
  source_url: string | null;
  source_width: number | null;
  status: ReviewSamplerJobStatus;
  total: number;
  workers: number;
}
/**
 * Serialized ReviewSamplerEntrySnapshot with only fields visible to review clients.
 */
export interface ReviewSamplerEntrySnapshot {
  all_enabled: boolean;
  configured_from_cli: boolean;
  current_enabled: boolean;
  duration_ms: number | null;
  error: string | null;
  filename: string;
  key: string;
  name: string;
  parts: string[];
  selected: boolean;
  status: ReviewSamplerEntryStatus;
  thumbnail_url: string | null;
}
