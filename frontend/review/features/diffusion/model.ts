/**
 * Provider-owned diffusion preserves debounce and polling while isolating preview generations and save barriers.
 */
import { computed, createModel, effect, signal, type ReadonlySignal } from "@preact/signals";
import type { CSSProperties } from "preact";
import { reviewApi, errorMessage, isAbortError } from "../../core/api";
import type { ReviewModelValue } from "../../core/model";
import type {
  DiffusionJobObservation,
  DiffusionSettings,
  DiffusionScope,
  DiffusionPreviewContext,
  DiffusionDetailArea,
  ImageSource,
  ReadonlyData,
  ReviewImageObservation,
  ReviewProfileRenderObservation,
  DiffusionSource,
} from "../../core/types";
import { DIFFUSION_POLL_MS, DIFFUSION_PREVIEW_DEBOUNCE_MS } from "../../core/constants";
import {
  normalizeDiffusionSettings,
  normalizeDiffusionDetailArea,
  diffusionSettingsSignature,
  diffusionJobIsTerminal,
  diffusionAfterSource,
} from "./helpers";
import { selectedProfile, isDirectCompressedImage, isSoocProfile, capitalize } from "../../core/selectors";
import type { ToolSessionActions } from "../../tools/types";

/** View observations derive mutually exclusive loading/save flags from the model's operation state. */
export interface DiffusionState {
  readonly diffusionOpen: boolean;
  readonly diffusionLoading: boolean;
  readonly diffusionSaving: boolean;
  readonly diffusionError: string;
  readonly diffusionErrorKind: "preview" | "save" | null;
  readonly diffusionMessage: string;
  readonly diffusionJob: DiffusionJobObservation | null;
  readonly diffusionBefore: ReadonlyData<ImageSource> | null;
  readonly diffusionPreviewContext: ReadonlyData<DiffusionPreviewContext> | null;
  readonly diffusionImageId: number | null;
  readonly diffusionProfileIndex: number | null;
  readonly diffusionSettings: ReadonlyData<DiffusionSettings> | null;
  readonly diffusionSource: DiffusionSource | null;
  readonly image: ReviewImageObservation | null;
}

/** Save and preview feedback cannot coexist accidentally or display a stale save message during a preview. */
type DiffusionWork =
  | { readonly kind: "preview"; readonly loading: boolean; readonly error: string }
  | { readonly kind: "saving"; readonly loading: boolean; readonly message: string }
  | { readonly kind: "save-failed"; readonly loading: boolean; readonly error: string };

/** An open dialog always owns a complete picture/profile/settings draft; closing destroys that identity. */
interface DiffusionActive {
  readonly kind: "open";
  readonly imageId: number;
  readonly profileIndex: number;
  readonly settings: ReadonlyData<DiffusionSettings>;
  readonly source: DiffusionSource | null;
  readonly job: DiffusionJobObservation | null;
  readonly before: ReadonlyData<ImageSource> | null;
  readonly context: ReadonlyData<DiffusionPreviewContext> | null;
  readonly work: DiffusionWork;
}

/** Dialog lifetime is distinct from preview and save work within an existing draft. */
type DiffusionDialog = { readonly kind: "closed" } | DiffusionActive;

/** Stable editing commands and derived media helpers consumed by diffusion views. */
export interface DiffusionActions {
  openDiffusion(this: void): void;
  closeDiffusion(this: void): void;
  setDiffusionSettings(this: void, patch: Partial<DiffusionSettings>): void;
  requestDiffusionPreview(this: void): void;
  applyDiffusion(this: void, scope: DiffusionScope): Promise<void>;
  resetDiffusion(this: void, scope: DiffusionScope): Promise<void>;
  diffusionBeforeSource(this: void, job: DiffusionJobObservation | null): ReadonlyData<ImageSource>;
  diffusionPreviewContext(
    this: void,
    job: DiffusionJobObservation | null,
  ): ReadonlyData<DiffusionPreviewContext> | null;
  diffusionMediaStyle(
    this: void,
    job: DiffusionJobObservation | null,
    image: ReviewImageObservation | null,
    profile: ReviewProfileRenderObservation | null,
  ): CSSProperties | undefined;
  diffusionStatusText(this: void, job: DiffusionJobObservation | null): string;
}

/** One application-owned instance outlives recoverable dialog views and owns all pending work. */
export interface DiffusionModelValue extends DiffusionActions {
  readonly state: ReadonlySignal<DiffusionState>;
}

/** Retain valid detail crops between matching preview dimensions to avoid shifting comparison frames. */
function previewContext(
  job: DiffusionJobObservation | null,
  remembered: ReadonlyData<DiffusionPreviewContext> | null,
): ReadonlyData<DiffusionPreviewContext> | null {
  const width = Number(job?.preview_width) > 0 ? Math.round(Number(job?.preview_width)) : remembered?.width;
  const height = Number(job?.preview_height) > 0 ? Math.round(Number(job?.preview_height)) : remembered?.height;
  if (!width || !height) return remembered;
  const sameDimensions = remembered?.width === width && remembered.height === height;
  const areas = (job?.detail_areas || [])
    .map((area) => normalizeDiffusionDetailArea(area, width, height))
    .filter((area): area is DiffusionDetailArea => area !== null);
  return {
    width,
    height,
    areas: areas.length ? areas : sameDimensions ? remembered.areas : [],
    focusSource: job?.focus_source || (sameDimensions ? remembered.focusSource : null),
  };
}

/** Own cancellation at action boundaries, not at delayed component-effect cleanup boundaries. */
export const DiffusionModel = createModel(
  (catalog: ReviewModelValue, session: Pick<ToolSessionActions, "applyMessage" | "commit">): DiffusionModelValue => {
    const dialog = signal<DiffusionDialog>({ kind: "closed" });
    let controller: AbortController | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let saveFlight: Promise<void> | null = null;
    let disposed = false;
    const state = computed((): DiffusionState => {
      const current = dialog.value;
      if (current.kind === "closed")
        return {
          diffusionOpen: false,
          diffusionLoading: false,
          diffusionSaving: false,
          diffusionError: "",
          diffusionErrorKind: null,
          diffusionMessage: "",
          diffusionJob: null,
          diffusionBefore: null,
          diffusionPreviewContext: null,
          diffusionImageId: null,
          diffusionProfileIndex: null,
          diffusionSettings: null,
          diffusionSource: null,
          image: null,
        };
      const work = current.work;
      return {
        diffusionOpen: true,
        diffusionImageId: current.imageId,
        diffusionLoading: work.loading,
        diffusionSaving: work.kind === "saving",
        diffusionError: work.kind === "saving" ? "" : work.error,
        diffusionErrorKind:
          work.kind === "save-failed" ? "save" : work.kind === "preview" && work.error ? "preview" : null,
        diffusionMessage: work.kind === "saving" ? work.message : "",
        diffusionJob: current.job,
        diffusionBefore: current.before,
        diffusionPreviewContext: current.context,
        diffusionProfileIndex: current.profileIndex,
        diffusionSettings: current.settings,
        diffusionSource: current.source,
        image: catalog.image(current.imageId).value,
      };
    });

    /** Change only an existing dialog; late callbacks cannot recreate a closed draft. */
    function change(patch: Partial<Omit<DiffusionActive, "kind">>): void {
      const current = dialog.peek();
      if (current.kind === "open") dialog.value = { ...current, ...patch };
    }

    /** Invalidate both a queued barrier continuation and a transport that does not honor its abort signal. */
    function cancelPreview(): void {
      clearTimeout(timer);
      timer = undefined;
      const previous = controller;
      controller = null;
      previous?.abort();
    }

    /** Merge only the owning request's result, preserving the last complete before frame during rendering. */
    function receive(job: DiffusionJobObservation): void {
      const current = dialog.peek();
      if (current.kind !== "open") return;
      change({
        job,
        context: previewContext(job, current.context),
        before:
          job.before_url || job.source_url
            ? current.before?.url && job.status !== "done"
              ? current.before
              : {
                  url: job.before_url || job.source_url || null,
                  updatedAt: job.before_updated_at || job.updated_at || null,
                }
            : current.before,
        work: {
          kind: "preview",
          loading: !diffusionJobIsTerminal(job),
          error: job.status === "failed" ? job.error || "Preview failed" : "",
        },
      });
    }

    /** Debounce user edits, commit their target image, then poll only the resulting preview identity. */
    function schedulePreview(delay: number): void {
      cancelPreview();
      const current = state.peek();
      if (disposed || !current.diffusionOpen || current.diffusionSaving || !current.diffusionSettings) return;
      const pending = new AbortController();
      controller = pending;
      const imageId = current.diffusionImageId;
      const profileIndex = current.diffusionProfileIndex;
      const settings = normalizeDiffusionSettings(current.diffusionSettings);

      /** Ownership survives slow queues and abort-insensitive transports without accepting stale responses. */
      function ownsPreview(): boolean {
        return !disposed && controller === pending && !pending.signal.aborted;
      }

      /** Continue serial polling without replaying the preview-creation mutation on transient GET failures. */
      async function poll(id: number): Promise<void> {
        if (!ownsPreview()) return;
        try {
          const job = await reviewApi.diffusion_get({ params: { job_id: id }, signal: pending.signal });
          if (!ownsPreview()) return;
          receive(job);
          if (diffusionJobIsTerminal(job)) return;
        } catch (error) {
          if (!ownsPreview() || isAbortError(error)) return;
          change({ work: { kind: "preview", loading: state.peek().diffusionLoading, error: errorMessage(error) } });
        }
        timer = setTimeout((): void => {
          void poll(id);
        }, DIFFUSION_POLL_MS);
      }

      /** A canceled preview may finish waiting for local saves, but must never submit its obsolete request. */
      async function start(): Promise<void> {
        if (!ownsPreview()) return;
        change({ job: null, work: { kind: "preview", loading: true, error: "" } });
        try {
          if (imageId === null || profileIndex === null) throw new Error("Select a picture and profile for diffusion");
          const job = await session.commit([imageId], async (): Promise<DiffusionJobObservation | null> => {
            if (!ownsPreview()) return null;
            return reviewApi.diffusion_create({
              body: { image_id: imageId, profile_index: profileIndex, settings },
              signal: pending.signal,
            });
          });
          if (!ownsPreview() || !job) return;
          receive(job);
          if (!diffusionJobIsTerminal(job))
            timer = setTimeout((): void => {
              void poll(job.id);
            }, DIFFUSION_POLL_MS);
        } catch (error) {
          if (!ownsPreview() || isAbortError(error)) return;
          change({ work: { kind: "preview", loading: false, error: errorMessage(error) } });
        }
      }
      timer = setTimeout((): void => {
        void start();
      }, delay);
    }

    /** Capture the selected picture/profile and inherited settings when the dialog opens. */
    function openDiffusion(): void {
      const current = catalog.getState();
      if (disposed || state.peek().diffusionSaving) return;
      const image = current.data?.images.find((candidate) => candidate.id === current.currentId) || null;
      const profile = selectedProfile(image, current);
      if (!image || !profile || isDirectCompressedImage(image) || isSoocProfile(profile)) return;
      cancelPreview();
      dialog.value = {
        kind: "open",
        work: { kind: "preview", loading: true, error: "" },
        job: null,
        before: null,
        context: null,
        imageId: image.id,
        profileIndex: profile.profile_index,
        settings: normalizeDiffusionSettings(profile.diffusion?.settings || profile.diffusion_settings),
        source: profile.diffusion?.source ?? profile.diffusion_source,
      };
      schedulePreview(0);
    }

    /** Save ownership prevents closing; ordinary cancellation immediately drops all preview work and its draft. */
    function closeDiffusion(): void {
      if (state.peek().diffusionSaving) return;
      cancelPreview();
      dialog.value = { kind: "closed" };
    }

    /** Normalize all controls together and ignore equivalent settings before allocating a new generation. */
    function setDiffusionSettings(patch: Partial<DiffusionSettings>): void {
      const current = state.peek();
      if (disposed || !current.diffusionOpen || current.diffusionSaving) return;
      const next = normalizeDiffusionSettings({ ...current.diffusionSettings, ...patch });
      if (diffusionSettingsSignature(next) === diffusionSettingsSignature(current.diffusionSettings)) return;
      cancelPreview();
      change({ settings: next, job: null, work: { kind: "preview", loading: true, error: "" } });
      schedulePreview(DIFFUSION_PREVIEW_DEBOUNCE_MS);
    }

    /** Explicit retries bypass slider debounce without replaying a failed save. */
    function requestDiffusionPreview(): void {
      schedulePreview(0);
    }

    /** Save through the same local command cut as preview creation; failed saves retain the current draft and media. */
    async function performSave(scope: DiffusionScope, reset: boolean, current: DiffusionState): Promise<void> {
      try {
        if (current.diffusionImageId === null || current.diffusionProfileIndex === null)
          throw new Error("Select a picture and profile for diffusion");
        const body = { image_id: current.diffusionImageId, profile_index: current.diffusionProfileIndex, scope };
        const message = await session.commit(scope === "all" ? "all" : [body.image_id], async () =>
          reset
            ? reviewApi.diffusion_reset({ body })
            : reviewApi.diffusion_apply({
                body: { ...body, settings: normalizeDiffusionSettings(current.diffusionSettings) },
              }),
        );
        if (disposed) return;
        cancelPreview();
        dialog.value = { kind: "closed" };
        session.applyMessage(message);
      } catch (error) {
        if (disposed) return;
        change({
          work: {
            kind: "save-failed",
            loading: current.diffusionLoading,
            error: `Could not ${reset ? "reset" : "apply"} diffusion: ${errorMessage(error)}`,
          },
        });
      }
    }

    /** Synchronous flight ownership closes the gap between repeated events before Preact updates disabled controls. */
    function save(scope: DiffusionScope, reset: boolean): Promise<void> {
      if (saveFlight) return saveFlight;
      const current = state.peek();
      if (disposed || !current.diffusionOpen || current.diffusionSaving || (!reset && !current.diffusionSettings))
        return Promise.resolve();
      cancelPreview();
      change({
        work: {
          kind: "saving",
          loading: current.diffusionLoading,
          message: reset
            ? scope === "all"
              ? "Resetting this profile for all pictures"
              : "Resetting current picture"
            : scope === "all"
              ? "Applying to all pictures for this profile"
              : "Applying to current picture",
        },
      });
      const pending = performSave(scope, reset, current).finally((): void => {
        if (saveFlight === pending) saveFlight = null;
      });
      saveFlight = pending;
      return pending;
    }

    /** Hold the before image steady until a new complete preview becomes available. */
    function diffusionBeforeSource(job: DiffusionJobObservation | null): ReadonlyData<ImageSource> {
      const before = state.peek().diffusionBefore;
      if (before?.url && job?.status !== "done") return before;
      const url = job?.before_url || job?.source_url;
      return url
        ? { url, updatedAt: job?.before_updated_at || job?.updated_at || null }
        : before || { url: null, updatedAt: null };
    }

    /** Derive detail geometry without changing state while a component renders. */
    function diffusionPreviewContext(
      job: DiffusionJobObservation | null,
    ): ReadonlyData<DiffusionPreviewContext> | null {
      return previewContext(job, state.peek().diffusionPreviewContext);
    }

    /** Reserve image proportions through preview and full-render transitions. */
    function diffusionMediaStyle(
      job: DiffusionJobObservation | null,
      image: ReviewImageObservation | null,
      profile: ReviewProfileRenderObservation | null,
    ): CSSProperties | undefined {
      const context = diffusionPreviewContext(job);
      const width = Number(context?.width || job?.source_width || profile?.width || image?.source_width);
      const height = Number(context?.height || job?.source_height || profile?.height || image?.source_height);
      return width > 0 && height > 0 && Number.isFinite(width) && Number.isFinite(height)
        ? { aspectRatio: `${width} / ${height}` }
        : undefined;
    }

    /** Describe preview and save progress from the current model snapshot. */
    function diffusionStatusText(job: DiffusionJobObservation | null): string {
      const current = state.peek();
      if (current.diffusionSaving) return current.diffusionMessage || "Saving diffusion settings";
      if (!job) return current.diffusionLoading ? "Preparing preview" : "Preview unavailable";
      if (job.status === "done") return diffusionAfterSource(job).url ? "Preview ready" : "Preview output unavailable";
      if (job.status === "failed") return job.error || "Preview failed";
      if (job.status === "processing") return "Rendering preview";
      if (job.status === "queued") return "Preview queued";
      return current.diffusionLoading ? "Preparing preview" : capitalize(job.status);
    }

    effect(() => (): void => {
      disposed = true;
      cancelPreview();
    });
    return {
      state,
      openDiffusion,
      closeDiffusion,
      setDiffusionSettings,
      requestDiffusionPreview,
      applyDiffusion: (scope): Promise<void> => save(scope, false),
      resetDiffusion: (scope): Promise<void> => save(scope, true),
      diffusionBeforeSource,
      diffusionPreviewContext,
      diffusionMediaStyle,
      diffusionStatusText,
    };
  },
);
