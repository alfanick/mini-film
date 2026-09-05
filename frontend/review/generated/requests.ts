/** Generated from Rust wire contracts; regenerate with npm run contracts:generate. */

/**
 * JSON representation of ReviewDiffusionScope; serialized spelling is part of the review protocol.
 */
export type ReviewDiffusionScope = "current" | "all";
/**
 * JSON representation of DiffusionMethod; serialized spelling is part of the review protocol.
 */
export type DiffusionMethod = "multi-scale-mist" | "edge-aware-glow";
/**
 * JSON representation of PanoramaMatchingMode; serialized spelling is part of the review protocol.
 */
export type PanoramaMatchingMode = "automatic" | "sequential" | "multi-row" | "flat-mosaic";
/**
 * JSON representation of PanoramaProjection; serialized spelling is part of the review protocol.
 */
export type PanoramaProjection = "rectilinear" | "cylindrical" | "equirectangular" | "panini";
/**
 * JSON representation of ReviewLabel; serialized spelling is part of the review protocol.
 */
export type ReviewLabel = "none" | "red" | "yellow" | "green" | "blue" | "purple";
/**
 * JSON representation of BwFilter; serialized spelling is part of the review protocol.
 */
export type BwFilter = "none" | "yellow" | "orange" | "red" | "green";
/**
 * JSON representation of ReviewSamplerScope; serialized spelling is part of the review protocol.
 */
export type ReviewSamplerScope = "current" | "all";

/**
 * Request schema catalog; property names are stable generator entry points, not wire envelopes.
 */
export interface RequestContracts {
  burst: ReviewBurstExpansionRequest;
  diffusion_apply: ReviewDiffusionSettingsRequest;
  diffusion_create: ReviewDiffusionJobRequest;
  diffusion_reset: ReviewDiffusionSettingsResetRequest;
  panorama_create: ReviewPanoramaCreateRequest;
  panorama_previews: ReviewPanoramaPreviewRequest;
  panorama_render: ReviewPanoramaRenderRequest;
  panorama_update: ReviewPanoramaUpdateRequest;
  publish: PublishRequest;
  review: ReviewUpdateRequest;
  sampler_create: ReviewSamplerStartRequest;
  sampler_priority: ReviewSamplerPriorityRequest;
  sampler_select: ReviewSamplerSelectionRequest;
  ui: ReviewUiUpdateRequest;
}
/**
 * Accepted JSON body for ReviewBurstExpansionRequest.
 */
export interface ReviewBurstExpansionRequest {
  expanded: boolean;
}
/**
 * Accepted JSON body for ReviewDiffusionSettingsRequest.
 */
export interface ReviewDiffusionSettingsRequest {
  image_id: number;
  profile_index: number;
  scope: ReviewDiffusionScope;
  settings: DiffusionSettings;
}
/**
 * JSON representation of DiffusionSettings; serialized spelling is part of the review protocol.
 */
export interface DiffusionSettings {
  glow_radius_percent?: number;
  highlight_glow?: number;
  highlight_reach?: number;
  intensity_percent?: number;
  method?: DiffusionMethod;
  softness?: number;
  softness_radius_percent?: number;
}
/**
 * Accepted JSON body for ReviewDiffusionJobRequest.
 */
export interface ReviewDiffusionJobRequest {
  image_id: number;
  profile_index: number;
  settings: DiffusionSettings;
}
/**
 * Accepted JSON body for ReviewDiffusionSettingsResetRequest.
 */
export interface ReviewDiffusionSettingsResetRequest {
  image_id: number;
  profile_index: number;
  scope: ReviewDiffusionScope;
}
/**
 * Accepted JSON body for ReviewPanoramaCreateRequest.
 */
export interface ReviewPanoramaCreateRequest {
  image_ids: number[];
  matching_mode?: PanoramaMatchingMode;
  name?: string | null;
}
/**
 * Accepted JSON body for ReviewPanoramaPreviewRequest.
 */
export interface ReviewPanoramaPreviewRequest {
  image_ids?: number[] | null;
  matching_mode?: PanoramaMatchingMode | null;
}
/**
 * Accepted JSON body for ReviewPanoramaRenderRequest.
 */
export interface ReviewPanoramaRenderRequest {
  name?: string | null;
  projection?: PanoramaProjection | null;
}
/**
 * Accepted JSON body for ReviewPanoramaUpdateRequest.
 */
export interface ReviewPanoramaUpdateRequest {
  image_ids?: number[] | null;
  matching_mode?: PanoramaMatchingMode | null;
  name?: string | null;
  selected_projection?: PanoramaProjection | null;
}
/**
 * Accepted JSON body for PublishRequest.
 */
export interface PublishRequest {
  album?: string | null;
  gallery?: string | null;
  gallery_columns?: number | null;
  gallery_thumbnail_long_edge?: number | null;
  grain_engine?: string | null;
  jpeg_subsampling?: string | null;
  jpg_quality?: number | null;
  labels?: ReviewLabel[];
  long_edge?: number | null;
  main_profile_only?: boolean;
  max_height?: number | null;
  max_width?: number | null;
  min_rating?: number;
  normalize_grain?: boolean | null;
  normalize_grain_mpix?: number | null;
  output_format?: string | null;
  progressive_jpeg?: boolean | null;
  resize?: string | null;
  size_mode?: string | null;
  strip_metadata?: boolean | null;
  tags?: string[];
}
/**
 * Accepted JSON body for ReviewUpdateRequest.
 */
export interface ReviewUpdateRequest {
  advance_after_update?: boolean;
  enabled_profile_indexes?: number[] | null;
  image_id: number;
  label?: ReviewLabel;
  labels?: ReviewLabel[];
  notes?: string;
  profile_bw_filters?: ReviewProfileBwFilter[] | null;
  publish_profile_indexes?: number[] | null;
  rating: number;
  retouch?: RetouchSettings | null;
  selected_profile_index?: number | null;
  tags: string[];
}
/**
 * JSON representation of ReviewProfileBwFilter; serialized spelling is part of the review protocol.
 */
export interface ReviewProfileBwFilter {
  filter?: BwFilter;
  profile_index: number;
}
/**
 * JSON representation of RetouchSettings; serialized spelling is part of the review protocol.
 */
export interface RetouchSettings {
  adjustments?: BasicRetouchAdjustments;
  crop?: RetouchCrop | null;
  rotation_degrees?: number;
}
/**
 * JSON representation of BasicRetouchAdjustments; serialized spelling is part of the review protocol.
 */
export interface BasicRetouchAdjustments {
  blacks?: number;
  clarity?: number;
  contrast?: number;
  exposure?: number;
  highlights?: number;
  offset?: number;
  shadows?: number;
  temperature?: number;
  whites?: number;
}
/**
 * JSON representation of RetouchCrop; serialized spelling is part of the review protocol.
 */
export interface RetouchCrop {
  height?: number;
  width?: number;
  x?: number;
  y?: number;
}
/**
 * Accepted JSON body for ReviewSamplerStartRequest.
 */
export interface ReviewSamplerStartRequest {
  image_id: number;
}
/**
 * Accepted JSON body for ReviewSamplerPriorityRequest.
 */
export interface ReviewSamplerPriorityRequest {
  expanded_keys?: string[];
  visible_keys?: string[];
}
/**
 * Accepted JSON body for ReviewSamplerSelectionRequest.
 */
export interface ReviewSamplerSelectionRequest {
  enabled: boolean;
  scope: ReviewSamplerScope;
}
/**
 * Accepted JSON body for ReviewUiUpdateRequest.
 */
export interface ReviewUiUpdateRequest {
  current_image_id?: number | null;
  labels?: ReviewLabel[];
  min_rating?: number;
}
