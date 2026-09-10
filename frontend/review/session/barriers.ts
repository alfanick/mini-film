/** A fixed command cut identifies locally invalidated outputs without inventing distributed server revisions. */
import type { ReviewImageObservation as ReviewImage, ReviewStateObservation as ReviewState } from "../core/types";

/** Image-scoped tools commit only their inputs; publishing commits the complete existing local catalog draft set. */
export type CommitScope = "all" | readonly number[];

/** Jobs read the committed snapshot and may hold the queue while waiting for these specific outputs. */
export interface CommitContext {
  affectedOutputs: ReadonlySet<string>;
  snapshot: ReviewState;
  refresh: (signal?: AbortSignal) => Promise<ReviewState>;
}

/** Follow the server's retouch/availability/BW invalidation rules, including direct compressed previews. */
export function invalidatedOutputs(before: ReviewImage, after: ReviewImage): readonly string[] {
  const retouchChanged = JSON.stringify(before.retouch) !== JSON.stringify(after.retouch);
  if (!after.profiles.length) return retouchChanged ? [`${after.id}:preview`] : [];
  return after.profiles
    .filter((profile) => {
      if (!profile.enabled) return false;
      const previous = before.profiles.find((item) => item.profile_index === profile.profile_index);
      return retouchChanged || previous?.enabled === false || previous?.bw_filter !== profile.bw_filter;
    })
    .map((profile) => `${after.id}:${profile.profile_index}`);
}
