/**
 * Reconcile full snapshots and incremental server events without mutating prior state.
 * Pending local profile choices remain visible until the server acknowledges them.
 */
import type {
  ReviewImageObservation as ReviewImage,
  ReviewStateObservation as ReviewState,
  ReviewCatalogObservation as ReviewStateData,
  ReviewStateMessage,
  ReviewStatePatch,
  ReadonlyData,
} from "./types";
import { COLOR_LABELS } from "./constants";
import { filteredImages } from "./selectors";

/** Image deltas and version checks have dedicated handlers; every other generated patch field must be copied. */
type ScalarPatchKey = Exclude<
  keyof ReviewStatePatch,
  "type" | "version" | "images" | "image_ids" | "removed_image_ids"
>;

/** Exhaustive coverage makes newly generated Rust patch properties a compile-time reconciliation decision. */
const SCALAR_PATCH_FIELDS = {
  profiles: true,
  client_count: true,
  codex: true,
  publish_defaults: true,
  diffusion_default: true,
  profile_diffusion_settings: true,
  publish_jobs: true,
  capabilities: true,
  panorama: true,
  bursts: true,
  ui: true,
  publish_root: true,
  invocation: true,
} as const satisfies Readonly<Record<ScalarPatchKey, true>>;

/** Narrow reflective table keys without claiming arbitrary input strings are generated contract fields. */
function isScalarPatchKey(key: string): key is ScalarPatchKey {
  return Object.prototype.hasOwnProperty.call(SCALAR_PATCH_FIELDS, key);
}

/** Identify incremental messages; full snapshots intentionally have no tag. */
export function isStatePatch(message: ReadonlyData<ReviewStateMessage>): message is ReadonlyData<ReviewStatePatch> {
  return "type" in message && message.type === "patch";
}

/** Apply the existing patch protocol, including explicit image ordering and removals. */
export function mergeSnapshot(
  previous: ReviewStateData | null,
  message: ReadonlyData<ReviewStateMessage>,
): ReviewStateData | null {
  if (!isStatePatch(message)) return message;
  if (!previous) return null;
  const data = { ...previous };
  for (const key of Object.keys(SCALAR_PATCH_FIELDS)) {
    if (isScalarPatchKey(key) && Object.prototype.hasOwnProperty.call(message, key))
      Object.assign(data, { [key]: message[key] });
  }
  if (message.images !== undefined || message.removed_image_ids !== undefined) {
    const images = new Map(data.images.map((image) => [image.id, image]));
    for (const id of message.removed_image_ids || []) images.delete(id);
    for (const image of message.images || []) images.set(image.id, image);
    data.images = message.image_ids
      ? message.image_ids.map((id) => images.get(id)).filter((image): image is ReviewImage => image !== undefined)
      : Array.from(images.values());
  }
  return data;
}

/** Protect optimistic profile choices while accepting all other server-owned fields. */
export function reconcileReview(state: ReviewState, message: ReadonlyData<ReviewStateMessage>): Partial<ReviewState> {
  const snapshot = mergeSnapshot(state.data, message);
  if (!snapshot) return {};
  const pending = new Map(state.pendingProfileSelections);
  const previous = new Map((state.data?.images || []).map((image) => [image.id, image]));
  const images = snapshot.images.map((image): ReviewImage => {
    const selected = pending.get(image.id);
    if (selected !== undefined) {
      if (image.selected_profile_index === selected) pending.delete(image.id);
      else return { ...image, selected_profile_index: selected };
    } else {
      const current = previous.get(image.id);
      if (
        current &&
        current.selected_profile_index !== image.selected_profile_index &&
        image.updated_at &&
        current.updated_at &&
        image.updated_at < current.updated_at
      )
        return { ...image, selected_profile_index: current.selected_profile_index };
    }
    return image;
  });
  const label = (snapshot.ui.labels || [])
    .map((value) => String(value).trim().toLowerCase())
    .find((value) => COLOR_LABELS.some((known) => known === value));
  const nextLabels = new Set(COLOR_LABELS.filter((value) => value === label));
  const labelFilters =
    nextLabels.size === state.labelFilters.size &&
    Array.from(nextLabels).every((value) => state.labelFilters.has(value))
      ? state.labelFilters
      : nextLabels;
  const pendingProfileSelections =
    pending.size === state.pendingProfileSelections.size &&
    Array.from(pending).every(([id, value]) => state.pendingProfileSelections.get(id) === value)
      ? state.pendingProfileSelections
      : pending;
  const data = { ...snapshot, images };
  const next: ReviewState = {
    ...state,
    data,
    labelFilters,
    pendingProfileSelections,
    currentId: snapshot.ui.current_image_id,
  };
  const visible = filteredImages(next);
  const currentId = visible.some((image) => image.id === next.currentId) ? next.currentId : (visible[0]?.id ?? null);
  return { data, labelFilters, pendingProfileSelections, currentId };
}

/** Retain equal catalog entities across complete responses so unrelated leaf subscriptions remain quiet. */
export function retainSnapshotIdentity(previous: ReviewStateData | null, next: ReviewStateData): ReviewStateData {
  if (!previous) return next;
  const byId = new Map(previous.images.map((image) => [image.id, image]));
  const images = next.images.map((image) => {
    const old = byId.get(image.id);
    return old === image || (old && JSON.stringify(old) === JSON.stringify(image)) ? old : image;
  });
  const unchangedImages =
    images.length === previous.images.length && images.every((image, index) => image === previous.images[index]);
  const data = { ...next, images: unchangedImages ? previous.images : images };
  for (const key of Object.keys(next) as (keyof ReviewStateData)[]) {
    if (key !== "images" && JSON.stringify(previous[key]) === JSON.stringify(next[key]))
      Object.assign(data, { [key]: previous[key] });
  }
  return Object.keys(data).every((key) => data[key as keyof ReviewStateData] === previous[key as keyof ReviewStateData])
    ? previous
    : data;
}
