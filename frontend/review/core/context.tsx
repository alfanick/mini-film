/**
 * Own the review application's reactive client state in Preact.
 * Features share immutable snapshots through context so rendering follows state,
 * and asynchronous actions can read the latest snapshot without stale closures.
 */
import { createContext, type ComponentChildren } from "preact";
import { useContext } from "preact/hooks";
import { useModel } from "@preact/signals";
import { ReviewModel, type ReviewModelValue, type ReviewStateUpdate } from "./model";
import type { ReviewStateObservation as ReviewState } from "./types";

export type { ReviewStateUpdate } from "./model";

/** Named shell subscriptions observe the catalog model; feature-local state belongs to provider-owned models. */
export interface ReviewContextValue {
  state: ReviewState;
  update: (patch: ReviewStateUpdate) => void;
  getState: () => ReviewState;
}

const ReviewContext = createContext<ReviewModelValue | null>(null);

/** Provide one store per mounted review application, including isolated test mounts. */
export function ReviewProvider({ children }: { children: ComponentChildren }): ComponentChildren {
  const model = useModel(ReviewModel);
  return <ReviewContext.Provider value={model}>{children}</ReviewContext.Provider>;
}

/** Read the shared state and fail early if a feature is mounted outside the app. */
export function useReviewModel(): ReviewModelValue {
  const model = useContext(ReviewContext);
  if (!model) throw new Error("Review features require ReviewProvider");
  return model;
}

/** Every view subscription names its fields; complete snapshots are read only for current event-time selectors. */
export function useReviewContext(keys: readonly (keyof ReviewState)[]): ReviewContextValue {
  const model = useReviewModel();
  for (const key of keys) void model.field(key).value;
  return { state: model.getState(), update: model.update, getState: model.getState };
}
