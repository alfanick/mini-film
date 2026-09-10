/**
 * Controlled publish drafts preserve text input and optional export fields while server jobs supply reactive
 * progress.
 */
import { computed, createModel, signal, type ReadonlySignal } from "@preact/signals";
import type { ReviewModelValue } from "../../core/model";
import { waitForPublishOutputs, waitForPublishSnapshot } from "./barrier";
import type { CommitContext } from "../../session/barriers";
import { reviewApi, errorMessage } from "../../core/api";
import { imageLabels, isDirectCompressedImage, publishProfileIndexes } from "../../core/selectors";
import { COLOR_LABELS } from "../../core/constants";
import type {
  PublishRequest,
  ReadonlyData,
  ReviewLabel,
  ReviewPublishDefaults,
  ReviewPublishJobObservation as ReviewPublishJob,
} from "../../core/types";
import { numberOrNull, splitPublishTags } from "./helpers";
import type { ToolSessionActions } from "../../tools/types";

/** Raw form values stay textual until submission so partially typed edits remain intact. */
export interface PublishDraft {
  album: string;
  minRating: string;
  labels: ReviewLabel[];
  tags: string;
  mainProfileOnly: boolean;
  outputFormat: string;
  grainEngine: string;
  normalizeGrain: boolean;
  normalizeGrainMpix: string;
  sizeMode: string;
  longEdge: string;
  maxWidth: string;
  maxHeight: string;
  resize: string;
  jpgQuality: string;
  jpegSubsampling: string;
  progressive: boolean;
  stripMetadata: boolean;
  gallery: string;
  galleryColumns: string;
  galleryThumbnailLongEdge: string;
}
/** Controlled form values, derived selection counts, and publish lifecycle actions. */
export interface PublishActions {
  readonly publishOpen: ReadonlySignal<boolean>;
  readonly publishForm: ReadonlySignal<ReadonlyData<PublishDraft>>;
  readonly publishSubmitting: ReadonlySignal<boolean>;
  readonly publishError: ReadonlySignal<string>;
  readonly publishRecovery: ReadonlySignal<boolean>;
  readonly publishJob: ReadonlySignal<ReviewPublishJob | null>;
  readonly publishRerender: ReadonlySignal<boolean>;
  readonly publishStats: ReadonlySignal<{ readonly pictures: number; readonly outputs: number }>;
  togglePublishWizard(this: void, force?: boolean): void;
  setPublishField<K extends keyof PublishDraft>(this: void, field: K, value: PublishDraft[K]): void;
  togglePublishLabel(this: void, label: ReviewLabel, checked: boolean): void;
  submitPublish(this: void): Promise<void>;
}

/** Populate the same daemon defaults every time the user opens the publish wizard. */
function defaultDraft(defaults: Partial<ReviewPublishDefaults>, minRating: number): PublishDraft {
  return {
    album: defaults.album || "published",
    minRating: String(minRating),
    labels: [],
    tags: "",
    mainProfileOnly: false,
    outputFormat: defaults.output_format || "jpg",
    grainEngine: defaults.grain_engine || "legacy",
    normalizeGrain: defaults.normalize_grain_mpix !== null,
    normalizeGrainMpix: String(defaults.normalize_grain_mpix ?? 12),
    sizeMode: defaults.resize
      ? "geometry"
      : defaults.long_edge
        ? "long-edge"
        : defaults.max_width || defaults.max_height
          ? "bounds"
          : "original",
    longEdge: defaults.long_edge ? String(defaults.long_edge) : "",
    maxWidth: defaults.max_width ? String(defaults.max_width) : "",
    maxHeight: defaults.max_height ? String(defaults.max_height) : "",
    resize: defaults.resize || "",
    jpgQuality: String(defaults.jpg_quality || 95),
    jpegSubsampling: defaults.jpeg_subsampling || "s444",
    progressive: Boolean(defaults.progressive_jpeg),
    stripMetadata: Boolean(defaults.strip_metadata),
    gallery: defaults.gallery || "none",
    galleryColumns: String(defaults.gallery_columns || 4),
    galleryThumbnailLongEdge: String(defaults.gallery_thumbnail_long_edge || 1024),
  };
}

/** Convert strings only at the request boundary and omit dimensions belonging to inactive size modes. */
export function publishBody(form: ReadonlyData<PublishDraft>): PublishRequest {
  return {
    album: form.album.trim() || "published",
    min_rating: Number(form.minRating || 0),
    labels: [...form.labels],
    tags: splitPublishTags(form.tags),
    main_profile_only: form.mainProfileOnly,
    output_format: form.outputFormat,
    grain_engine: form.grainEngine,
    normalize_grain: form.normalizeGrain,
    normalize_grain_mpix: numberOrNull(form.normalizeGrainMpix),
    gallery: form.gallery,
    size_mode: form.sizeMode,
    jpg_quality: Number(form.jpgQuality || 95),
    jpeg_subsampling: form.jpegSubsampling,
    strip_metadata: form.stripMetadata,
    progressive_jpeg: form.progressive,
    gallery_columns: Number(form.galleryColumns || 4),
    gallery_thumbnail_long_edge: Number(form.galleryThumbnailLongEdge || 1024),
    ...(form.sizeMode === "long-edge" ? { long_edge: numberOrNull(form.longEdge) } : {}),
    ...(form.sizeMode === "bounds"
      ? { max_width: numberOrNull(form.maxWidth), max_height: numberOrNull(form.maxHeight) }
      : {}),
    ...(form.sizeMode === "geometry" ? { resize: form.resize.trim() } : {}),
  };
}

/** Compare only render-affecting options; gallery and selection changes can still reuse reviewed outputs. */
function wouldRerender(form: PublishDraft, defaults: Partial<ReviewPublishDefaults>): boolean {
  const body = publishBody(form);
  const baseline = publishBody(defaultDraft(defaults, 0));
  return (
    body.output_format !== baseline.output_format ||
    body.grain_engine !== baseline.grain_engine ||
    body.normalize_grain !== baseline.normalize_grain ||
    Boolean(body.normalize_grain && body.normalize_grain_mpix !== baseline.normalize_grain_mpix) ||
    body.size_mode !== baseline.size_mode ||
    body.jpg_quality !== baseline.jpg_quality ||
    body.jpeg_subsampling !== baseline.jpeg_subsampling ||
    body.strip_metadata !== baseline.strip_metadata ||
    body.progressive_jpeg !== baseline.progressive_jpeg ||
    (body.resize || "") !== (defaults.resize || "") ||
    (body.long_edge || null) !== (defaults.long_edge || null) ||
    (body.max_width || null) !== (defaults.max_width || null) ||
    (body.max_height || null) !== (defaults.max_height || null)
  );
}

/** Manage the controlled form and derive selection counts/progress without querying rendered form fields. */
export const PublishModel = createModel((catalog: ReviewModelValue, session: ToolSessionActions): PublishActions => {
  const getState = catalog.getState;
  const state = getState();
  const publishOpen = signal(false);
  /** Change dialog visibility without canceling an in-flight publish operation. */
  const setOpen = (value: boolean): void => {
    publishOpen.value = value;
  };
  const publishForm = signal<PublishDraft>(
    defaultDraft(state.data?.publish_defaults || {}, state.data?.ui.min_rating || 0),
  );
  /** Copy controlled form input so caller-owned label arrays cannot mutate the draft. */
  const setForm = (value: PublishDraft | ((previous: PublishDraft) => PublishDraft)): void => {
    const next = typeof value === "function" ? value(publishForm.peek()) : value;
    publishForm.value = { ...next, labels: [...next.labels] };
  };
  const publishSubmitting = signal(false);
  /** Reflect submission progress independently of whether the dialog is visible. */
  const setSubmitting = (value: boolean): void => {
    publishSubmitting.value = value;
  };
  const publishError = signal("");
  /** Retain actionable publish errors in the durable feature model. */
  const setError = (value: string): void => {
    publishError.value = value;
  };
  const submitting = { current: false };
  const publishRecovery = signal(false);
  let recovery: (() => Promise<void>) | null = null;
  const ownJobId = signal<number | null>(null);

  /** Explicit read-only recovery may resume edits, but never infers a created job or retries its POST. */
  function holdUnknown(context: CommitContext, jobId: number | null, message: string): Promise<void> {
    publishRecovery.value = true;
    setError(
      `Publish outcome is unknown: ${message}. Check state before resuming edits; do not resubmit automatically.`,
    );
    return new Promise<void>((resolve) => {
      recovery = async (): Promise<void> => {
        const controller = new AbortController();
        const timer = setTimeout((): void => controller.abort(), 30_000);
        try {
          const snapshot = await context.refresh(controller.signal);
          const jobs = snapshot.data?.publish_jobs || [];
          const own = jobId === null ? null : jobs.find((job) => job.id === jobId);
          if (
            (jobId !== null && !own) ||
            (own?.status === "running" && own.step === "starting") ||
            (jobId === null && jobs.some((job) => job.status === "running" && job.step === "starting"))
          ) {
            setError("Publish startup is still uncertain. Edits remain local; check state again shortly.");
            return;
          }
          // With a lost creation acknowledgement this is an explicit best-effort release, not job correlation proof.
          recovery = null;
          publishRecovery.value = false;
          setError(
            jobId === null
              ? "Publish outcome unknown. Editing resumed after checking state; inspect jobs before starting another."
              : "Publish state checked; local editing resumed.",
          );
          resolve();
        } catch (error) {
          setError(`State check failed: ${errorMessage(error)}. Edits remain local.`);
        } finally {
          clearTimeout(timer);
          controller.abort();
        }
      };
    });
  }

  /** Reinitialize controls on each opening, matching the daemon-default publish workflow. */
  const togglePublishWizard = (force?: boolean): void => {
    const show = force ?? !publishOpen.peek();
    if (show && !submitting.current) {
      const current = getState();
      setForm(defaultDraft(current.data?.publish_defaults || {}, current.data?.ui.min_rating || 0));
      setError("");
    }
    setOpen(show);
  };

  /** Keep a field's declared value type when replacing part of the controlled draft. */
  const setPublishField = <K extends keyof PublishDraft>(field: K, value: PublishDraft[K]): void =>
    setForm((current) => ({ ...current, [field]: value }));

  /** Store selected color labels in the same order as the rendered checkbox list. */
  const togglePublishLabel = (label: ReviewLabel, checked: boolean): void =>
    setForm((current) => ({
      ...current,
      labels: COLOR_LABELS.filter((candidate) => (candidate === label ? checked : current.labels.includes(candidate))),
    }));

  /** Start publishing with the current draft and merge the returned server job state. */
  const submitPublish = async (): Promise<void> => {
    if (recovery) {
      await recovery();
      return;
    }
    if (submitting.current) return;
    submitting.current = true;
    setSubmitting(true);
    setError("");
    try {
      const body = publishBody(publishForm.peek());
      await session.commit("all", async (context): Promise<void> => {
        setError("Saving edits and waiting for affected pictures to finish rendering...");
        await waitForPublishOutputs(context, body);
        setError("Starting publish job...");
        let jobId: number | null = null;
        try {
          const response = await reviewApi.publish({ body });
          session.applyMessage(response);
          if (!("created_job_id" in response) || typeof response.created_job_id !== "number")
            throw new Error("Publish creation acknowledgement did not identify its job");
          jobId = response.created_job_id;
          ownJobId.value = jobId;
          await waitForPublishSnapshot(context, jobId);
          setError("");
        } catch (error) {
          await holdUnknown(context, jobId, errorMessage(error));
        }
      });
    } catch (error) {
      setError(`Publish failed: ${errorMessage(error)}`);
    } finally {
      submitting.current = false;
      setSubmitting(false);
    }
  };
  const publishStats = computed(() => {
    const body = publishBody(publishForm.value);
    const labels = new Set(body.labels);
    const tags = new Set(body.tags.map((tag) => tag.toLowerCase()));
    const totals = { pictures: 0, outputs: 0 };
    if (!publishOpen.value) return totals;
    for (const image of catalog.images.value) {
      if (
        image.rating < body.min_rating ||
        (labels.size > 0 && imageLabels(image).every((label) => !labels.has(label))) ||
        (tags.size > 0 && !image.tags.some((tag) => tags.has(tag.toLowerCase())))
      )
        continue;
      totals.pictures += 1;
      totals.outputs += isDirectCompressedImage(image)
        ? 1
        : body.main_profile_only
          ? 1
          : publishProfileIndexes(image).length;
    }
    return totals;
  });
  const publishJob = computed((): ReviewPublishJob | null => {
    const jobs = catalog.catalogField("publish_jobs").value || [];
    const id = ownJobId.value;
    return id === null ? jobs[jobs.length - 1] || null : jobs.find((job) => job.id === id) || null;
  });
  return {
    publishOpen,
    publishForm,
    publishSubmitting,
    publishError,
    publishRecovery,
    publishJob,
    publishStats,
    publishRerender: computed(
      () => publishOpen.value && wouldRerender(publishForm.value, catalog.catalogField("publish_defaults").value || {}),
    ),
    togglePublishWizard,
    setPublishField,
    togglePublishLabel,
    submitPublish,
  };
});
