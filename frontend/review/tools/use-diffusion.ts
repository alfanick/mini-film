/**
 * Reactive diffusion editing ties request cancellation, preview media, and inherited settings to the dialog
 * lifecycle.
 */
import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import type { JSX } from "preact";
import { useReviewContext } from "../core/context";
import { requestJson } from "../core/api";
import type {
  DiffusionJob,
  DiffusionSettings,
  DiffusionScope,
  DiffusionPreviewContext,
  DiffusionDetailArea,
  ImageSource,
  ReviewImage,
  ReviewProfileRender,
  ReviewStateMessage,
} from "../core/types";
import { DIFFUSION_POLL_MS, DIFFUSION_PREVIEW_DEBOUNCE_MS } from "../core/constants";
import {
  normalizeDiffusionSettings,
  normalizeDiffusionDetailArea,
  diffusionSettingsSignature,
  diffusionJobIsTerminal,
  diffusionAfterSource,
} from "./diffusion-helpers";
import { selectedProfile, isDirectCompressedImage, isSoocProfile, capitalize } from "../core/selectors";
import { errorMessage, isAbortError } from "../core/api";
import type { ToolSessionActions } from "./types";

/** Typed editing commands and derived media helpers consumed by diffusion views. */
export interface DiffusionActions {
  openDiffusion(this: void): void;
  closeDiffusion(this: void): void;
  setDiffusionSettings(this: void, patch: Partial<DiffusionSettings>): void;
  requestDiffusionPreview(this: void): void;
  applyDiffusion(this: void, scope: DiffusionScope): Promise<void>;
  resetDiffusion(this: void, scope: DiffusionScope): Promise<void>;
  diffusionBeforeSource(this: void, job: DiffusionJob | null): ImageSource;
  diffusionPreviewContext(this: void, job: DiffusionJob | null): DiffusionPreviewContext | null;
  diffusionMediaStyle(
    this: void,
    job: DiffusionJob | null,
    image: ReviewImage | null,
    profile: ReviewProfileRender | null,
  ): JSX.CSSProperties | undefined;
  diffusionStatusText(this: void, job: DiffusionJob | null): string;
}

/** Carry valid detail crops between previews of matching size so loading does not shift the comparison frames. */
function previewContext(
  job: DiffusionJob | null,
  remembered: DiffusionPreviewContext | null,
): DiffusionPreviewContext | null {
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

/** Expose typed dialog actions while effects own debounce, polling, and cancellation. */
export function useDiffusion(session: ToolSessionActions): DiffusionActions {
  const { state, update, getState } = useReviewContext();
  const [retry, setRetry] = useState(0);
  const [immediate, setImmediate] = useState(true);
  const [previewPaused, setPreviewPaused] = useState(false);
  const previewController = useRef<AbortController | null>(null);
  const sessionRef = useRef(session);
  sessionRef.current = session;

  /** Capture the selected picture/profile and its inherited diffusion settings when the dialog opens. */
  const openDiffusion = useCallback((): void => {
    const current = getState();
    const image = current.data?.images.find((candidate) => candidate.id === current.currentId) || null;
    const profile = selectedProfile(image, current);
    if (!image || !profile || isDirectCompressedImage(image) || isSoocProfile(profile)) return;
    previewController.current?.abort();
    setImmediate(true);
    setPreviewPaused(false);
    update({
      diffusionOpen: true,
      diffusionLoading: true,
      diffusionSaving: false,
      diffusionError: "",
      diffusionErrorKind: null,
      diffusionMessage: "",
      diffusionJob: null,
      diffusionBefore: null,
      diffusionPreviewContext: null,
      diffusionImageId: image.id,
      diffusionProfileIndex: profile.profile_index,
      diffusionSettings: normalizeDiffusionSettings(profile.diffusion?.settings || profile.diffusion_settings),
      diffusionSource: profile.diffusion?.source ?? profile.diffusion_source,
    });
  }, [getState, update]);

  /** Closing the dialog clears its draft and causes preview-effect cleanup to abort pending work. */
  const closeDiffusion = useCallback((): void => {
    if (getState().diffusionSaving) return;
    previewController.current?.abort();
    update({
      diffusionOpen: false,
      diffusionJob: null,
      diffusionBefore: null,
      diffusionPreviewContext: null,
      diffusionImageId: null,
      diffusionProfileIndex: null,
      diffusionSettings: null,
      diffusionSource: null,
      diffusionErrorKind: null,
      diffusionLoading: false,
    });
  }, [getState, update]);

  /** Normalize changes together and debounce only settings that differ from the current draft. */
  const setDiffusionSettings = useCallback(
    (patch: Partial<DiffusionSettings>): void => {
      const current = getState();
      if (!current.diffusionOpen || current.diffusionSaving) return;
      const next = normalizeDiffusionSettings({ ...current.diffusionSettings, ...patch });
      if (diffusionSettingsSignature(next) === diffusionSettingsSignature(current.diffusionSettings)) return;
      // Invalidate old results at the event boundary, before the next passive-effect cleanup can run.
      previewController.current?.abort();
      setImmediate(false);
      setPreviewPaused(false);
      update({
        diffusionSettings: next,
        diffusionJob: null,
        diffusionLoading: true,
        diffusionError: "",
        diffusionErrorKind: null,
        diffusionMessage: "",
      });
    },
    [getState, update],
  );

  /** Retry explicitly without waiting for the slider debounce. */
  const requestDiffusionPreview = useCallback((): void => {
    previewController.current?.abort();
    setImmediate(true);
    setPreviewPaused(false);
    setRetry((value) => value + 1);
  }, []);
  useEffect(() => {
    if (!state.diffusionOpen || state.diffusionSaving || previewPaused || !state.diffusionSettings) return;
    const controller = new AbortController();
    previewController.current = controller;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const imageId = state.diffusionImageId;
    const profileIndex = state.diffusionProfileIndex;
    const settings = normalizeDiffusionSettings(state.diffusionSettings);

    /** Merge only the current effect's response; aborted renders cannot replace newer media or controls. */
    const receive = (job: DiffusionJob | null): void => {
      if (controller.signal.aborted) return;
      update((current) => ({
        diffusionJob: job,
        diffusionPreviewContext: previewContext(job, current.diffusionPreviewContext),
        diffusionBefore:
          job?.before_url || job?.source_url
            ? current.diffusionBefore?.url && job.status !== "done"
              ? current.diffusionBefore
              : { url: job.before_url || job.source_url || null, updatedAt: job.before_updated_at || job.updated_at }
            : current.diffusionBefore,
        diffusionLoading: !diffusionJobIsTerminal(job),
        diffusionError: job?.status === "failed" ? job.error || "Preview failed" : "",
        diffusionErrorKind: job?.status === "failed" ? "preview" : null,
      }));
    };

    /** Poll serially until completion and retry recoverable transport failures at the existing interval. */
    const poll = async (id: number): Promise<void> => {
      if (controller.signal.aborted) return;
      try {
        const job = await requestJson<DiffusionJob>(`api/diffusion/jobs/${id}`, "GET", undefined, controller.signal);
        receive(job);
        if (!controller.signal.aborted && !diffusionJobIsTerminal(job))
          timer = setTimeout(() => {
            void poll(id);
          }, DIFFUSION_POLL_MS);
      } catch (error) {
        if (controller.signal.aborted || isAbortError(error)) return;
        update({ diffusionError: errorMessage(error), diffusionErrorKind: "preview" });
        timer = setTimeout(() => {
          void poll(id);
        }, DIFFUSION_POLL_MS);
      }
    };

    /** Create a preview for the current settings, then poll the returned job identity. */
    const start = async (): Promise<void> => {
      if (controller.signal.aborted) return;
      update({
        diffusionLoading: true,
        diffusionError: "",
        diffusionErrorKind: null,
        diffusionMessage: "",
        diffusionJob: null,
      });
      try {
        const job = await requestJson<DiffusionJob>(
          "api/diffusion/jobs",
          "POST",
          { image_id: imageId, profile_index: profileIndex, settings },
          controller.signal,
        );
        if (controller.signal.aborted) return;
        if (!job) throw new Error("preview job returned no data");
        receive(job);
        if (!diffusionJobIsTerminal(job))
          timer = setTimeout(() => {
            void poll(job.id);
          }, DIFFUSION_POLL_MS);
      } catch (error) {
        if (controller.signal.aborted || isAbortError(error)) return;
        update({
          diffusionLoading: false,
          diffusionError: errorMessage(error),
          diffusionErrorKind: "preview",
        });
      }
    };
    timer = setTimeout(
      () => {
        void start();
      },
      immediate ? 0 : DIFFUSION_PREVIEW_DEBOUNCE_MS,
    );
    return () => {
      clearTimeout(timer);
      controller.abort();
      if (previewController.current === controller) previewController.current = null;
    };
  }, [
    state.diffusionOpen,
    state.diffusionSaving,
    state.diffusionImageId,
    state.diffusionProfileIndex,
    state.diffusionSettings,
    previewPaused,
    retry,
    immediate,
    update,
  ]);

  /** Save or reset the chosen inheritance scope; successful empty resets are accepted. */
  const save = useCallback(
    async (scope: DiffusionScope, reset: boolean): Promise<void> => {
      const current = getState();
      if (!current.diffusionOpen || current.diffusionSaving || (!reset && !current.diffusionSettings)) return;
      previewController.current?.abort();
      // A failed save keeps the current preview and error until the user edits or explicitly retries.
      setPreviewPaused(true);
      update({
        diffusionSaving: true,
        diffusionError: "",
        diffusionErrorKind: null,
        diffusionMessage: reset
          ? scope === "all"
            ? "Resetting this profile for all pictures"
            : "Resetting current picture"
          : scope === "all"
            ? "Applying to all pictures for this profile"
            : "Applying to current picture",
      });
      try {
        const body = {
          image_id: current.diffusionImageId,
          profile_index: current.diffusionProfileIndex,
          scope,
          ...(reset ? {} : { settings: normalizeDiffusionSettings(current.diffusionSettings) }),
        };
        const message = await requestJson<ReviewStateMessage>(
          "api/diffusion/settings",
          reset ? "DELETE" : "POST",
          body,
        );
        update({ diffusionSaving: false });
        closeDiffusion();
        if (message) sessionRef.current.applyMessage(message);
      } catch (error) {
        update({
          diffusionSaving: false,
          diffusionError: `Could not ${reset ? "reset" : "apply"} diffusion: ${errorMessage(error)}`,
          diffusionErrorKind: "save",
          diffusionMessage: "",
        });
      }
    },
    [getState, update, closeDiffusion],
  );

  /** Hold the before image steady until a new complete preview becomes available. */
  const diffusionBeforeSource = (job: DiffusionJob | null): ImageSource => {
    if (state.diffusionBefore?.url && job?.status !== "done") return state.diffusionBefore;
    const url = job?.before_url || job?.source_url;
    return url
      ? { url, updatedAt: job?.before_updated_at || job?.updated_at }
      : state.diffusionBefore || { url: null, updatedAt: null };
  };
  /** Derive detail geometry without updating state during rendering. */
  const diffusionPreviewContext = (job: DiffusionJob | null): DiffusionPreviewContext | null =>
    previewContext(job, state.diffusionPreviewContext);
  /** Reserve the correct image proportions through preview and full-render transitions. */
  const diffusionMediaStyle = (
    job: DiffusionJob | null,
    image: ReviewImage | null,
    profile: ReviewProfileRender | null,
  ): JSX.CSSProperties | undefined => {
    const context = diffusionPreviewContext(job);
    const width = Number(context?.width || job?.source_width || profile?.width || image?.source_width);
    const height = Number(context?.height || job?.source_height || profile?.height || image?.source_height);
    return width > 0 && height > 0 && Number.isFinite(width) && Number.isFinite(height)
      ? { aspectRatio: `${width} / ${height}` }
      : undefined;
  };
  /** Describe preview and save progress directly from reactive state. */
  const diffusionStatusText = (job: DiffusionJob | null): string => {
    if (state.diffusionSaving) return state.diffusionMessage || "Saving diffusion settings";
    if (!job) return state.diffusionLoading ? "Preparing preview" : "Preview unavailable";
    if (job.status === "done") return diffusionAfterSource(job).url ? "Preview ready" : "Preview output unavailable";
    if (job.status === "failed") return job.error || "Preview failed";
    if (job.status === "processing") return "Rendering preview";
    if (job.status === "queued") return "Preview queued";
    return state.diffusionLoading ? "Preparing preview" : capitalize(job.status);
  };
  return {
    openDiffusion,
    closeDiffusion,
    setDiffusionSettings,
    requestDiffusionPreview,
    applyDiffusion: (scope) => save(scope, false),
    resetDiffusion: (scope) => save(scope, true),
    diffusionBeforeSource,
    diffusionPreviewContext,
    diffusionMediaStyle,
    diffusionStatusText,
  };
}
