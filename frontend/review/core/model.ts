/** Provider-scoped Signals separate confirmed catalog data from pending local intentions and leaf subscriptions. */
import { batch, computed, createModel, signal, type ReadonlySignal, type Signal } from "@preact/signals";
import { createState } from "./state";
import { reconcileReview, retainSnapshotIdentity } from "./reconcile";
import { imageLabels } from "./selectors";
import { projectReviewIntent, type ReviewIntent } from "../session/commands";
import type {
  ReadonlyData,
  RetouchSettings,
  ReviewImage,
  ReviewState,
  ReviewStateData,
  ReviewStateMessage,
} from "./types";

export type ReviewStateUpdate = Partial<ReviewState> | ((state: ReviewState) => Partial<ReviewState>);

/** Track local command ownership independently of server event timestamps. */
interface PendingCommand {
  id: number;
  imageId: number;
  intent: ReviewIntent;
}

/** Public signals are read-only; writes cross explicit model actions. */
export interface ReviewModelValue {
  catalog: ReadonlySignal<ReadonlyData<ReviewStateData> | null>;
  state: ReadonlySignal<ReviewState>;
  images: ReadonlySignal<ReviewImage[]>;
  visibleImages: ReadonlySignal<ReviewImage[]>;
  imagesById: ReadonlySignal<ReadonlyMap<number, ReviewImage>>;
  dirtyRetouchIds: ReadonlySignal<ReadonlySet<number>>;
  field: <K extends keyof ReviewState>(key: K) => ReadonlySignal<ReviewState[K]>;
  image: (id: number) => ReadonlySignal<ReviewImage | null>;
  update: (patch: ReviewStateUpdate) => void;
  getState: () => ReviewState;
  getConfirmedState: () => ReviewState;
  applyMessage: (message: ReviewStateMessage) => void;
  beginCommand: (imageId: number, intent: ReviewIntent) => number;
  finishCommand: (id: number) => void;
  setRetouchDraft: (imageId: number, value: RetouchSettings | null) => void;
}

/** Normalize image identity and expose narrowly subscribable values for large catalogs. */
export const ReviewModel = createModel((): ReviewModelValue => {
  const initial = createState();
  const keys = Object.keys(initial) as (keyof ReviewState)[];
  // Each field has its own writable cell; the complete snapshot is an on-demand compatibility projection.
  const cells = Object.fromEntries(keys.map((key) => [key, signal(initial[key])])) as {
    [K in keyof ReviewState]: Signal<ReviewState[K]>;
  };
  const confirmed = computed(
    (): ReviewState => Object.fromEntries(keys.map((key) => [key, cells[key].value])) as unknown as ReviewState,
  );
  /** Write heterogeneous field cells through a checked key/value pair and one batched transaction. */
  function writeFields(patch: Partial<ReviewState>): void {
    /** Keep the indexed assignment's value tied to its concrete key. */
    function assign<K extends keyof ReviewState>(key: K, value: ReviewState[K]): void {
      cells[key].value = value;
    }
    batch(() => {
      for (const key of Object.keys(patch) as (keyof ReviewState)[])
        if (Object.prototype.hasOwnProperty.call(patch, key)) assign(key, patch[key] as ReviewState[typeof key]);
    });
  }
  const commands = signal<readonly PendingCommand[]>([]);
  const retouchDrafts = signal<ReadonlyMap<number, RetouchSettings>>(new Map());
  const emptyImages: ReviewImage[] = [];
  let commandId = 0;
  let previousImages: ReviewImage[] = emptyImages;
  const projected = new Map<
    number,
    {
      original: ReviewImage;
      commands: readonly PendingCommand[];
      retouch: RetouchSettings | undefined;
      image: ReviewImage;
    }
  >();
  const images = computed((): ReviewImage[] => {
    const source = cells.data.value?.images || emptyImages;
    const pending = commands.value;
    const drafts = retouchDrafts.value;
    if (!pending.length && !drafts.size) {
      projected.clear();
      previousImages = source;
      return source;
    }
    const byImage = new Map<number, PendingCommand[]>();
    for (const command of pending) {
      const own = byImage.get(command.imageId) || [];
      own.push(command);
      byImage.set(command.imageId, own);
    }
    const next = source.map((original) => {
      const own = byImage.get(original.id) || [];
      const retouch = drafts.get(original.id);
      if (!own.length && !retouch) {
        projected.delete(original.id);
        return original;
      }
      const cached = projected.get(original.id);
      if (
        cached?.original === original &&
        cached.retouch === retouch &&
        cached.commands.length === own.length &&
        cached.commands.every((command, index) => command === own[index])
      )
        return cached.image;
      let image = original;
      for (const command of own) image = projectReviewIntent(image, command.intent);
      if (retouch) image = { ...image, retouch };
      projected.set(original.id, { original, commands: own, retouch, image });
      return image;
    });
    if (next.length === previousImages.length && next.every((image, index) => image === previousImages[index]))
      return previousImages;
    previousImages = next;
    return next;
  });
  const imagesById = computed(
    (): ReadonlyMap<number, ReviewImage> => new Map(images.value.map((image) => [image.id, image])),
  );
  const dirtyRetouchIds = computed((): ReadonlySet<number> => new Set(retouchDrafts.value.keys()));
  const data = computed((): ReviewStateData | null => {
    const base = cells.data.value;
    const catalog = images.value;
    return base ? (catalog === base.images ? base : { ...base, images: catalog }) : null;
  });
  const localRetouchDirty = computed((): boolean => {
    const id = cells.currentId.value;
    return id !== null && dirtyRetouchIds.value.has(id);
  });
  const state = computed((): ReviewState => ({
    ...confirmed.value,
    data: data.value,
    localRetouchDirty: localRetouchDirty.value,
  }));
  const publicCells: { [K in keyof ReviewState]: ReadonlySignal<ReviewState[K]> } = {
    ...cells,
    data,
    localRetouchDirty,
  };
  const imageSignals = new Map<number, ReadonlySignal<ReviewImage | null>>();
  let filteredSource: ReviewImage[] = emptyImages;
  let filterKey = "";
  let visible: ReviewImage[] = emptyImages;
  const visibleImages = computed((): ReviewImage[] => {
    const source = images.value;
    const minimum = cells.data.value?.ui.min_rating || 0;
    const labels = cells.labelFilters.value;
    const key = `${minimum}:${Array.from(labels).join(",")}`;
    if (source !== filteredSource || key !== filterKey) {
      filteredSource = source;
      filterKey = key;
      visible = source.filter(
        (image) =>
          image.rating >= minimum && (labels.size === 0 || imageLabels(image).some((label) => labels.has(label))),
      );
    }
    return visible;
  });
  return {
    catalog: data,
    state,
    images,
    visibleImages,
    imagesById,
    dirtyRetouchIds,
    /** Cache each projection so components subscribe only to fields they display. */
    field<K extends keyof ReviewState>(key: K): ReadonlySignal<ReviewState[K]> {
      return publicCells[key];
    },
    /** Retain one image subscription through unrelated catalog changes. */
    image(id: number): ReadonlySignal<ReviewImage | null> {
      let projection = imageSignals.get(id);
      if (!projection) {
        projection = computed(() => imagesById.value.get(id) || null);
        imageSignals.set(id, projection);
      }
      return projection;
    },
    /** Update client state without exposing writable signals to components. */
    update(patch: ReviewStateUpdate): void {
      const partial = typeof patch === "function" ? patch(state.peek()) : patch;
      writeFields(partial);
    },
    getState: (): ReviewState => state.peek(),
    getConfirmedState: (): ReviewState => confirmed.peek(),
    /** Merge server fields without acknowledging any local draft revision. */
    applyMessage(message: ReviewStateMessage): void {
      const previous = confirmed.peek();
      const next = reconcileReview(previous, message);
      if (next.data) next.data = retainSnapshotIdentity(previous.data, next.data);
      writeFields(next);
    },
    /** Publish an intention before its HTTP request reaches the queue's front. */
    beginCommand(imageId: number, intent: ReviewIntent): number {
      const id = ++commandId;
      commands.value = [...commands.peek(), { id, imageId, intent }];
      return id;
    },
    /** Remove only this completed command; newer intentions keep their ownership. */
    finishCommand(id: number): void {
      commands.value = commands.peek().filter((command) => command.id !== id);
    },
    /** Keep local retouch presentation until the matching edit revision is acknowledged. */
    setRetouchDraft(imageId: number, value: RetouchSettings | null): void {
      if (value ? retouchDrafts.peek().get(imageId) === value : !retouchDrafts.peek().has(imageId)) return;
      const next = new Map(retouchDrafts.peek());
      if (value) next.set(imageId, value);
      else next.delete(imageId);
      retouchDrafts.value = next;
    },
  };
});
