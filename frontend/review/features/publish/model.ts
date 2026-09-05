/**
 * Controlled publish drafts preserve text input and optional export fields while server jobs supply reactive
 * progress.
 */
import { useCallback, useMemo, useRef, useState } from "preact/hooks";
import { useReviewContext } from "../../core/context";
import { reviewApi, errorMessage } from "../../core/api";
import { imageLabels, isDirectCompressedImage, publishProfileIndexes } from "../../core/selectors";
import { COLOR_LABELS } from "../../core/constants";
import type { PublishRequest, ReviewLabel, ReviewPublishDefaults, ReviewPublishJob } from "../../core/types";
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
  publishOpen: boolean;
  publishForm: PublishDraft;
  publishSubmitting: boolean;
  publishError: string;
  publishJob: ReviewPublishJob | null;
  publishRerender: boolean;
  publishStats: { pictures: number; outputs: number };
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
export function publishBody(form: PublishDraft): PublishRequest {
  return {
    album: form.album.trim() || "published",
    min_rating: Number(form.minRating || 0),
    labels: form.labels,
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
export function usePublish(session: ToolSessionActions): PublishActions {
  const { state, getState } = useReviewContext();
  const [publishOpen, setOpen] = useState(false);
  const [publishForm, setForm] = useState<PublishDraft>(() =>
    defaultDraft(state.data?.publish_defaults || {}, state.data?.ui.min_rating || 0),
  );
  const [publishSubmitting, setSubmitting] = useState(false);
  const [publishError, setError] = useState("");
  const submitting = useRef(false);

  /** Reinitialize controls on each opening, matching the daemon-default publish workflow. */
  const togglePublishWizard = useCallback(
    (force?: boolean): void => {
      const show = force ?? !publishOpen;
      if (show) {
        const current = getState();
        setForm(defaultDraft(current.data?.publish_defaults || {}, current.data?.ui.min_rating || 0));
        setError("");
      }
      setOpen(show);
    },
    [publishOpen, getState],
  );

  /** Keep a field's declared value type when replacing part of the controlled draft. */
  const setPublishField = useCallback(
    <K extends keyof PublishDraft>(field: K, value: PublishDraft[K]): void =>
      setForm((current) => ({ ...current, [field]: value })),
    [],
  );

  /** Store selected color labels in the same order as the rendered checkbox list. */
  const togglePublishLabel = useCallback(
    (label: ReviewLabel, checked: boolean): void =>
      setForm((current) => ({
        ...current,
        labels: COLOR_LABELS.filter((candidate) =>
          candidate === label ? checked : current.labels.includes(candidate),
        ),
      })),
    [],
  );

  /** Start publishing with the current draft and merge the returned server job state. */
  const submitPublish = async (): Promise<void> => {
    if (submitting.current) return;
    submitting.current = true;
    setSubmitting(true);
    setError("");
    try {
      session.applyMessage(await reviewApi.publish({ body: publishBody(publishForm) }));
    } catch (error) {
      setError(`Publish failed: ${errorMessage(error)}`);
    } finally {
      submitting.current = false;
      setSubmitting(false);
    }
  };
  const publishStats = useMemo(() => {
    const body = publishBody(publishForm);
    const labels = new Set(body.labels);
    const tags = new Set(body.tags.map((tag) => tag.toLowerCase()));
    const totals = { pictures: 0, outputs: 0 };
    if (!publishOpen) return totals;
    for (const image of state.data?.images || []) {
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
  }, [publishForm, publishOpen, state.data?.images]);
  const jobs = state.data?.publish_jobs || [];
  const publishJob = jobs[jobs.length - 1] || null;
  return {
    publishOpen,
    publishForm,
    publishSubmitting,
    publishError,
    publishJob,
    publishStats,
    publishRerender: wouldRerender(publishForm, state.data?.publish_defaults || {}),
    togglePublishWizard,
    setPublishField,
    togglePublishLabel,
    submitPublish,
  };
}
