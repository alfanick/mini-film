/**
 * Preserve the original review request and profile-selection rules as pure data
 * transformations so hooks can share them and request parity can be tested.
 */
import {
  defaultRetouch,
  imageLabels,
  isSoocProfile,
  normalizeBwFilter,
  publishProfileIndexes,
} from "../core/selectors";
import type { BwFilter, ProfileBwFilter, ReviewImage, ReviewUpdateRequest } from "../core/types";

/** Capture controlled draft values without introducing an imperative dependency on the UI. */
export type ReviewDraftReader = (image: ReviewImage) => Partial<ReviewUpdateRequest>;

/** Keep one supported monochrome filter per available profile in display order. */
export function profileBwFilters(image: ReviewImage): ProfileBwFilter[] {
  const available = new Set(image.profiles.map((profile) => profile.profile_index));
  const byIndex = new Map<number, BwFilter>();
  for (const entry of image.profile_bw_filters || []) {
    const index = Number(entry.profile_index);
    const filter = normalizeBwFilter(entry.filter);
    if (Number.isInteger(index) && available.has(index) && filter !== "none") byIndex.set(index, filter);
  }
  return image.profiles.flatMap((profile): ProfileBwFilter[] => {
    const filter = byIndex.get(profile.profile_index);
    return filter ? [{ profile_index: profile.profile_index, filter }] : [];
  });
}

/** Build the complete legacy request while retaining intentional omitted fields. */
export function reviewRequestBody(image: ReviewImage, patch: Partial<ReviewUpdateRequest> = {}): ReviewUpdateRequest {
  return {
    image_id: image.id,
    rating: patch.rating ?? image.rating,
    label: patch.label ?? (patch.labels ? patch.labels[0] || "none" : image.label || "none"),
    labels: patch.labels ?? imageLabels(image),
    tags: patch.tags ?? image.tags ?? [],
    notes: patch.notes ?? image.notes ?? "",
    retouch: patch.retouch ?? image.retouch ?? defaultRetouch(),
    ...(patch.selected_profile_index === undefined ? {} : { selected_profile_index: patch.selected_profile_index }),
    ...(patch.enabled_profile_indexes === undefined
      ? { publish_profile_indexes: patch.publish_profile_indexes ?? publishProfileIndexes(image) }
      : { enabled_profile_indexes: patch.enabled_profile_indexes }),
    profile_bw_filters: patch.profile_bw_filters ?? profileBwFilters(image),
    advance_after_update: Boolean(patch.advance_after_update),
  };
}

/** Toggle creative profiles without treating the always-available camera rendition as enabled. */
export function toggleEnabledProfile(image: ReviewImage, profileIndex: number): number[] {
  const enabled = new Set(
    image.profiles
      .filter((profile) => !isSoocProfile(profile) && profile.enabled !== false)
      .map((profile) => profile.profile_index),
  );
  if (enabled.has(profileIndex)) enabled.delete(profileIndex);
  else enabled.add(profileIndex);
  return image.profiles.map((profile) => profile.profile_index).filter((index) => enabled.has(index));
}

/** Carry the same published look, otherwise use the next picture's first published look. */
export function carriedProfileIndex(image: ReviewImage, profileIndex: number | undefined): number | undefined {
  if (profileIndex === undefined) return undefined;
  const published = publishProfileIndexes(image);
  const present = image.profiles.some((profile) => profile.profile_index === profileIndex);
  const next = published.includes(profileIndex) ? profileIndex : published[0];
  if (next === undefined && !present) return undefined;
  const resolved = next ?? image.selected_profile_index;
  return resolved === image.selected_profile_index ? undefined : resolved;
}
