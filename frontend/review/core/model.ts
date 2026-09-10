/** Compose immutable catalog observations and explicit client-state cells without granting views write ownership. */
import { batch, computed, createModel, signal, type ReadonlySignal } from "@preact/signals";
import { createState } from "./state";
import { ReviewCatalog } from "./catalog";
import { reconcileReview, retainSnapshotIdentity } from "./reconcile";
import { imageLabels } from "./selectors";
import type { ReviewIntent } from "../session/commands";
import type {
  ReadonlyData,
  RetouchObservation,
  ReviewImageObservation,
  ReviewStateObservation,
  ReviewCatalogObservation,
  ReviewStateMessage,
} from "./types";

/** Server catalog and derived dirty state can only change through their owning actions. */
export type ReviewClientState = Omit<ReviewStateObservation, "data" | "localRetouchDirty">;
/** A client action never replaces confirmed wire data or manufactures derived state. */
export type ReviewStateUpdate =
  Partial<ReviewClientState> | ((state: ReviewStateObservation) => Partial<ReviewClientState>);

/** Public observation values are deeply readonly, including nested collection contents. */
export interface ReviewModelValue {
  readonly catalog: ReadonlySignal<ReviewCatalogObservation | null>;
  readonly state: ReadonlySignal<ReviewStateObservation>;
  readonly images: ReadonlySignal<readonly ReviewImageObservation[]>;
  readonly visibleImages: ReadonlySignal<readonly ReviewImageObservation[]>;
  readonly imagesById: ReadonlySignal<ReadonlyMap<number, ReviewImageObservation>>;
  readonly dirtyRetouchIds: ReadonlySignal<ReadonlySet<number>>;
  readonly imageIds: ReadonlySignal<readonly number[]>;
  readonly field: <K extends keyof ReviewStateObservation>(key: K) => ReadonlySignal<ReviewStateObservation[K]>;
  readonly image: (id: number) => ReadonlySignal<ReviewImageObservation | null>;
  readonly confirmedImage: (id: number) => ReviewImageObservation | null;
  readonly catalogField: <K extends keyof ReviewCatalogObservation>(
    key: K,
  ) => ReadonlySignal<ReviewCatalogObservation[K] | null>;
  readonly update: (patch: ReviewStateUpdate) => void;
  readonly getState: () => ReviewStateObservation;
  readonly getConfirmedState: () => ReviewStateObservation;
  readonly applyMessage: (message: ReadonlyData<ReviewStateMessage>) => void;
  readonly beginCommand: (imageId: number, intent: ReviewIntent) => number;
  readonly finishCommand: (id: number) => void;
  readonly setRetouchDraft: (imageId: number, value: RetouchObservation | null) => void;
}

/** Keep aggregate snapshots lazy while leaf consumers observe a field or picture directly. */
export const ReviewModel = createModel((): ReviewModelValue => {
  const initial = createState();
  // Explicit fields make additions type-checked without reflective heterogeneous-record assertions.
  const cells = {
    data: signal<ReviewStateObservation["data"]>(initial.data),
    currentId: signal<ReviewStateObservation["currentId"]>(initial.currentId),
    labelFilters: signal<ReviewStateObservation["labelFilters"]>(initial.labelFilters),
    cropEditing: signal<ReviewStateObservation["cropEditing"]>(initial.cropEditing),
    localRetouchDirty: signal<ReviewStateObservation["localRetouchDirty"]>(initial.localRetouchDirty),
    mobileDrawer: signal<ReviewStateObservation["mobileDrawer"]>(initial.mobileDrawer),
    pendingProfileSelections: signal<ReviewStateObservation["pendingProfileSelections"]>(
      initial.pendingProfileSelections,
    ),
    histogramOpen: signal<ReviewStateObservation["histogramOpen"]>(initial.histogramOpen),
    informationOpen: signal<ReviewStateObservation["informationOpen"]>(initial.informationOpen),
  };
  const confirmed = computed((): ReviewStateObservation => ({
    data: cells.data.value,
    currentId: cells.currentId.value,
    labelFilters: cells.labelFilters.value,
    cropEditing: cells.cropEditing.value,
    localRetouchDirty: cells.localRetouchDirty.value,
    mobileDrawer: cells.mobileDrawer.value,
    pendingProfileSelections: cells.pendingProfileSelections.value,
    histogramOpen: cells.histogramOpen.value,
    informationOpen: cells.informationOpen.value,
  }));
  /** Assign only supplied members; explicit null remains a replacement, never an omitted update. */
  function writeFields(patch: Partial<ReviewStateObservation>, includeConfirmed: boolean): void {
    batch((): void => {
      if (includeConfirmed && patch.data !== undefined) cells.data.value = patch.data;
      if (patch.currentId !== undefined) cells.currentId.value = patch.currentId;
      if (patch.labelFilters !== undefined) cells.labelFilters.value = patch.labelFilters;
      if (patch.cropEditing !== undefined) cells.cropEditing.value = patch.cropEditing;
      if (includeConfirmed && patch.localRetouchDirty !== undefined)
        cells.localRetouchDirty.value = patch.localRetouchDirty;
      if (patch.mobileDrawer !== undefined) cells.mobileDrawer.value = patch.mobileDrawer;
      if (patch.pendingProfileSelections !== undefined)
        cells.pendingProfileSelections.value = patch.pendingProfileSelections;
      if (patch.histogramOpen !== undefined) cells.histogramOpen.value = patch.histogramOpen;
      if (patch.informationOpen !== undefined) cells.informationOpen.value = patch.informationOpen;
    });
  }
  const catalog = new ReviewCatalog();
  const data = computed((): ReviewCatalogObservation | null => {
    const base = cells.data.value;
    if (!base) return null;
    const images = catalog.images.value;
    return images.length === base.images.length && images.every((image, index) => image === base.images[index])
      ? base
      : { ...base, images };
  });
  const localRetouchDirty = computed((): boolean => {
    const id = cells.currentId.value;
    return id !== null && catalog.dirtyIds.value.has(id);
  });
  const state = computed((): ReviewStateObservation => ({
    ...confirmed.value,
    data: data.value,
    localRetouchDirty: localRetouchDirty.value,
  }));
  const publicCells: { readonly [K in keyof ReviewStateObservation]: ReadonlySignal<ReviewStateObservation[K]> } = {
    ...cells,
    data,
    localRetouchDirty,
  };
  // Per-member projections keep server heartbeat/job traffic away from unrelated layout and editing components.
  const catalogFields: {
    readonly [K in keyof ReviewCatalogObservation]: ReadonlySignal<ReviewCatalogObservation[K] | null>;
  } = {
    bursts: computed(() => data.value?.bursts ?? null),
    capabilities: computed(() => data.value?.capabilities ?? null),
    client_count: computed(() => data.value?.client_count ?? null),
    codex: computed(() => data.value?.codex ?? null),
    diffusion_default: computed(() => data.value?.diffusion_default ?? null),
    images: computed(() => data.value?.images ?? null),
    invocation: computed(() => data.value?.invocation ?? null),
    panorama: computed(() => data.value?.panorama ?? null),
    profile_diffusion_settings: computed(() => data.value?.profile_diffusion_settings ?? null),
    profiles: computed(() => data.value?.profiles ?? null),
    publish_defaults: computed(() => data.value?.publish_defaults ?? null),
    publish_jobs: computed(() => data.value?.publish_jobs ?? null),
    publish_root: computed(() => data.value?.publish_root ?? null),
    ui: computed(() => data.value?.ui ?? null),
    version: computed(() => data.value?.version ?? null),
  };
  const minimumRating = computed((): number => catalogFields.ui.value?.min_rating ?? 0);
  let previousVisible: readonly ReviewImageObservation[] = [];
  const visibleImages = computed((): readonly ReviewImageObservation[] => {
    const minimum = minimumRating.value;
    const labels = cells.labelFilters.value;
    const next = catalog.images.value.filter(
      (image) =>
        image.rating >= minimum && (labels.size === 0 || imageLabels(image).some((label) => labels.has(label))),
    );
    if (next.length !== previousVisible.length || next.some((image, index) => image !== previousVisible[index]))
      previousVisible = next;
    return previousVisible;
  });
  return {
    catalog: data,
    state,
    images: catalog.images,
    visibleImages,
    imagesById: catalog.byId,
    imageIds: catalog.order,
    dirtyRetouchIds: catalog.dirtyIds,
    /** Expose the particular cell without subscribing its caller to a complete state snapshot. */
    field<K extends keyof ReviewStateObservation>(key: K): ReadonlySignal<ReviewStateObservation[K]> {
      return publicCells[key];
    },
    /** Reuse a picture-owned projection; unrelated commands never invalidate this cell. */
    image: (id: number): ReadonlySignal<ReviewImageObservation | null> => catalog.image(id),
    /** Read one acknowledged image in constant time at a mutation boundary. */
    confirmedImage: (id: number): ReviewImageObservation | null => catalog.confirmedImage(id),
    /** Keep catalog membership, job progress, and settings independently subscribable. */
    catalogField<K extends keyof ReviewCatalogObservation>(key: K): ReadonlySignal<ReviewCatalogObservation[K] | null> {
      return catalogFields[key];
    },
    /** Restrict UI updates to client-owned fields; derived and server-owned writes have separate entry points. */
    update(patch: ReviewStateUpdate): void {
      writeFields(typeof patch === "function" ? patch(state.peek()) : patch, false);
    },
    getState: (): ReviewStateObservation => state.peek(),
    getConfirmedState: (): ReviewStateObservation => confirmed.peek(),
    /** Merge confirmed messages before projecting pending intentions, preserving unacknowledged local revisions. */
    applyMessage(message: ReadonlyData<ReviewStateMessage>): void {
      const previous = confirmed.peek();
      const next = reconcileReview(previous, message);
      const snapshot = next.data ? retainSnapshotIdentity(previous.data, next.data) : next.data;
      batch((): void => {
        if (snapshot) catalog.replace(snapshot.images);
        writeFields(snapshot === undefined ? next : { ...next, data: snapshot }, true);
      });
    },
    /** Publish one semantic operation to its picture's cell. */
    beginCommand: (imageId: number, intent: ReviewIntent): number => catalog.begin(imageId, intent),
    /** A completed request removes only its own optimistic intent. */
    finishCommand: (id: number): void => catalog.finish(id),
    /** Preserve retouch presentation until the draft model acknowledges its exact revision. */
    setRetouchDraft: (imageId: number, value: RetouchObservation | null): void => catalog.setRetouch(imageId, value),
  };
});
