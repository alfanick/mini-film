/**
 * Own the review application's reactive client state in Preact.
 * Features share immutable snapshots through context so rendering follows state,
 * and asynchronous actions can read the latest snapshot without stale closures.
 */
import { createContext, type ComponentChildren } from "preact";
import { useCallback, useContext, useMemo, useRef, useState } from "preact/hooks";
import { createState } from "./state";
import type { ReviewState } from "./types";

export type ReviewStateUpdate = Partial<ReviewState> | ((state: ReviewState) => Partial<ReviewState>);

/** The only application-wide state interface; feature-local state stays in hooks. */
export interface ReviewContextValue {
  state: ReviewState;
  update: (patch: ReviewStateUpdate) => void;
  getState: () => ReviewState;
}

const ReviewContext = createContext<ReviewContextValue | null>(null);

/** Provide one store per mounted review application, including isolated test mounts. */
export function ReviewProvider({ children }: { children: ComponentChildren }): ComponentChildren {
  const [state, setState] = useState<ReviewState>(createState);
  const latest = useRef<ReviewState>(state);
  const update = useCallback((patch: ReviewStateUpdate): void => {
    const previous = latest.current;
    const next = { ...previous, ...(typeof patch === "function" ? patch(previous) : patch) };
    latest.current = next;
    setState(next);
  }, []);
  const getState = useCallback((): ReviewState => latest.current, []);
  const value = useMemo<ReviewContextValue>(() => ({ state, update, getState }), [state, update, getState]);
  return <ReviewContext.Provider value={value}>{children}</ReviewContext.Provider>;
}

/** Read the shared state and fail early if a feature is mounted outside the app. */
export function useReviewContext(): ReviewContextValue {
  const context = useContext(ReviewContext);
  if (!context) throw new Error("Review features require ReviewProvider");
  return context;
}
