/** Compile image-scoped user intentions immediately before sending them, avoiding stale whole-record writes. */
import type { BwFilter, ReviewImage, ReviewLabel, ReviewUpdateRequest } from "../core/types";
import { imageLabels, isSoocProfile } from "../core/selectors";
import { COLOR_LABELS } from "../core/constants";
import { profileBwFilters, toggleEnabledProfile } from "./review-requests";

/** A command owns only these fields; the required legacy request is completed at execution time. */
export type ReviewFields = Partial<Omit<ReviewUpdateRequest, "image_id">>;

/** Stable identities and desired membership make queued toggles compose and explicit retries predictable. */
export type ReviewIntent =
  | { kind: "with-draft"; fields: ReviewFields; intent: ReviewIntent }
  | { kind: "fields"; fields: ReviewFields }
  | { kind: "profile-enabled"; profileIndex: number; enabled: boolean }
  | { kind: "profile-selected"; profileIndex: number }
  | { kind: "profile-solo"; profileIndex: number }
  | { kind: "label"; label: ReviewLabel; enabled: boolean }
  | { kind: "bw-filter"; profileIndex: number; filter: BwFilter };

/** Resolve a semantic intention against the latest accepted server image, not a captured request body. */
export function reviewIntentFields(image: ReviewImage, intent: ReviewIntent): ReviewFields {
  switch (intent.kind) {
    case "with-draft":
      return { ...intent.fields, ...reviewIntentFields(image, intent.intent) };
    case "fields":
      return intent.fields;
    case "profile-enabled": {
      const profile = image.profiles.find((item) => item.profile_index === intent.profileIndex);
      if (!profile || isSoocProfile(profile)) return {};
      return {
        enabled_profile_indexes:
          (profile.enabled !== false) === intent.enabled
            ? image.profiles
                .filter((item) => !isSoocProfile(item) && item.enabled !== false)
                .map((item) => item.profile_index)
            : toggleEnabledProfile(image, intent.profileIndex),
      };
    }
    case "profile-selected": {
      const profile = image.profiles.find((item) => item.profile_index === intent.profileIndex);
      return {
        selected_profile_index: intent.profileIndex,
        ...(profile && !isSoocProfile(profile) && profile.enabled === false
          ? { enabled_profile_indexes: toggleEnabledProfile(image, intent.profileIndex) }
          : {}),
      };
    }
    case "profile-solo": {
      const profile = image.profiles.find((item) => item.profile_index === intent.profileIndex);
      return {
        selected_profile_index: intent.profileIndex,
        enabled_profile_indexes: isSoocProfile(profile || null) ? [] : [intent.profileIndex],
      };
    }
    case "label": {
      const selected = new Set(imageLabels(image));
      if (intent.label === "none") selected.clear();
      else if (intent.enabled) selected.add(intent.label);
      else selected.delete(intent.label);
      const labels = COLOR_LABELS.filter((label) => selected.has(label));
      return { labels, label: labels[0] || "none" };
    }
    case "bw-filter": {
      const filters = profileBwFilters(image).filter((entry) => entry.profile_index !== intent.profileIndex);
      if (intent.filter !== "none") filters.push({ profile_index: intent.profileIndex, filter: intent.filter });
      return { profile_bw_filters: filters };
    }
  }
}

/** Show pending commands without contaminating the confirmed catalog used to compile later requests. */
export function projectReviewIntent(image: ReviewImage, intent: ReviewIntent): ReviewImage {
  const fields = reviewIntentFields(image, intent);
  const enabled = fields.enabled_profile_indexes;
  const filters = fields.profile_bw_filters;
  return {
    ...image,
    ...(fields.rating !== undefined ? { rating: fields.rating } : {}),
    ...(fields.labels !== undefined ? { labels: fields.labels, label: fields.label || "none" } : {}),
    ...(fields.tags !== undefined ? { tags: fields.tags } : {}),
    ...(fields.notes !== undefined ? { notes: fields.notes } : {}),
    ...(fields.retouch !== undefined ? { retouch: fields.retouch } : {}),
    ...(fields.selected_profile_index !== undefined ? { selected_profile_index: fields.selected_profile_index } : {}),
    ...(fields.publish_profile_indexes !== undefined
      ? { publish_profile_indexes: fields.publish_profile_indexes }
      : {}),
    ...(enabled ? { publish_profile_indexes: enabled } : {}),
    ...(filters ? { profile_bw_filters: filters } : {}),
    profiles:
      enabled || filters
        ? image.profiles.map((profile) => ({
            ...profile,
            ...(enabled && !isSoocProfile(profile) ? { enabled: enabled.includes(profile.profile_index) } : {}),
            ...(filters
              ? { bw_filter: filters.find((entry) => entry.profile_index === profile.profile_index)?.filter || "none" }
              : {}),
          }))
        : image.profiles,
  };
}

/** Serialize execution, but let a rejected command finish without poisoning subsequent independent operations. */
export function createCommandQueue(): { enqueue: (execute: () => Promise<void>) => Promise<void> } {
  let tail = Promise.resolve();
  return {
    enqueue(execute: () => Promise<void>): Promise<void> {
      const command = tail.catch(() => undefined).then(execute);
      tail = command;
      return command;
    },
  };
}
