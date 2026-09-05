/** Per-image draft revisions survive shared navigation and acknowledge only the fields a completed save owned. */
import { computed, createModel, effect, signal, type ReadonlySignal } from "@preact/signals";
import type { RetouchSettings, ReviewImage, ReviewUpdateRequest } from "../core/types";
import { normalizedRetouch } from "../core/selectors";
import type { ReviewFields } from "./commands";

/** Acknowledgement is a local edit counter, never a claim about distributed server ordering. */
export interface DraftField<T> {
  readonly value: T;
  readonly revision: number;
  readonly acknowledged: number;
  readonly error: string;
}

/** Retouch is atomic so dependent crop and rotation coordinates cannot be acknowledged separately. */
export interface ImageDraft {
  readonly imageId: number;
  readonly tags: DraftField<string>;
  readonly notes: DraftField<string>;
  readonly retouch: DraftField<RetouchSettings>;
  readonly error: string;
}

/** Browser timers and transport remain injectable I/O boundaries rather than hidden model dependencies. */
export interface DraftPorts {
  findImage: (id: number) => ReviewImage | null;
  save: (image: ReviewImage, fields: Partial<ReviewUpdateRequest>) => Promise<void>;
  visibleRetouch: (image: ReviewImage, value: RetouchSettings) => RetouchSettings;
  presentRetouch: (id: number, value: RetouchSettings | null) => void;
  schedule: (callback: () => void, delay: number) => () => void;
}

/** Read-only draft signals and explicit image-scoped editing commands form the public boundary. */
export interface DraftModelValue {
  entries: ReadonlySignal<ReadonlyMap<number, ImageDraft>>;
  errors: ReadonlySignal<readonly { imageId: number; message: string }[]>;
  metadataFocus: ReadonlySignal<{ imageId: number; field: "tags" | "notes" } | null>;
  retouchFocus: ReadonlySignal<number | null>;
  image: (id: number) => ReadonlySignal<ImageDraft | null>;
  read: (id: number) => ImageDraft | null;
  fields: (id: number) => ReviewFields;
  focusMetadata: (id: number, field: "tags" | "notes" | null) => void;
  focusRetouch: (id: number, active: boolean) => void;
  setMetadata: (id: number, field: "tags" | "notes", value: string) => void;
  setRetouch: (id: number, value: RetouchSettings, save?: boolean) => void;
  flush: (id: number, forceRetouch?: boolean) => Promise<void>;
}

/** Preserve comma/space syntax, duplicates, and tag ordering at the request boundary. */
export function parseTags(value: string): string[] {
  return value
    .split(/[,\s]+/)
    .map((tag) => tag.trim())
    .filter(Boolean);
}

/** Compare local revisions without equating an SSE value match with an acknowledgement. */
export function fieldDirty<T>(field: DraftField<T>): boolean {
  return field.revision !== field.acknowledged;
}

/** Initialize a clean field while leaving server-provided values authoritative until editing begins. */
function initialField<T>(value: T): DraftField<T> {
  return { value, revision: 0, acknowledged: 0, error: "" };
}

/** Isolate timers, in-flight snapshots and errors by image rather than the current browser selection. */
export const ReviewDraftModel = createModel((ports: DraftPorts): DraftModelValue => {
  const entries = signal<ReadonlyMap<number, ImageDraft>>(new Map());
  const metadataFocus = signal<{ imageId: number; field: "tags" | "notes" } | null>(null);
  const retouchFocus = signal<number | null>(null);
  const timers = new Map<string, () => void>();
  const saving = new Map<string, Promise<void>>();
  const projections = new Map<number, ReadonlySignal<ImageDraft | null>>();
  let revision = 0;
  let disposed = false;

  /** Replace one entry, retaining all other image identities and their independent subscriptions. */
  function store(draft: ImageDraft | null, id: number): void {
    const next = new Map(entries.peek());
    if (draft)
      next.set(id, {
        ...draft,
        error: Array.from(new Set([draft.tags.error, draft.notes.error, draft.retouch.error].filter(Boolean))).join(
          "; ",
        ),
      });
    else next.delete(id);
    entries.value = next;
  }

  /** Merge server-owned fields lazily while preserving dirty or keyboard-owned raw values. */
  function read(id: number): ImageDraft | null {
    const image = ports.findImage(id);
    const previous = entries.peek().get(id);
    if (!image) return previous || null;
    const focus = metadataFocus.peek();
    return {
      imageId: id,
      tags:
        previous && (fieldDirty(previous.tags) || (focus?.imageId === id && focus.field === "tags"))
          ? previous.tags
          : initialField(image.tags.join(", ")),
      notes:
        previous && (fieldDirty(previous.notes) || (focus?.imageId === id && focus.field === "notes"))
          ? previous.notes
          : initialField(image.notes || ""),
      retouch:
        previous && (fieldDirty(previous.retouch) || retouchFocus.peek() === id)
          ? previous.retouch
          : initialField(normalizedRetouch(image.retouch)),
      error: previous?.error || "",
    };
  }

  /** Drop clean, unfocused entries without discarding another picture's pending edit. */
  function release(id: number): void {
    const draft = entries.peek().get(id);
    if (
      draft &&
      !draft.error &&
      !fieldDirty(draft.tags) &&
      !fieldDirty(draft.notes) &&
      !fieldDirty(draft.retouch) &&
      metadataFocus.peek()?.imageId !== id &&
      retouchFocus.peek() !== id
    )
      store(null, id);
  }

  /** Capture only local/focused fields; the command queue supplies untouched fields when sending. */
  function fields(id: number): ReviewFields {
    const draft = entries.peek().get(id);
    const image = ports.findImage(id);
    if (!draft || !image) return {};
    const focus = metadataFocus.peek();
    return {
      ...(fieldDirty(draft.tags) || (focus?.imageId === id && focus.field === "tags")
        ? { tags: parseTags(draft.tags.value) }
        : {}),
      ...(fieldDirty(draft.notes) || (focus?.imageId === id && focus.field === "notes")
        ? { notes: draft.notes.value }
        : {}),
      ...(fieldDirty(draft.retouch) || retouchFocus.peek() === id
        ? { retouch: ports.visibleRetouch(image, draft.retouch.value) }
        : {}),
    };
  }

  /** Cancel only the named image's debounce; other drafts continue saving after shared navigation. */
  function cancel(id: number, kind: "metadata" | "retouch"): void {
    const key = `${id}:${kind}`;
    timers.get(key)?.();
    timers.delete(key);
  }

  /** Keep an acknowledged raw field focused while refusing to clear a subsequently edited revision. */
  function acknowledge<T>(current: DraftField<T>, submitted: DraftField<T>): DraftField<T> {
    return current.revision === submitted.revision
      ? { ...current, acknowledged: submitted.revision, error: "" }
      : current;
  }

  /** A late failure owns only its still-dirty submitted revision, never a newer edit or acknowledgement. */
  function failed<T>(current: DraftField<T>, submitted: DraftField<T>, message: string): DraftField<T> {
    return current.revision === submitted.revision && fieldDirty(current) ? { ...current, error: message } : current;
  }

  /** Submit one captured revision set and retain failed drafts for an explicit retry. */
  async function flush(id: number, forceRetouch = false): Promise<void> {
    cancel(id, "metadata");
    cancel(id, "retouch");
    let pending = entries.peek().get(id) || (forceRetouch ? read(id) : null);
    if (
      !pending ||
      (!forceRetouch && !fieldDirty(pending.tags) && !fieldDirty(pending.notes) && !fieldDirty(pending.retouch))
    )
      return;
    if (forceRetouch) {
      pending = { ...pending, retouch: { ...pending.retouch, revision: ++revision, error: "" } };
      store(pending, id);
      ports.presentRetouch(id, pending.retouch.value);
    }
    const submitted = pending;
    const key = `${id}:${pending.tags.revision}:${pending.notes.revision}:${pending.retouch.revision}`;
    const existing = saving.get(key);
    if (!forceRetouch && existing) return existing;
    const image = ports.findImage(id);
    if (!image) {
      const error = new Error(`Picture ${id} is unavailable; its unsaved edits have been retained`);
      store(
        {
          ...pending,
          tags: failed(pending.tags, pending.tags, error.message),
          notes: failed(pending.notes, pending.notes, error.message),
          retouch: failed(pending.retouch, pending.retouch, error.message),
        },
        id,
      );
      throw error;
    }
    const patch = {
      ...fields(id),
      ...(forceRetouch ? { retouch: ports.visibleRetouch(image, pending.retouch.value) } : {}),
    };
    const promise = ports
      .save(image, patch)
      .then((): void => {
        if (disposed) return;
        const current = entries.peek().get(id);
        if (!current) return;
        const next: ImageDraft = {
          ...current,
          tags: patch.tags !== undefined ? acknowledge(current.tags, submitted.tags) : current.tags,
          notes: patch.notes !== undefined ? acknowledge(current.notes, submitted.notes) : current.notes,
          retouch: patch.retouch !== undefined ? acknowledge(current.retouch, submitted.retouch) : current.retouch,
        };
        store(next, id);
        if (!fieldDirty(next.retouch)) ports.presentRetouch(id, null);
        release(id);
      })
      .catch((error: unknown): never => {
        if (!disposed) {
          const current = entries.peek().get(id);
          const message = error instanceof Error ? error.message : String(error);
          if (current)
            store(
              {
                ...current,
                tags: patch.tags !== undefined ? failed(current.tags, submitted.tags, message) : current.tags,
                notes: patch.notes !== undefined ? failed(current.notes, submitted.notes, message) : current.notes,
                retouch:
                  patch.retouch !== undefined ? failed(current.retouch, submitted.retouch, message) : current.retouch,
              },
              id,
            );
        }
        throw error;
      });
    saving.set(key, promise);
    try {
      await promise;
    } finally {
      if (saving.get(key) === promise) saving.delete(key);
    }
  }

  /** Restart a per-image debounce; errors remain visible instead of triggering automatic mutation retries. */
  function schedule(id: number, kind: "metadata" | "retouch", delay: number): void {
    cancel(id, kind);
    const key = `${id}:${kind}`;
    timers.set(
      key,
      ports.schedule((): void => {
        timers.delete(key);
        void flush(id).catch(() => undefined);
      }, delay),
    );
  }

  effect(() => (): void => {
    disposed = true;
    for (const stop of timers.values()) stop();
    timers.clear();
  });
  return {
    entries,
    metadataFocus,
    retouchFocus,
    errors: computed(() =>
      Array.from(entries.value.values())
        .filter((draft) => draft.error)
        .map((draft) => ({ imageId: draft.imageId, message: draft.error })),
    ),
    /** Cache a leaf projection so editing another picture does not invalidate this input. */
    image(id: number): ReadonlySignal<ImageDraft | null> {
      let projection = projections.get(id);
      if (!projection) {
        projection = computed(() => entries.value.get(id) || null);
        projections.set(id, projection);
      }
      return projection;
    },
    read,
    fields,
    /** Keyboard ownership preserves raw spelling and the caret through acknowledgements. */
    focusMetadata(id: number, field: "tags" | "notes" | null): void {
      const previous = metadataFocus.peek()?.imageId;
      if (field) store(read(id), id);
      metadataFocus.value = field ? { imageId: id, field } : null;
      if (previous !== undefined) release(previous);
    },
    /** Retouch focus retains visible values even after a clean autosave. */
    focusRetouch(id: number, active: boolean): void {
      const previous = retouchFocus.peek();
      if (active) store(read(id), id);
      retouchFocus.value = active ? id : null;
      if (previous !== null) release(previous);
    },
    /** Assign a new local revision before restarting the metadata debounce. */
    setMetadata(id: number, field: "tags" | "notes", value: string): void {
      const previous = read(id);
      if (!previous) return;
      store({ ...previous, [field]: { ...previous[field], value, revision: ++revision, error: "" } }, id);
      schedule(id, "metadata", 500);
    },
    /** Publish draft pixels immediately while the server retains final RAW rendering ownership. */
    setRetouch(id: number, value: RetouchSettings, save = true): void {
      const previous = read(id);
      if (!previous) return;
      cancel(id, "retouch");
      const retouch = normalizedRetouch(value);
      store({ ...previous, retouch: { ...previous.retouch, value: retouch, revision: ++revision, error: "" } }, id);
      ports.presentRetouch(id, retouch);
      if (save) schedule(id, "retouch", 1200);
    },
    flush,
  };
});
