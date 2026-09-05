/**
 * Own the review connection and serialized write queue as a Preact lifecycle hook.
 * Live events, optimistic selection and navigation all use one current state snapshot.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "preact/hooks";
import { useReviewModel } from "../core/context";
import { reviewApi, reviewUrl, errorMessage, decodeStateMessage, decodeKeepalive } from "../core/api";
import { isStatePatch } from "../core/reconcile";
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
import { carriedProfileIndex, reviewRequestBody } from "./review-requests";
import type { ReviewDraftReader } from "./review-requests";
import { createCommandQueue, reviewIntentFields, type ReviewIntent } from "./commands";
import { createFailureTracker, type IntentFailure } from "./failures";

/** Stable callback properties allow features to pass actions without losing their receiver. */
export interface ReviewActions {
  applyMessage: (message: ReviewStateMessage) => void;
  saveReview: (patch?: Partial<ReviewUpdateRequest>) => Promise<void>;
  saveImageReview: (image: ReviewImage, patch: Partial<ReviewUpdateRequest>) => Promise<void>;
  setDraftReader: (reader: ReviewDraftReader | null, flush?: (imageId: number) => Promise<void>) => void;
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
  actions: ReviewActions;
  connected: boolean;
  connectionError: string;
  keepalive: { title: string; tick: number };
  reviewFailures: readonly IntentFailure[];
  recover: (failure: IntentFailure) => Promise<void>;
}

/** Attach one SSE stream and one save queue for the lifetime of the mounted review app. */
export function useReviewSession(): ReviewSession {
  const model = useReviewModel();
  const { update, getState } = model;
  const [queue] = useState(createCommandQueue);
  const [failures] = useState(createFailureTracker);
  const draftReader = useRef<ReviewDraftReader | null>(null);
  const draftFlusher = useRef<((imageId: number) => Promise<void>) | undefined>(undefined);
  const uncertain = useRef(false);
  const [connected, setConnected] = useState<boolean>(false);
  const [connectionError, setConnectionError] = useState<string>("");
  const [keepalive, setKeepalive] = useState<{ title: string; tick: number }>({ title: "", tick: 0 });

  /** Read controlled inputs synchronously when capturing a queued review action. */
  const setDraftReader = useCallback(
    (reader: ReviewDraftReader | null, flush?: (imageId: number) => Promise<void>): void => {
      draftReader.current = reader;
      draftFlusher.current = flush;
    },
    [],
  );

  /** Merge a server acknowledgement without replacing newer optimistic choices. */
  const applyMessage = useCallback(
    (message: ReviewStateMessage): void => {
      const state = getState();
      if (state.data?.version && message.version && state.data.version !== message.version) {
        window.location.reload();
        return;
      }
      if (isStatePatch(message) && !state.data) {
        void reviewApi
          .state({})
          .then((data) => {
            if (data) model.applyMessage(data);
          })
          .catch((error) => setConnectionError(errorMessage(error)));
        return;
      }
      model.applyMessage(message);
    },
    [getState, model],
  );

  /** A read-only resync reduces ambiguous-response damage without claiming cross-client causal ordering. */
  const refresh = useCallback(async (): Promise<void> => {
    applyMessage(await reviewApi.state({}));
    uncertain.current = false;
  }, [applyMessage]);

  /** Serialize writes so faster requests cannot roll back a later user edit. */
  const enqueue = useCallback(
    (request: () => Promise<ReviewStateMessage>): Promise<void> =>
      queue.enqueue(async (): Promise<void> => {
        applyMessage(await request());
      }),
    [applyMessage, queue],
  );

  /** Compile image-scoped intentions only when prior local commands have completed. */
  const saveIntent = useCallback(
    (imageId: number, intent: ReviewIntent, before = Promise.resolve(), tracked = false): Promise<void> => {
      const ticket = tracked ? failures.begin(imageId, intent) : null;
      const command = model.beginCommand(imageId, intent);
      return queue.enqueue(async (): Promise<void> => {
        try {
          await before;
          try {
            if (uncertain.current) await refresh();
            const image = model.getConfirmedState().data?.images.find((item) => item.id === imageId);
            if (!image) throw new Error(`Review picture ${imageId} is no longer available`);
            const body = reviewRequestBody(image, reviewIntentFields(image, intent));
            applyMessage(await reviewApi.review({ body }));
            if (ticket) failures.clear(ticket);
          } catch (error) {
            uncertain.current = true;
            if (ticket) failures.fail(ticket, errorMessage(error));
            await refresh().catch((failure: unknown): void => setConnectionError(errorMessage(failure)));
            throw error;
          }
        } finally {
          model.finishCommand(command);
        }
      });
    },
    [applyMessage, failures, model, queue, refresh],
  );

  /** Capture only explicitly owned fields; untouched fields are read from the latest server image on execution. */
  const saveImageReview = useCallback(
    (image: ReviewImage, patch: Partial<ReviewUpdateRequest>): Promise<void> =>
      saveIntent(image.id, { kind: "fields", fields: patch }),
    [saveIntent],
  );

  /** Keep focused/manual input values with a semantic action without freezing unrelated server fields. */
  const saveCurrentIntent = useCallback(
    (intent: ReviewIntent): Promise<void> => {
      const image = currentImage(getState());
      return image
        ? saveIntent(
            image.id,
            { kind: "with-draft", fields: draftReader.current?.(image) || {}, intent },
            draftFlusher.current?.(image.id),
            true,
          )
        : Promise.resolve();
    },
    [getState, saveIntent],
  );

  /** Save the current image using the latest snapshot, including local retouch drafts. */
  const saveReview = useCallback(
    (patch: Partial<ReviewUpdateRequest> = {}): Promise<void> => saveCurrentIntent({ kind: "fields", fields: patch }),
    [saveCurrentIntent],
  );

  /** Explicit retry always refreshes first, and re-reads drafts instead of replaying captured input text. */
  const recover = useCallback(
    async (failure: IntentFailure): Promise<void> => {
      try {
        await queue.enqueue(refresh);
        if (!failures.current(failure)) return;
        if (!failure.retryable) {
          failures.clear(failure);
          return;
        }
        const image = model.getState().data?.images.find((item) => item.id === failure.imageId);
        const fields = image ? draftReader.current?.(image) || {} : {};
        await saveIntent(
          failure.imageId,
          { kind: "with-draft", fields, intent: failure.intent },
          draftFlusher.current?.(failure.imageId),
          true,
        );
      } catch (error) {
        failures.fail(failure, errorMessage(error));
      }
    },
    [failures, model, queue, refresh, saveIntent],
  );

  /** Share navigation/filter changes with other browsers without losing pending saves. */
  const updateSharedUi = useCallback(
    (patch: Partial<ReviewUiState>): Promise<void> => {
      const label = patch.labels?.find((candidate) => COLOR_LABELS.some((known) => known === candidate));
      if (patch.labels !== undefined) update({ labelFilters: new Set(label ? [label] : []) });
      return enqueue(() => {
        const state = model.getConfirmedState();
        const body: ReviewUiState = {
          current_image_id: patch.current_image_id ?? state.currentId,
          min_rating: patch.min_rating ?? state.data?.ui.min_rating ?? 0,
          labels: patch.labels === undefined ? Array.from(state.labelFilters) : label ? [label] : [],
        };
        return reviewApi.ui({ body });
      });
    },
    [model, update, enqueue],
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
      const target = images[next];
      if (next !== index && target) await selectImage(target);
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
      return saveCurrentIntent({ kind: "profile-selected", profileIndex: profile.profile_index });
    },
    [saveCurrentIntent],
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
      if (next) await saveCurrentIntent({ kind: "profile-selected", profileIndex: next.profile_index });
    },
    [getState, saveCurrentIntent],
  );

  /** Toggle or solo availability without inventing a creative render for SOOC. */
  const toggleProfile = useCallback(
    (profile: ReviewProfileRender, solo = false): Promise<void> => {
      const image = currentImage(getState());
      if (!image) return Promise.resolve();
      if (!solo && isSoocProfile(profile)) return Promise.resolve();
      const current = image.profiles.find((item) => item.profile_index === profile.profile_index) || profile;
      return saveCurrentIntent(
        solo
          ? { kind: "profile-solo", profileIndex: profile.profile_index }
          : { kind: "profile-enabled", profileIndex: profile.profile_index, enabled: current.enabled === false },
      );
    },
    [getState, saveCurrentIntent],
  );

  /** Toggle labels in the established color order. */
  const toggleLabel = useCallback(
    (label: ReviewLabel): Promise<void> => {
      const image = currentImage(getState());
      if (!image) return Promise.resolve();
      return saveCurrentIntent({
        kind: "label",
        label,
        enabled: label !== "none" && !imageLabels(image).includes(label),
      });
    },
    [getState, saveCurrentIntent],
  );

  /** Store monochrome filter choices on their individual profile render. */
  const setBwFilter = useCallback(
    (profile: ReviewProfileRender, filter: BwFilter): Promise<void> => {
      const image = currentImage(getState());
      if (!image) return Promise.resolve();
      return saveCurrentIntent({ kind: "bw-filter", profileIndex: profile.profile_index, filter });
    },
    [getState, saveCurrentIntent],
  );

  /** Persist burst expansion through the same ordered write channel. */
  const toggleBurst = useCallback(
    (id: string, expanded: boolean): Promise<void> =>
      enqueue(() => reviewApi.burst({ params: { burst_id: id }, body: { expanded } })),
    [enqueue],
  );

  useEffect(() => {
    const controller = new AbortController();
    let events: EventSource | null = null;
    let retry: number | undefined;
    /** Open SSE only after the initial snapshot exists; cleanup prevents duplicate streams. */
    const start = async (): Promise<void> => {
      try {
        const data = await reviewApi.state({ signal: controller.signal });
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
          try {
            applyMessage(decodeStateMessage(event.data));
          } catch (error) {
            setConnectionError(errorMessage(error));
            void reviewApi
              .state({ signal: controller.signal })
              .then((snapshot) => {
                if (!controller.signal.aborted) applyMessage(snapshot);
              })
              .catch((failure: unknown) => {
                if (!controller.signal.aborted) setConnectionError(errorMessage(failure));
              });
          }
        };
        events.addEventListener("keepalive", (event: MessageEvent<string>): void => {
          setConnected(true);
          let title = "Connected";
          try {
            const value = decodeKeepalive(event.data);
            title = `Connected · ${value.datetime || "keepalive"} · mini-film ${value.version}`.trim();
          } catch (error) {
            setConnectionError(errorMessage(error));
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

  const actions = useMemo<ReviewActions>(
    () => ({
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
    }),
    [
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
    ],
  );
  return {
    ...actions,
    actions,
    connected,
    connectionError,
    keepalive,
    reviewFailures: failures.failures.value,
    recover,
  };
}
