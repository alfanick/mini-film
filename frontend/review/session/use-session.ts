/**
 * Own the review connection and serialized write queue as a Preact lifecycle hook.
 * Live events, optimistic selection and navigation all use one current state snapshot.
 */
import { createModel, effect, signal, type ReadonlySignal } from "@preact/signals";
import type { ReviewModelValue } from "../core/model";

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
  ReviewImageObservation as ReviewImage,
  ReviewLabel,
  ReviewProfileRenderObservation as ReviewProfileRender,
  ReviewStateMessage,
  ReviewUiState,
} from "../core/types";
import { COLOR_LABELS } from "../core/constants";
import { carriedProfileIndex, reviewRequestBody } from "./review-requests";
import type { ReviewDraftReader } from "./review-requests";
import { createCommandQueue, reviewIntentFields, type ReviewIntent, type ReviewFields } from "./commands";
import { createFailureTracker, type IntentFailure } from "./failures";
import type { DraftModelValue, DraftSaveOptions } from "./draft-model";
import { invalidatedOutputs, type CommitContext, type CommitScope } from "./barriers";

/** Stable callback properties allow features to pass actions without losing their receiver. */
export interface ReviewActions {
  applyMessage: (message: ReviewStateMessage) => void;
  saveReview: (patch?: ReviewFields) => Promise<void>;
  saveImageReview: (image: ReviewImage, patch: ReviewFields, options?: DraftSaveOptions) => Promise<void>;
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
  readonly connected: ReadonlySignal<boolean>;
  readonly connectionError: ReadonlySignal<string>;
  readonly keepalive: ReadonlySignal<{ readonly title: string; readonly tick: number }>;
  readonly reviewFailures: ReadonlySignal<readonly IntentFailure[]>;
  recover: (failure: IntentFailure) => Promise<void>;
  refresh: () => Promise<void>;
  attachDrafts: (drafts: DraftModelValue) => void;
  stop: () => void;
  commit: <T>(scope: CommitScope, operation: (context: CommitContext) => Promise<T>) => Promise<T>;
}

/** Tests inject endpoint-bound operations; production keeps the generated validated transport and one SSE stream. */
export interface SessionPorts {
  readonly api?: Pick<typeof reviewApi, "state" | "review" | "ui" | "burst">;
  readonly connect?: boolean;
}

/** Attach one SSE stream and one save queue for the lifetime of the mounted review app. */
export const ReviewSessionModel = createModel((model: ReviewModelValue, ports: SessionPorts = {}): ReviewSession => {
  const api = ports.api || reviewApi;
  const { update, getState } = model;
  const queue = createCommandQueue();
  const failures = createFailureTracker();
  const draftReader: { current: ReviewDraftReader | null } = { current: null };
  const draftFlusher: { current: ((imageId: number) => Promise<void>) | undefined } = { current: undefined };
  const uncertain = { current: false };
  let disposed = false;
  let stopConnection: (() => void) | null = null;
  let drafts: DraftModelValue | null = null;
  let sequence = 0;
  const pendingCommands = new Set<number>();
  const barriers = new Set<{ pending: ReadonlySet<number>; start: number; cut: number; affected: Set<string> }>();
  const connected = signal(false);
  /** Publish connection status without invalidating unrelated catalog observations. */
  const setConnected = (value: boolean): void => {
    connected.value = value;
  };
  const connectionError = signal("");
  /** Keep stream failures observable until a later connection update clears them. */
  const setConnectionError = (value: string): void => {
    connectionError.value = value;
  };
  const keepalive = signal({ title: "", tick: 0 });
  /** Update heartbeat metadata without subscribing the session model to its own status. */
  const setKeepalive = (
    value:
      | { title: string; tick: number }
      | ((previous: { title: string; tick: number }) => { title: string; tick: number }),
  ): void => {
    keepalive.value = typeof value === "function" ? value(keepalive.peek()) : value;
  };

  /** Read controlled inputs synchronously when capturing a queued review action. */
  const setDraftReader = (reader: ReviewDraftReader | null, flush?: (imageId: number) => Promise<void>): void => {
    draftReader.current = reader;
    draftFlusher.current = flush;
  };

  /** Merge a server acknowledgement without replacing newer optimistic choices. */
  const applyMessage = (message: ReviewStateMessage): void => {
    if (disposed) return;
    const state = getState();
    if (state.data?.version && message.version && state.data.version !== message.version) {
      window.location.reload();
      return;
    }
    if (isStatePatch(message) && !state.data) {
      void api
        .state({})
        .then((data) => {
          if (data) model.applyMessage(data);
        })
        .catch((error) => setConnectionError(errorMessage(error)));
      return;
    }
    model.applyMessage(message);
  };

  /** A read-only resync reduces ambiguous-response damage without claiming cross-client causal ordering. */
  const refresh = async (signal?: AbortSignal): Promise<void> => {
    const snapshot = await api.state(signal ? { signal } : {});
    if (signal?.aborted) throw new Error("Review state check was cancelled");
    applyMessage(snapshot);
    uncertain.current = false;
  };

  /** Serialize writes so faster requests cannot roll back a later user edit. */
  const enqueue = (request: () => Promise<ReviewStateMessage>): Promise<void> =>
    queue.enqueue(async (): Promise<void> => {
      if (disposed) throw new Error("Review session is no longer active");
      applyMessage(await request());
    });

  /** Compile image-scoped intentions only when prior local commands have completed. */
  const saveIntent = (
    imageId: number,
    intent: ReviewIntent,
    before = Promise.resolve(),
    tracked = false,
    options: DraftSaveOptions = {},
  ): Promise<void> => {
    const captured = structuredClone(intent);
    const ticket = tracked ? failures.begin(imageId, captured) : null;
    const command = model.beginCommand(imageId, captured);
    const ownSequence = ++sequence;
    pendingCommands.add(ownSequence);
    return queue.enqueue(async (): Promise<void> => {
      try {
        if (disposed) throw new Error("Review session is no longer active");
        await before;
        try {
          if (uncertain.current) await refresh();
          const image = model.confirmedImage(imageId);
          if (!image) throw new Error(`Review picture ${imageId} is no longer available`);
          const body = reviewRequestBody(image, reviewIntentFields(image, captured));
          const keepalive = options.keepalive === true && new Blob([JSON.stringify(body)]).size <= 65_536;
          applyMessage(await api.review({ body, ...(keepalive ? { keepalive: true } : {}) }));
          const after = model.confirmedImage(imageId);
          if (after)
            for (const barrier of barriers) {
              if (barrier.pending.has(ownSequence) || (ownSequence > barrier.start && ownSequence <= barrier.cut))
                for (const output of invalidatedOutputs(image, after)) barrier.affected.add(output);
            }
          if (ticket) failures.clear(ticket);
        } catch (error) {
          uncertain.current = true;
          if (ticket) failures.fail(ticket, errorMessage(error));
          await refresh().catch((failure: unknown): void => setConnectionError(errorMessage(failure)));
          throw error;
        }
      } finally {
        pendingCommands.delete(ownSequence);
        model.finishCommand(command);
      }
    });
  };

  /** Capture only explicitly owned fields; untouched fields are read from the latest server image on execution. */
  const saveImageReview = (image: ReviewImage, patch: ReviewFields, options: DraftSaveOptions = {}): Promise<void> =>
    saveIntent(image.id, { kind: "fields", fields: patch }, Promise.resolve(), false, options);

  /** Reserve the complete fixed draft cut before yielding; later user writes cannot overtake a waiting job. */
  function commit<T>(scope: CommitScope, operation: (context: CommitContext) => Promise<T>): Promise<T> {
    const selected = scope === "all" ? undefined : [...scope];
    const barrier = { pending: new Set(pendingCommands), start: sequence, cut: sequence, affected: new Set<string>() };
    barriers.add(barrier);
    const flushed = drafts?.flushOwned(selected) || Promise.resolve();
    const completion = flushed.then(
      () => null,
      (error: unknown) => error,
    );
    barrier.cut = sequence;
    return queue.enqueue(async (): Promise<T> => {
      try {
        if (disposed) throw new Error("Review session is no longer active");
        const error = await completion;
        if (error !== null) throw error instanceof Error ? error : new Error(errorMessage(error));
        const owns = (id: number): boolean => selected === undefined || selected.includes(id);
        const failure = drafts?.errors.peek().find((item) => owns(item.imageId));
        const commandFailure = failures.failures.peek().find((item) => owns(item.imageId));
        if (failure || commandFailure) throw new Error(failure?.message || commandFailure?.message);
        if (uncertain.current) await refresh();
        return await operation({
          affectedOutputs: barrier.affected,
          snapshot: model.getConfirmedState(),
          refresh: async (signal?: AbortSignal): Promise<ReturnType<ReviewModelValue["getConfirmedState"]>> => {
            await refresh(signal);
            return model.getConfirmedState();
          },
        });
      } finally {
        barriers.delete(barrier);
      }
    });
  }

  /** Keep focused/manual input values with a semantic action without freezing unrelated server fields. */
  const saveCurrentIntent = (intent: ReviewIntent): Promise<void> => {
    const image = currentImage(getState());
    return image
      ? saveIntent(
          image.id,
          { kind: "with-draft", fields: draftReader.current?.(image) || {}, intent },
          draftFlusher.current?.(image.id),
          true,
        )
      : Promise.resolve();
  };

  /** Save the current image using the latest snapshot, including local retouch drafts. */
  const saveReview = (patch: ReviewFields = {}): Promise<void> => saveCurrentIntent({ kind: "fields", fields: patch });

  /** Explicit retry always refreshes first, and re-reads drafts instead of replaying captured input text. */
  const recover = async (failure: IntentFailure): Promise<void> => {
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
  };

  /** Share navigation/filter changes with other browsers without losing pending saves. */
  const updateSharedUi = (patch: Partial<ReviewUiState>): Promise<void> => {
    const label = patch.labels?.find((candidate) => COLOR_LABELS.some((known) => known === candidate));
    if (patch.labels !== undefined) update({ labelFilters: new Set(label ? [label] : []) });
    return enqueue(() => {
      const state = model.getConfirmedState();
      const body: ReviewUiState = {
        current_image_id: patch.current_image_id ?? state.currentId,
        min_rating: patch.min_rating ?? state.data?.ui.min_rating ?? 0,
        labels: patch.labels === undefined ? Array.from(state.labelFilters) : label ? [label] : [],
      };
      return api.ui({ body });
    });
  };

  /** Apply the original published-profile fallback after the server changes pictures. */
  const carryProfile = async (imageId: number | null, profileIndex: number | undefined): Promise<void> => {
    const image = getState().data?.images.find((candidate) => candidate.id === imageId);
    if (!image) return;
    const selected = carriedProfileIndex(image, profileIndex);
    if (selected !== undefined) await saveImageReview(image, { selected_profile_index: selected });
  };

  /** Save current inputs before navigation, then carry the selected published look. */
  const selectImage = async (image: ReviewImage): Promise<void> => {
    const previous = selectedProfile(currentImage(getState()), getState());
    await saveReview();
    await updateSharedUi({ current_image_id: image.id });
    await carryProfile(image.id, previous?.profile_index);
  };

  /** Move within the filtered picture list; reaching an end does not wrap. */
  const move = async (delta: number): Promise<void> => {
    const state = getState();
    const images = filteredImages(state);
    const index = images.findIndex((image) => image.id === state.currentId);
    if (index < 0) return;
    const next = Math.max(0, Math.min(images.length - 1, index + delta));
    const target = images[next];
    if (next !== index && target) await selectImage(target);
  };

  /** Preserve the server's rating-and-advance operation as a single atomic request. */
  const rate = async (rating: number, advance = true): Promise<void> => {
    const profile = selectedProfile(currentImage(getState()), getState())?.profile_index;
    await saveReview({ rating: Math.max(0, Math.min(5, rating)), advance_after_update: advance });
    if (advance) await carryProfile(getState().currentId, profile);
  };

  /** Enable a disabled creative profile when it is selected for viewing. */
  const selectProfile = (profile: ReviewProfileRender): Promise<void> => {
    return saveCurrentIntent({ kind: "profile-selected", profileIndex: profile.profile_index });
  };

  /** Cycle through enabled profiles while retaining the camera rendition. */
  const stepProfile = async (delta: number): Promise<void> => {
    const state = getState(),
      image = currentImage(state);
    if (!image || profilesAreImplicitOnly(state, image)) return;
    const profiles = image.profiles.filter((profile) => isSoocProfile(profile) || profile.enabled !== false);
    if (!profiles.length) return;
    const index = profiles.findIndex((profile) => profile.profile_index === image.selected_profile_index);
    const next = profiles[(Math.max(0, index) + delta + profiles.length) % profiles.length];
    if (next) await saveCurrentIntent({ kind: "profile-selected", profileIndex: next.profile_index });
  };

  /** Toggle or solo availability without inventing a creative render for SOOC. */
  const toggleProfile = (profile: ReviewProfileRender, solo = false): Promise<void> => {
    const image = currentImage(getState());
    if (!image) return Promise.resolve();
    if (!solo && isSoocProfile(profile)) return Promise.resolve();
    const current = image.profiles.find((item) => item.profile_index === profile.profile_index) || profile;
    return saveCurrentIntent(
      solo
        ? { kind: "profile-solo", profileIndex: profile.profile_index }
        : { kind: "profile-enabled", profileIndex: profile.profile_index, enabled: current.enabled === false },
    );
  };

  /** Toggle labels in the established color order. */
  const toggleLabel = (label: ReviewLabel): Promise<void> => {
    const image = currentImage(getState());
    if (!image) return Promise.resolve();
    return saveCurrentIntent({
      kind: "label",
      label,
      enabled: label !== "none" && !imageLabels(image).includes(label),
    });
  };

  /** Store monochrome filter choices on their individual profile render. */
  const setBwFilter = (profile: ReviewProfileRender, filter: BwFilter): Promise<void> => {
    const image = currentImage(getState());
    if (!image) return Promise.resolve();
    return saveCurrentIntent({ kind: "bw-filter", profileIndex: profile.profile_index, filter });
  };

  /** Persist burst expansion through the same ordered write channel. */
  const toggleBurst = (id: string, expanded: boolean): Promise<void> =>
    enqueue(() => api.burst({ params: { burst_id: id }, body: { expanded } }));

  effect(() => {
    if (ports.connect === false) return;
    const controller = new AbortController();
    let events: EventSource | null = null;
    let retry: number | undefined;
    /** Open SSE only after the initial snapshot exists; cleanup prevents duplicate streams. */
    const start = async (): Promise<void> => {
      try {
        const data = await api.state({ signal: controller.signal });
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
            void api
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
    stopConnection = (): void => {
      controller.abort();
      events?.close();
      window.clearTimeout(retry);
    };
    return stopConnection;
  });

  const actions: ReviewActions = (() => ({
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
  }))();
  return {
    ...actions,
    actions,
    connected,
    connectionError,
    keepalive,
    reviewFailures: failures.failures,
    recover,
    refresh: (): Promise<void> => queue.enqueue(refresh),
    attachDrafts: (value: DraftModelValue): void => {
      drafts = value;
    },
    commit,
    stop: (): void => {
      disposed = true;
      stopConnection?.();
    },
  };
});
