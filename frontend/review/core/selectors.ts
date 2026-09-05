/**
 * Derive display and editing values from the Rust catalog without touching the DOM.
 * Sharing these rules keeps the browser, tools and mobile controls in agreement.
 */
import type {
  BasicRetouchAdjustments,
  BwFilter,
  ColorLabel,
  CropRect,
  ImageSource,
  RetouchSettings,
  ReviewImage,
  ReviewProfile,
  ReviewProfileRender,
  ReviewState,
  ReviewStateData,
} from "./types";
import { BW_FILTERS, COLOR_LABELS, COMPRESSED_REVIEW_PREVIEW_LONG_EDGE } from "./constants";

export interface ProfileDisplayState {
  state: string;
  text: string;
  title: string;
}
export type RetouchInput =
  | { adjustments?: Partial<BasicRetouchAdjustments>; crop?: CropRect | null; rotation_degrees?: number }
  | null
  | undefined;

/** Bound an edit to the range supported by the shared RAW pipeline. */
export function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value));
}

/** Normalize rotation to the signed range used by crop geometry and exported edits. */
export function normalizeRotation(value: number): number {
  let rotation = Number.isFinite(value) ? value % 360 : 0;
  if (rotation > 180) rotation -= 360;
  if (rotation < -180) rotation += 360;
  return Math.abs(rotation) < 0.0001 ? 0 : rotation;
}

/** Create a fresh neutral edit so one image cannot mutate another image's defaults. */
export function defaultRetouch(): RetouchSettings {
  return {
    adjustments: {
      exposure: 0,
      contrast: 0,
      highlights: 0,
      shadows: 0,
      whites: 0,
      blacks: 0,
      temperature: 0,
      offset: 0,
      clarity: 0,
    },
    crop: null,
    rotation_degrees: 0,
  };
}

/** Preserve the previous normalization rules for partial, persisted and pasted edits. */
export function normalizedRetouch(input: RetouchInput): RetouchSettings {
  const value = input || defaultRetouch();
  const crop = value.crop
    ? {
        x: clamp(Number(value.crop.x) || 0, 0, 1),
        y: clamp(Number(value.crop.y) || 0, 0, 1),
        width: clamp(Number(value.crop.width) || 1, 0.01, 1),
        height: clamp(Number(value.crop.height) || 1, 0.01, 1),
      }
    : null;
  if (crop) {
    crop.x = clamp(crop.x, 0, 1 - crop.width);
    crop.y = clamp(crop.y, 0, 1 - crop.height);
  }
  const adjustments = value.adjustments || {};
  return {
    adjustments: {
      exposure: clamp(Number(adjustments.exposure) || 0, -4, 4),
      contrast: clamp(Number(adjustments.contrast) || 0, -100, 100),
      highlights: clamp(Number(adjustments.highlights) || 0, -100, 100),
      shadows: clamp(Number(adjustments.shadows) || 0, -100, 100),
      whites: clamp(Number(adjustments.whites) || 0, -100, 100),
      blacks: clamp(Number(adjustments.blacks) || 0, -100, 100),
      temperature: clamp(Number(adjustments.temperature) || 0, -2500, 2500),
      offset: clamp(Number(adjustments.offset) || 0, -100, 100),
      clarity: clamp(Number(adjustments.clarity) || 0, -100, 100),
    },
    crop,
    rotation_degrees: normalizeRotation(Number(value.rotation_degrees) || 0),
  };
}

/** Resolve the selected picture from the current shared browser position. */
export function currentImage(state: ReviewState): ReviewImage | null {
  return state.data?.images.find((image) => image.id === state.currentId) || null;
}

/** Distinguish camera-rendered files from RAW inputs. */
export function isCompressedImage(image: ReviewImage | null): boolean {
  return image?.source_type === "compressed";
}

/** Identify the original compressed-file path where creative editing is disabled. */
export function isDirectCompressedImage(image: ReviewImage | null): boolean {
  return isCompressedImage(image) && (image?.processing_mode ? image.processing_mode !== "profiled" : true);
}

/** Recognize the camera rendition using both supported server representations. */
export function isSoocProfile(profile: ReviewProfileRender | null): boolean {
  return profile?.profile_stem === "sooc" || profile?.profile_index === 1000000000;
}

/** Resolve an enabled profile while retaining immediate optimistic selections. */
export function selectedProfile(
  image: ReviewImage | null,
  state?: Pick<ReviewState, "pendingProfileSelections">,
): ReviewProfileRender | null {
  if (!image || isDirectCompressedImage(image)) return null;
  const profiles = image.profiles.filter((profile) => isSoocProfile(profile) || profile.enabled !== false);
  const index = state?.pendingProfileSelections.get(image.id) ?? image.selected_profile_index;
  return profiles.find((profile) => profile.profile_index === index) || profiles[0] || null;
}

/** Display configured profiles and individual render variants through one name rule. */
export function profileDisplayName(profile: ReviewProfileRender | ReviewProfile | null | undefined): string {
  if (!profile) return "profile";
  return "profile_stem" in profile ? profile.display_name || profile.profile_stem || "profile" : "profile";
}

/** Keep the implicit neutral profile rail hidden unless there is a camera rendition. */
export function profilesAreImplicitOnly(
  state: { data: Pick<ReviewStateData, "profiles"> | null },
  image: ReviewImage | null,
): boolean {
  if (image?.profiles.some(isSoocProfile)) return false;
  const profiles = state.data?.profiles || [];
  return profiles.length === 1 && !String(profiles[0]?.selector || "").trim();
}

/** Return the published/enabled variants in their existing display order. */
export function publishProfileIndexes(image: ReviewImage | null): number[] {
  if (!image || isDirectCompressedImage(image)) return [];
  return Array.isArray(image.publish_profile_indexes)
    ? image.publish_profile_indexes
    : image.profiles.map((profile) => profile.profile_index);
}

/** Normalize both legacy single labels and the current multiple-label field. */
export function imageLabels(image: ReviewImage | null): ColorLabel[] {
  const labels = image?.labels?.length ? image.labels : image?.label ? [image.label] : [];
  return labels.filter(
    (label): label is ColorLabel => label !== "none" && COLOR_LABELS.some((known) => known === label),
  );
}

/** Filter the catalog according to the shared rating and local color selection. */
export function filteredImages(state: ReviewState): ReviewImage[] {
  return (state.data?.images || []).filter(
    (image) =>
      image.rating >= (state.data?.ui.min_rating || 0) &&
      (state.labelFilters.size === 0 || imageLabels(image).some((label) => state.labelFilters.has(label))),
  );
}

/** Keep derivative cache-busting identical for previews, tools and downloads. */
export function versionedUrl(url: string, updatedAt?: string | null): string {
  return `${url}?v=${encodeURIComponent(updatedAt || "")}`;
}

/** Use full compressed media on large viewports and the selected profile otherwise. */
export function mainImageSource(
  image: ReviewImage | null,
  selected: ReviewProfileRender | null,
  longEdge = Math.max(window.innerWidth, window.innerHeight),
): ImageSource {
  if (selected?.url) return { url: selected.url, updatedAt: selected.updated_at };
  if (isDirectCompressedImage(image) && image?.full_url && longEdge > COMPRESSED_REVIEW_PREVIEW_LONG_EDGE)
    return { url: image.full_url, updatedAt: image.preview_updated_at || image.updated_at };
  return { url: image?.preview_url || null, updatedAt: image?.preview_updated_at || image?.updated_at || null };
}

/** Express queued/rendering/local-draft state without coupling it to markup. */
export function profileDisplayState(
  image: ReviewImage | null,
  profile: ReviewProfileRender | null,
  localDirty = false,
): ProfileDisplayState {
  const direct = isDirectCompressedImage(image);
  if ((direct && !image) || (!direct && !profile))
    return {
      state: "waiting",
      text: "waiting",
      title: direct ? "waiting for image render" : "waiting for profile render",
    };
  const status = direct ? image?.preview_status : profile?.status;
  const pending = direct ? image?.preview_retouch_pending : profile?.retouch_pending;
  if (localDirty && image)
    return {
      state: "retouch-draft",
      text: direct ? "crop draft" : "retouch draft",
      title: direct
        ? "local crop preview; server render will queue after edits settle"
        : "local draft preview; server render will queue after edits settle",
    };
  if (pending && (status === "queued" || status === "processing"))
    return {
      state: `retouch-${status}`,
      text: `${direct ? "crop" : "retouch"} ${status === "processing" ? "rendering" : "queued"}`,
      title: `server-side ${direct ? "crop" : "retouch"} render is ${status === "processing" ? "running" : "queued"}`,
    };
  return {
    state: status || "waiting",
    text: direct && status === "done" ? "ready" : status || "waiting",
    title:
      (direct ? image?.preview_error : profile?.error) ||
      status ||
      (direct ? "waiting for image render" : "waiting for profile render"),
  };
}

/** Accept only the monochrome filters that are supported by the backend. */
export function normalizeBwFilter(value: string | null | undefined): BwFilter {
  return BW_FILTERS.find((filter) => filter === value) || "none";
}

/** Format compact labels without locale-dependent title casing. */
export function capitalize(value: string | null | undefined): string {
  return value ? `${value.charAt(0).toUpperCase()}${value.slice(1)}` : "";
}

/** Match existing UI counts in statuses and selection summaries. */
export function plural(count: number, singular: string): string {
  return Number(count) === 1 ? singular : `${singular}s`;
}

/** Use safe attachment names while preserving recognizable picture/profile names. */
export function safeDownloadPart(value: string): string {
  return String(value || "image")
    .trim()
    .replace(/[\\/:*?"<>|]+/g, "-")
    .split("")
    .filter((char) => char >= " ")
    .join("")
    .replace(/\s+/g, " ")
    .replace(/^-+|-+$/g, "")
    .slice(0, 120);
}

/** Convert a color label into the compact control caption used throughout review. */
export function labelLetter(label: string): string {
  const letters: Record<string, string> = { red: "R", yellow: "Y", green: "G", blue: "B", purple: "P" };
  return letters[label] || "";
}
