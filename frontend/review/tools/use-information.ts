/** Information dialogs load profile details on demand and expose recorded commands without owning global DOM state. */
import { useCallback, useEffect, useRef } from "preact/hooks";
import { useReviewContext } from "../core/context";
import { reviewUrl, errorMessage } from "../core/api";
import type { ReviewImage, ReviewProfile, ReviewProfileRender } from "../core/types";
import { profilePp3Key, profilePp3Url, profileRenderIndex } from "./information-helpers";

/** Commands and catalog lookups required by the profile and invocation dialogs. */
export interface InformationActions {
  openProfileInfo(this: void, profile: ReviewProfile | ReviewProfileRender): void;
  closeProfileInfo(this: void): void;
  openCommandInvocation(this: void): void;
  closeCommandInvocation(this: void): void;
  loadProfilePp3(this: void, image: ReviewImage, profile: ReviewProfile): Promise<void>;
  profileByIndex(this: void, index: number | null): ReviewProfile | null;
}

/** Keep profile and invocation overlays mutually exclusive and cancel stale PP3 requests. */
export function useInformation(): InformationActions {
  const { state, update, getState } = useReviewContext();
  const pending = useRef<AbortController | null>(null);

  /** Clear the cached PP3 identity whenever the selected information dialog changes. */
  const clearPp3 = useCallback((): void => {
    pending.current?.abort();
    pending.current = null;
    update({
      profileInfoPp3: {
        key: null,
        text: null,
        error: null,
        loading: false,
      },
    });
  }, [update]);

  /** Open the configured profile corresponding to a rendered variant. */
  const openProfileInfo = useCallback(
    (profile: ReviewProfile | ReviewProfileRender): void => {
      clearPp3();
      update({ commandInvocationOpen: false, profileInfoProfileIndex: profileRenderIndex(profile) });
    },
    [clearPp3, update],
  );

  /** Closing a profile dialog also invalidates any request whose result can no longer be displayed. */
  const closeProfileInfo = useCallback((): void => {
    if (getState().profileInfoProfileIndex === null) return;
    clearPp3();
    update({ profileInfoProfileIndex: null });
  }, [clearPp3, getState, update]);

  /** Open the recorded command after closing its mutually exclusive profile dialog. */
  const openCommandInvocation = useCallback((): void => {
    closeProfileInfo();
    update({ commandInvocationOpen: true });
  }, [closeProfileInfo, update]);

  /** Hide the invocation dialog through reactive state so keyboard and pointer actions agree. */
  const closeCommandInvocation = useCallback((): void => update({ commandInvocationOpen: false }), [update]);

  /** Fetch complete PP3 text only when its details section is opened, preserving its exact download content. */
  const loadProfilePp3 = useCallback(
    async (image: ReviewImage, profile: ReviewProfile): Promise<void> => {
      const key = profilePp3Key(image, profile);
      const current = getState().profileInfoPp3;
      if (current.key === key && (current.loading || current.text !== null || current.error !== null)) return;
      pending.current?.abort();
      const controller = new AbortController();
      pending.current = controller;
      update({ profileInfoPp3: { key, text: null, error: null, loading: true } });
      try {
        const response = await fetch(reviewUrl(profilePp3Url(image, profile)), {
          cache: "no-store",
          signal: controller.signal,
        });
        const body = await response.text();
        if (!response.ok) {
          let message = `PP3 ${response.status}`;
          try {
            const failure: unknown = JSON.parse(body);
            if (failure && typeof failure === "object" && "error" in failure && typeof failure.error === "string")
              message = failure.error || message;
          } catch {
            if (body.trim()) message = body.trim();
          }
          throw new Error(message);
        }
        if (!controller.signal.aborted) update({ profileInfoPp3: { key, text: body, error: null, loading: false } });
      } catch (error) {
        if (!controller.signal.aborted)
          update({
            profileInfoPp3: {
              key,
              text: null,
              error: `Could not load PP3: ${errorMessage(error)}`,
              loading: false,
            },
          });
      }
    },
    [getState, update],
  );

  useEffect(() => () => pending.current?.abort(), []);
  useEffect(() => {
    if (
      state.profileInfoProfileIndex !== null &&
      !state.data?.profiles.some((profile) => profile.index === state.profileInfoProfileIndex)
    )
      closeProfileInfo();
  }, [state.profileInfoProfileIndex, state.data?.profiles, closeProfileInfo]);

  /** Resolve profile metadata from the current snapshot rather than a captured initial catalog. */
  const profileByIndex = (index: number | null): ReviewProfile | null =>
    state.data?.profiles.find((profile) => profile.index === index) || null;
  return {
    openProfileInfo,
    closeProfileInfo,
    openCommandInvocation,
    closeCommandInvocation,
    loadProfilePp3,
    profileByIndex,
  };
}
