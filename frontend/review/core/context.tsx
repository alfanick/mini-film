/**
 * Own the review application's reactive client state in Preact.
 * Features share immutable snapshots through context so rendering follows state,
 * and asynchronous actions can read the latest snapshot without stale closures.
 */
import { createContext, type ComponentChildren } from "preact";
import { useContext } from "preact/hooks";
import { useModel } from "@preact/signals";
import { ReviewModel, type ReviewModelValue, type ReviewStateUpdate } from "./model";
import type { ReviewState } from "./types";

export type { ReviewStateUpdate } from "./model";

/** The only application-wide state interface; feature-local state stays in hooks. */
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

/** Subscribe only to listed client fields, or to the full snapshot for unmigrated consumers. */
export function useReviewContext(keys?: readonly (keyof ReviewState)[]): ReviewContextValue {
  const model = useReviewModel();
  if (keys) for (const key of keys) void model.field(key).value;
  else void model.state.value;
  return { state: model.getState(), update: model.update, getState: model.getState };
}
