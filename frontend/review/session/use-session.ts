/**
 * Own the review connection and serialized write queue as a Preact lifecycle hook.
 * Live events, optimistic selection and navigation all use one current state snapshot.
 */
import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import { useReviewContext } from "../core/context";
import { requestJson, reviewUrl, errorMessage } from "../core/api";
import { isStatePatch, reconcileReview } from "../core/reconcile";
import {
  currentImage,
  filteredImages,
  imageLabels,
  isSoocProfile,
  profilesAreImplicitOnly,
  selectedProfile,
} from "../core/selectors";
import type {
  BwFilter,
  ReviewImage,
  ReviewLabel,
  ReviewProfileRender,
  ReviewStateMessage,
  ReviewUiState,
  ReviewUpdateRequest,
} from "../core/types";
import { COLOR_LABELS } from "../core/constants";
import { carriedProfileIndex, profileBwFilters, reviewRequestBody, toggleEnabledProfile } from "./review-requests";
import type { ReviewDraftReader } from "./review-requests";

/** Stable callback properties allow features to pass actions without losing their receiver. */
export interface ReviewActions {
  applyMessage: (message: ReviewStateMessage) => void;
  saveReview: (patch?: Partial<ReviewUpdateRequest>) => Promise<void>;
  saveImageReview: (image: ReviewImage, patch: Partial<ReviewUpdateRequest>) => Promise<void>;
  setDraftReader: (reader: ReviewDraftReader | null) => void;
  updateSharedUi: (patch: Partial<ReviewUiState>) => Promise<void>;
  move: (delta: number) => Promise<void>;
  rate: (rating: number, advance?: boolean) => Promise<void>;
  selectImage: (image: ReviewImage) => Promise<void>;
  selectProfile: (profile: ReviewProfileRender) => Promise<void>;
  stepProfile: (delta: number) => Promise<void>;
  toggleProfile: (profile: ReviewProfileRender, solo?: boolean) => Promise<void>;
  toggleLabel: (label: ReviewLabel) => Promise<void>;
  setBwFilter: (profile: ReviewProfileRender, filter: BwFilter) => Promise<void>;
  toggleBurst: (id: string, expanded: boolean) => Promise<void>;
}

/** Expose connection presentation separately from the shared review action callbacks. */
export interface ReviewSession extends ReviewActions {
  connected: boolean;
  connectionError: string;
  keepalive: { title: string; tick: number };
}

/** Attach one SSE stream and one save queue for the lifetime of the mounted review app. */
export function useReviewSession(): ReviewSession {
  const { update, getState } = useReviewContext();
  const queue = useRef<Promise<void>>(Promise.resolve());
  const draftReader = useRef<ReviewDraftReader | null>(null);
  const [connected, setConnected] = useState<boolean>(false);
  const [connectionError, setConnectionError] = useState<string>("");
  const [keepalive, setKeepalive] = useState<{ title: string; tick: number }>({ title: "", tick: 0 });

  /** Read controlled inputs synchronously when capturing a queued review action. */
  const setDraftReader = useCallback((reader: ReviewDraftReader | null): void => {
    draftReader.current = reader;
  }, []);

  /** Merge a server acknowledgement without replacing newer optimistic choices. */
  const applyMessage = useCallback(
    (message: ReviewStateMessage): void => {
      const state = getState();
      if (state.data?.version && message.version && state.data.version !== message.version) {
        window.location.reload();
        return;
      }
      if (isStatePatch(message) && !state.data) {
        void requestJson<ReviewStateMessage>("api/state")
          .then((data) => {
            if (data) update((previous) => reconcileReview(previous, data));
          })
          .catch((error) => setConnectionError(errorMessage(error)));
        return;
      }
      update((previous) => reconcileReview(previous, message));
    },
    [getState, update],
  );

  /** Serialize writes so faster requests cannot roll back a later user edit. */
  const enqueue = useCallback(
    (path: string, body: unknown, method: "POST" | "PATCH" = "POST"): Promise<void> => {
      const task = queue.current
        .catch(() => undefined)
        .then(async (): Promise<void> => {
          const message = await requestJson<ReviewStateMessage>(path, method, body);
          if (message) applyMessage(message);
        });
      queue.current = task;
      return task;
    },
    [applyMessage],
  );

  /** Capture the complete review request before it enters the asynchronous save queue. */
  const saveImageReview = useCallback(
    (image: ReviewImage, patch: Partial<ReviewUpdateRequest>): Promise<void> => {
      const body = reviewRequestBody(image, patch);
      const selected = patch.selected_profile_index;
      if (selected !== undefined)
        update((state) => ({
          pendingProfileSelections: new Map(state.pendingProfileSelections).set(image.id, selected),
          data: state.data
            ? {
                ...state.data,
                images: state.data.images.map((item) =>
                  item.id === image.id ? { ...item, selected_profile_index: selected } : item,
                ),
              }
            : null,
        }));
      return enqueue("api/review", body).catch((error: unknown) => {
        if (selected !== undefined)
          update((state) => {
            const pending = new Map(state.pendingProfileSelections);
            pending.delete(image.id);
            return { pendingProfileSelections: pending };
          });
        throw error;
      });
    },
    [enqueue, update],
  );

  /** Save the current image using the latest snapshot, including local retouch drafts. */
  const saveReview = useCallback(
    async (patch: Partial<ReviewUpdateRequest> = {}): Promise<void> => {
      const image = currentImage(getState());
      if (image) await saveImageReview(image, { ...draftReader.current?.(image), ...patch });
    },
    [getState, saveImageReview],
  );

  /** Share navigation/filter changes with other browsers without losing pending saves. */
  const updateSharedUi = useCallback(
    (patch: Partial<ReviewUiState>): Promise<void> => {
      const state = getState();
      const label = patch.labels?.find((candidate) => COLOR_LABELS.some((known) => known === candidate));
      const body: ReviewUiState = {
        current_image_id: patch.current_image_id ?? state.currentId,
        min_rating: patch.min_rating ?? state.data?.ui.min_rating ?? 0,
        labels: patch.labels === undefined ? Array.from(state.labelFilters) : label ? [label] : [],
      };
      update({ labelFilters: new Set(body.labels) });
      return enqueue("api/ui", body);
    },
    [getState, update, enqueue],
  );

  /** Apply the original published-profile fallback after the server changes pictures. */
  const carryProfile = useCallback(
    async (imageId: number | null, profileIndex: number | undefined): Promise<void> => {
      const image = getState().data?.images.find((candidate) => candidate.id === imageId);
      if (!image) return;
      const selected = carriedProfileIndex(image, profileIndex);
      if (selected !== undefined) await saveImageReview(image, { selected_profile_index: selected });
    },
    [getState, saveImageReview],
  );

  /** Save current inputs before navigation, then carry the selected published look. */
  const selectImage = useCallback(
    async (image: ReviewImage): Promise<void> => {
      const previous = selectedProfile(currentImage(getState()), getState());
      await saveReview();
      await updateSharedUi({ current_image_id: image.id });
      await carryProfile(image.id, previous?.profile_index);
    },
    [getState, updateSharedUi, saveReview, carryProfile],
  );

  /** Move within the filtered picture list; reaching an end does not wrap. */
  const move = useCallback(
    async (delta: number): Promise<void> => {
      const state = getState();
      const images = filteredImages(state);
      const index = images.findIndex((image) => image.id === state.currentId);
      if (index < 0) return;
      const next = Math.max(0, Math.min(images.length - 1, index + delta));
      if (next !== index) await selectImage(images[next]);
    },
    [getState, selectImage],
  );

  /** Preserve the server's rating-and-advance operation as a single atomic request. */
  const rate = useCallback(
    async (rating: number, advance = true): Promise<void> => {
      const profile = selectedProfile(currentImage(getState()), getState())?.profile_index;
      await saveReview({ rating: Math.max(0, Math.min(5, rating)), advance_after_update: advance });
      if (advance) await carryProfile(getState().currentId, profile);
    },
    [saveReview, getState, carryProfile],
  );

  /** Enable a disabled creative profile when it is selected for viewing. */
  const selectProfile = useCallback(
    (profile: ReviewProfileRender): Promise<void> => {
      const image = currentImage(getState());
      if (!image) return Promise.resolve();
      const patch: Partial<ReviewUpdateRequest> = { selected_profile_index: profile.profile_index };
      if (!isSoocProfile(profile) && profile.enabled === false)
        patch.enabled_profile_indexes = toggleEnabledProfile(image, profile.profile_index);
      return saveReview(patch);
    },
    [getState, saveReview],
  );

  /** Cycle through enabled profiles while retaining the camera rendition. */
  const stepProfile = useCallback(
    async (delta: number): Promise<void> => {
      const state = getState(),
        image = currentImage(state);
      if (!image || profilesAreImplicitOnly(state, image)) return;
      const profiles = image.profiles.filter((profile) => isSoocProfile(profile) || profile.enabled !== false);
      if (!profiles.length) return;
      const index = profiles.findIndex((profile) => profile.profile_index === image.selected_profile_index);
      const next = profiles[(Math.max(0, index) + delta + profiles.length) % profiles.length];
      await saveReview({ selected_profile_index: next.profile_index });
    },
    [getState, saveReview],
  );

  /** Toggle or solo availability without inventing a creative render for SOOC. */
  const toggleProfile = useCallback(
    (profile: ReviewProfileRender, solo = false): Promise<void> => {
      const image = currentImage(getState());
      if (!image) return Promise.resolve();
      if (!solo && isSoocProfile(profile)) return Promise.resolve();
      const enabled = solo
        ? isSoocProfile(profile)
          ? []
          : [profile.profile_index]
        : toggleEnabledProfile(image, profile.profile_index);
      return saveReview({
        enabled_profile_indexes: enabled,
        ...(solo ? { selected_profile_index: profile.profile_index } : {}),
      });
    },
    [getState, saveReview],
  );

  /** Toggle labels in the established color order. */
  const toggleLabel = useCallback(
    (label: ReviewLabel): Promise<void> => {
      const image = currentImage(getState());
      if (!image) return Promise.resolve();
      const labels = imageLabels(image);
      const next =
        label === "none" ? [] : labels.includes(label) ? labels.filter((item) => item !== label) : [...labels, label];
      const ordered: ReviewLabel[] = ["red", "yellow", "green", "blue", "purple"].filter(
        (item): item is Exclude<ReviewLabel, "none"> => next.some((label) => label === item),
      );
      return saveReview({ label: ordered[0] || "none", labels: ordered });
    },
    [getState, saveReview],
  );

  /** Store monochrome filter choices on their individual profile render. */
  const setBwFilter = useCallback(
    (profile: ReviewProfileRender, filter: BwFilter): Promise<void> => {
      const image = currentImage(getState());
      if (!image) return Promise.resolve();
      const filters = profileBwFilters(image).filter((entry) => entry.profile_index !== profile.profile_index);
      if (filter !== "none") filters.push({ profile_index: profile.profile_index, filter });
      return saveReview({ profile_bw_filters: filters });
    },
    [getState, saveReview],
  );

  /** Persist burst expansion through the same ordered write channel. */
  const toggleBurst = useCallback(
    (id: string, expanded: boolean): Promise<void> =>
      enqueue(`api/bursts/${encodeURIComponent(id)}`, { expanded }, "PATCH"),
    [enqueue],
  );

  useEffect(() => {
    const controller = new AbortController();
    let events: EventSource | null = null;
    let retry: number | undefined;
    /** Open SSE only after the initial snapshot exists; cleanup prevents duplicate streams. */
    const start = async (): Promise<void> => {
      try {
        const data = await requestJson<ReviewStateMessage>("api/state", "GET", undefined, controller.signal);
        if (controller.signal.aborted) return;
        if (data) applyMessage(data);
        events = new EventSource(reviewUrl("api/events"));
        events.onopen = (): void => {
          setConnected(true);
          setConnectionError("");
        };
        events.onmessage = (event: MessageEvent<string>): void => {
          setConnected(true);
          setConnectionError("");
          applyMessage(JSON.parse(event.data) as ReviewStateMessage);
        };
        events.addEventListener("keepalive", (event: MessageEvent<string>): void => {
          setConnected(true);
          let title = "Connected";
          try {
            const value: unknown = JSON.parse(event.data);
            if (typeof value === "object" && value !== null) {
              const datetime = "datetime" in value && typeof value.datetime === "string" ? value.datetime : "";
              const version = "version" in value && typeof value.version === "string" ? value.version : "";
              title = `Connected · ${datetime || "keepalive"} · mini-film ${version}`.trim();
            }
          } catch {
            /* Keep the existing generic connected title on malformed keepalives. */
          }
          setKeepalive((previous) => ({ title, tick: previous.tick + 1 }));
        });
        events.onerror = (): void => {
          setConnected(false);
          setConnectionError("Reconnecting...");
          setKeepalive({ title: "Reconnecting", tick: 0 });
        };
      } catch (error) {
        if (controller.signal.aborted) return;
        setConnectionError(`Disconnected: ${errorMessage(error)}`);
        retry = window.setTimeout(() => window.location.reload(), 1500);
      }
    };
    void start();
    return (): void => {
      controller.abort();
      events?.close();
      window.clearTimeout(retry);
    };
  }, [applyMessage]);

  return {
    applyMessage,
    saveReview,
    saveImageReview,
    setDraftReader,
    updateSharedUi,
    move,
    rate,
    selectImage,
    selectProfile,
    stepProfile,
    toggleProfile,
    toggleLabel,
    setBwFilter,
    toggleBurst,
    connected,
    connectionError,
    keepalive,
  };
}
