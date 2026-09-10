/** Separate Rust-generated wire contracts from browser-only geometry, drafts, and feature state. */
import type * as Wire from "../generated/responses";
import type * as Requests from "../generated/requests";
/** Supported color labels shared by camera metadata, review filters, and publish selection. */
export type ColorLabel = Exclude<Wire.ReviewLabel, "none">;
/** The unlabelled review value alongside the supported camera color labels. */
export type ReviewLabel = Wire.ReviewLabel;
/** Monochrome filters accepted by the review endpoint. */
export type BwFilter = Wire.BwFilter;
/** Lifecycle states emitted for a single rendered profile. */
export type RenderStatus = Wire.ReviewRenderStatus;
/** Provenance used to keep manual and camera metadata authoritative. */
export type MetadataSource = Wire.ReviewMetadataSource;
/** Inheritance source reported for a profile diffusion setting. */
export type DiffusionSource = Wire.ReviewDiffusionSettingSource;
/** Named diffusion algorithms supported by the daemon. */
export type DiffusionMethod = Wire.DiffusionMethod;
/** Target scope for applying or resetting profile settings. */
export type DiffusionScope = Requests.ReviewDiffusionScope;
/** Source-matching strategies exposed by the panorama API. */
export type PanoramaMatching = Wire.PanoramaMatchingMode;
/** Projection choices available for panorama previews and final renders. */
export type PanoramaProjection = Wire.PanoramaProjection;

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
export type FocusRegion = Wire.GalleryFocusRegion;

/** Editable basic adjustments sent to the shared RAW rendering pipeline. */
export type BasicRetouchAdjustments = Wire.BasicRetouchAdjustments;

/** Non-destructive review adjustments, crop, and rotation stored by the server. */
export type RetouchSettings = Wire.RetouchSettings;

/** Parametric tone settings projected from the configured profile. */
export type ReviewProfileParametricTone = Wire.ReviewProfileParametricTone;

/** Per-channel calibration values displayed in profile information. */
export type ReviewProfileCalibration = Wire.ReviewProfileCalibration;

/** Control points for the profile composite and individual channel curves. */
export type ReviewProfileToneCurves = Wire.ReviewProfileToneCurves;

/** Color and tonal profile metadata used by the information dialog. */
export type ReviewProfileAdjustments = Wire.ReviewProfileAdjustments;

/** Source or emulation sharpening metadata, including whether it was supplied. */
export type ReviewProfileSharpening = Wire.ReviewProfileSharpening;

/** A literal key/value pair from a generated RawTherapee profile section. */
export type ReviewProfilePp3Entry = Wire.ReviewProfilePp3Entry;
/** A named PP3 section and its source provenance for inspection. */
export type ReviewProfilePp3Section = Wire.ReviewProfilePp3Section;

/** Detailed configured-profile metadata projected by the Rust review server. */
export type ReviewProfileMetadata = Wire.ReviewProfileMetadata;

/** A configured creative profile available to the current review session. */
export type ReviewProfile = Wire.ReviewProfile;

/** Normalized diffusion controls accepted by preview and settings endpoints. */
export type DiffusionSettings = Wire.DiffusionSettings;

/** A profile-wide diffusion override applied across reviewed pictures. */
export type ProfileDiffusionSetting = Wire.ReviewProfileDiffusionSetting;
/** A picture-specific override of inherited profile diffusion. */
export type ImageProfileDiffusionSetting = Wire.ReviewImageProfileDiffusionSetting;
/** A monochrome filter override bound to one profile identity. */
export type ProfileBwFilter = Wire.ReviewProfileBwFilter;

/** Per-picture profile availability, render progress, media, and effective settings. */
export type ReviewProfileRender = Wire.ReviewProfileRender;

/** Camera and file metadata projected for review without inventing missing values. */
export type ReviewExif = Wire.GalleryExifData;

/** Analysis capabilities enabled by the current daemon invocation. */
export type CodexFlags = Wire.CodexAnalysisFlags;
/** A reviewed picture with user-owned metadata and all available profile renders. */
export type ReviewImage = Wire.ReviewImage;

/** One panorama projection preview and its processing outcome. */
export type ReviewPanoramaPreview = Wire.ReviewPanoramaPreview;

/** Server-owned panorama sources, choices, progress, and final-image identity. */
export type ReviewPanoramaProject = Wire.ReviewPanoramaProject;

/** Daemon export defaults used to initialize each publish form. */
export type ReviewPublishDefaults = Wire.ReviewPublishDefaults;

/** Progress and output links for an asynchronous publish operation. */
export type ReviewPublishJob = Wire.ReviewPublishJob;

/** A server-grouped sequence of picture identities and its shared expansion state. */
export type ReviewBurst = Wire.ReviewBurst;
/** Navigation and filtering choices synchronized across connected review clients. */
export type ReviewUiState = Wire.ReviewUiState;
/** The complete review JSON snapshot emitted by the state endpoint and SSE. */
export type ReviewStateData = Wire.ReviewStateSnapshot;

/** An incremental SSE projection with explicit image ordering and removals. */
export type ReviewStatePatch = Wire.ReviewStatePatch;
/** Either a complete state replacement or an incremental server update. */
export type ReviewStateMessage = Wire.ReviewStateMessage;

/** A complete UI save payload; the generated request type separately retains sparse and nullable wire inputs. */
export interface MaterializedReviewUpdate extends Omit<
  Requests.ReviewUpdateRequest,
  | "label"
  | "labels"
  | "notes"
  | "retouch"
  | "selected_profile_index"
  | "publish_profile_indexes"
  | "enabled_profile_indexes"
  | "profile_bw_filters"
> {
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

/** Preserve source compatibility while callers adopt the explicit materialized-payload name. */
export type ReviewUpdateRequest = MaterializedReviewUpdate;

/** Expose JSON and collection observations without granting consumers mutation ownership. */
export type ReadonlyData<T> =
  T extends ReadonlyMap<infer Key, infer Value>
    ? ReadonlyMap<ReadonlyData<Key>, ReadonlyData<Value>>
    : T extends ReadonlySet<infer Item>
      ? ReadonlySet<ReadonlyData<Item>>
      : T extends readonly (infer Item)[]
        ? readonly ReadonlyData<Item>[]
        : T extends object
          ? { readonly [Key in keyof T]: ReadonlyData<T[Key]> }
          : T;

/** Public picture observations cannot alter confirmed data or bypass reactive subscriptions. */
export type ReviewImageObservation = ReadonlyData<ReviewImage>;
/** Public catalog observations retain immutable nested profiles, jobs, and image sequences. */
export type ReviewCatalogObservation = ReadonlyData<ReviewStateData>;
/** Public browser state includes read-only sets and maps as well as read-only catalog values. */
export type ReviewStateObservation = ReadonlyData<ReviewState>;
/** Retouch observations cannot bypass the owning draft revision by changing nested adjustments. */
export type RetouchObservation = ReadonlyData<RetouchSettings>;
/** Profile configuration observed by controls and information dialogs without mutation ownership. */
export type ReviewProfileObservation = ReadonlyData<ReviewProfile>;
/** A rendered profile observed through its owning image signal. */
export type ReviewProfileRenderObservation = ReadonlyData<ReviewProfileRender>;
/** Detailed configured metadata retains read-only tone-curve and PP3 sequences. */
export type ReviewProfileMetadataObservation = ReadonlyData<ReviewProfileMetadata>;
/** Sampler previews retain read-only catalog path segments and entries. */
export type SamplerEntryObservation = ReadonlyData<SamplerEntry>;
/** A sampler job observed while the owning model controls replacement and polling. */
export type SamplerJobObservation = ReadonlyData<SamplerJob>;
/** Diffusion preview geometry and aliases remain read-only to consuming views. */
export type DiffusionJobObservation = ReadonlyData<DiffusionJob>;
/** Panorama progress and source identities remain owned by the server. */
export type ReviewPanoramaProjectObservation = ReadonlyData<ReviewPanoramaProject>;
/** Publish progress and output links remain owned by the server. */
export type ReviewPublishJobObservation = ReadonlyData<ReviewPublishJob>;

/** Export selection and encoding options sent by the publish wizard. */
export type PublishRequest = Requests.PublishRequest &
  Required<Pick<Requests.PublishRequest, "min_rating" | "labels" | "tags" | "main_profile_only">>;

/** One catalog profile preview and its current/global enablement state. */
export type SamplerEntry = Wire.ReviewSamplerEntrySnapshot;

/** A sampler catalog with asynchronous rendering progress for its entries. */
export type SamplerJob = Wire.ReviewSamplerJobSnapshot;

/** The subject or highlight category used to explain a preview detail crop. */
export type DiffusionDetailKind = Wire.ReviewDiffusionDetailAreaKind;
/** Whether preview subject framing came from the camera or center fallback. */
export type DiffusionFocusSource = Wire.ReviewDiffusionFocusSource;
/** A normalized preview-detail crop carrying its selection category. */
export type DiffusionDetailArea = Wire.ReviewDiffusionDetailArea;
/** Asynchronous preview output, with legacy response aliases retained for compatibility. */
export type DiffusionJob = Wire.ReviewDiffusionJob;

/** Remembered preview geometry that prevents layout shifts between slider changes. */
export interface DiffusionPreviewContext extends Dimensions {
  focusSource: DiffusionFocusSource | null;
  areas: DiffusionDetailArea[];
}
/** On-demand PP3 text and loading outcome for the currently inspected profile. */
export type ProfileInfoPp3 =
  | { status: "idle"; key: null }
  | { status: "loading"; key: string }
  | { status: "ready"; key: string; text: string }
  | { status: "failed"; key: string; error: string };

/** Shared catalog and viewer state; tool drafts and asynchronous work belong to their feature models. */
export interface ReviewState {
  data: ReviewStateData | null;
  currentId: number | null;
  labelFilters: Set<ReviewLabel>;
  cropEditing: boolean;
  localRetouchDirty: boolean;
  mobileDrawer: string | null;
  pendingProfileSelections: Map<number, number>;
  histogramOpen: boolean;
  informationOpen: boolean;
}
