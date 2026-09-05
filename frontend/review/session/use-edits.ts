/**
 * Keep metadata and retouch drafts in component state until their ordered save.
 * Capturing the image with every draft prevents delayed edits from reaching another picture.
 */
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { useReviewContext } from "../core/context";
import { currentImage, defaultRetouch, normalizedRetouch, selectedProfile } from "../core/selectors";
import { reviewUrl } from "../core/api";
import type {
  BasicRetouchAdjustments as RetouchAdjustments,
  RetouchSettings,
  ReviewImage,
  ReviewUpdateRequest,
} from "../core/types";
import type { ReviewActions } from "./use-session";
import { reviewRequestBody } from "./review-requests";
import { retouchFromVisibleControls } from "./retouch-controls";

/** Track fields independently so unedited values still follow incoming server updates. */
interface EditDraft {
  image: ReviewImage;
  tags: string;
  notes: string;
  retouch: RetouchSettings;
  tagsDirty: boolean;
  notesDirty: boolean;
  retouchDirty: boolean;
}

/** Repeated blur/navigation flushes can await the request already saving this draft. */
interface PendingSave {
  draft: EditDraft;
  promise: Promise<void>;
}

/** Controlled values and stable callbacks used by the review's editing controls. */
export interface ReviewEdits {
  tags: string;
  notes: string;
  retouch: RetouchSettings;
  clipboard: RetouchAdjustments | null;
  setTags: (value: string) => void;
  setNotes: (value: string) => void;
  focusMetadata: (field: "tags" | "notes" | null) => void;
  focusRetouch: (active: boolean) => void;
  setRetouch: (value: RetouchSettings, save?: boolean) => void;
  flush: (forceRetouch?: boolean) => Promise<void>;
  copy: () => void;
  paste: () => void;
}

/** Preserve the existing comma/space syntax, duplicate tags, and original tag order. */
export function parseTags(value: string): string[] {
  return value
    .split(/[,\s]+/)
    .map((tag: string): string => tag.trim())
    .filter(Boolean);
}

/** Debounce edits while retaining a synchronous flush boundary for navigation and unloading. */
export function useReviewEdits(actions: ReviewActions): ReviewEdits {
  const { state, getState, update } = useReviewContext();
  const { saveImageReview, setDraftReader } = actions;
  const image = currentImage(state);
  const [draft, setDraft] = useState<EditDraft | null>(null);
  const latest = useRef<EditDraft | null>(null);
  const focused = useRef<"tags" | "notes" | null>(null);
  const [metadataFocus, setMetadataFocus] = useState<"tags" | "notes" | null>(null);
  const retouchFocused = useRef<boolean>(false);
  const [retouchFocus, setRetouchFocus] = useState<boolean>(false);
  const saving = useRef<PendingSave | null>(null);
  const metadataTimer = useRef<number | undefined>(undefined);
  const retouchTimer = useRef<number | undefined>(undefined);
  const [clipboard, setClipboard] = useState<RetouchAdjustments | null>(null);

  /** Build an edit snapshot lazily so SSE updates remain visible until the user types. */
  const currentDraft = useCallback((): EditDraft | null => {
    const current = currentImage(getState());
    if (!current) return null;
    const previous = latest.current?.image.id === current.id ? latest.current : null;
    return {
      image: current,
      tags: previous && (previous.tagsDirty || focused.current === "tags") ? previous.tags : current.tags.join(", "),
      notes: previous && (previous.notesDirty || focused.current === "notes") ? previous.notes : current.notes || "",
      retouch:
        previous && (previous.retouchDirty || retouchFocused.current)
          ? previous.retouch
          : normalizedRetouch(current.retouch),
      tagsDirty: previous?.tagsDirty ?? false,
      notesDirty: previous?.notesDirty ?? false,
      retouchDirty: previous?.retouchDirty ?? false,
    };
  }, [getState]);

  /** Retain focused spelling and the caret across autosave acknowledgements. */
  const focusMetadata = useCallback(
    (field: "tags" | "notes" | null): void => {
      focused.current = field;
      setMetadataFocus(field);
      if (field) {
        const current = currentDraft();
        latest.current = current;
        setDraft(current);
      } else if (
        latest.current &&
        !retouchFocused.current &&
        !latest.current.tagsDirty &&
        !latest.current.notesDirty &&
        !latest.current.retouchDirty
      ) {
        latest.current = null;
        setDraft(null);
      }
    },
    [currentDraft],
  );

  /** Preserve focused slider values across autosave/SSE until the retouch section loses keyboard ownership. */
  const focusRetouch = useCallback(
    (active: boolean): void => {
      if (active === retouchFocused.current) return;
      const current = currentDraft();
      retouchFocused.current = active;
      setRetouchFocus(active);
      if (active) {
        latest.current = current;
        setDraft(current);
      } else if (
        latest.current &&
        !focused.current &&
        !latest.current.tagsDirty &&
        !latest.current.notesDirty &&
        !latest.current.retouchDirty
      ) {
        latest.current = null;
        setDraft(null);
      }
    },
    [currentDraft],
  );

  /** Resolve current profile baselines at the save boundary just as the original input reader did. */
  const visibleRetouch = useCallback(
    (current: ReviewImage, value: RetouchSettings): RetouchSettings => {
      const snapshot = getState();
      const selected = selectedProfile(current, snapshot);
      const profile = snapshot.data?.profiles.find((item) => item.index === selected?.profile_index) || null;
      return retouchFromVisibleControls(value, profile, current);
    },
    [getState],
  );

  /** Supply controlled inputs synchronously to queued ratings, labels, and profile changes. */
  const readDraft = useCallback(
    (current: ReviewImage): Partial<ReviewUpdateRequest> => {
      const pending = latest.current?.image.id === current.id ? latest.current : null;
      if (!pending) return {};
      return {
        ...(pending.tagsDirty || focused.current === "tags" ? { tags: parseTags(pending.tags) } : {}),
        ...(pending.notesDirty || focused.current === "notes" ? { notes: pending.notes } : {}),
        ...(pending.retouchDirty || retouchFocused.current
          ? { retouch: visibleRetouch(current, pending.retouch) }
          : {}),
      };
    },
    [visibleRetouch],
  );

  useLayoutEffect(() => {
    setDraftReader(readDraft);
    return (): void => setDraftReader(null);
  }, [setDraftReader, readDraft]);

  /** Retain manual drafts until their acknowledgement and reuse an already queued flush. */
  const flush = useCallback(
    async (forceRetouch = false): Promise<void> => {
      window.clearTimeout(metadataTimer.current);
      window.clearTimeout(retouchTimer.current);
      const pending = latest.current || (forceRetouch ? currentDraft() : null);
      if (!pending || (!forceRetouch && !pending.tagsDirty && !pending.notesDirty && !pending.retouchDirty)) return;
      if (!forceRetouch && saving.current?.draft === pending) return saving.current.promise;
      const fresh = getState().data?.images.find((item: ReviewImage): boolean => item.id === pending.image.id);
      // Enter commits visible retouch even when autosave made its draft clean before a later server update.
      const patch = {
        ...readDraft(pending.image),
        ...(forceRetouch ? { retouch: visibleRetouch(fresh || pending.image, pending.retouch) } : {}),
      };
      const promise = saveImageReview(fresh || pending.image, patch).then((): void => {
        if (latest.current === pending) {
          const clean =
            focused.current || retouchFocused.current
              ? { ...pending, tagsDirty: false, notesDirty: false, retouchDirty: false }
              : null;
          latest.current = clean;
          setDraft(clean);
        }
      });
      const request: PendingSave = { draft: pending, promise };
      saving.current = request;
      try {
        await promise;
      } finally {
        if (saving.current === request) saving.current = null;
      }
    },
    [saveImageReview, getState, readDraft, currentDraft, visibleRetouch],
  );

  /** Record a controlled metadata field and restart the short autosave debounce. */
  const metadata = useCallback(
    (field: "tags" | "notes", value: string): void => {
      const previous = currentDraft();
      if (!previous) return;
      const next: EditDraft = { ...previous, [field]: value, [`${field}Dirty`]: true };
      latest.current = next;
      setDraft(next);
      window.clearTimeout(metadataTimer.current);
      metadataTimer.current = window.setTimeout((): void => {
        void flush().catch(console.error);
      }, 500);
    },
    [currentDraft, flush],
  );

  /** Publish draft pixels through Preact state, leaving final RAW rendering to the daemon. */
  const setRetouch = useCallback(
    (value: RetouchSettings, save = true): void => {
      const previous = currentDraft();
      if (!previous) return;
      window.clearTimeout(retouchTimer.current);
      const retouch = normalizedRetouch(value);
      const next = { ...previous, retouch, retouchDirty: true };
      latest.current = next;
      setDraft(next);
      update((snapshot) => ({
        localRetouchDirty: true,
        data: snapshot.data
          ? {
              ...snapshot.data,
              images: snapshot.data.images.map((item: ReviewImage): ReviewImage =>
                item.id === previous.image.id ? { ...item, retouch } : item,
              ),
            }
          : null,
      }));
      if (save)
        retouchTimer.current = window.setTimeout((): void => {
          void flush().catch(console.error);
        }, 1200);
    },
    [currentDraft, flush, update],
  );

  /** Copy only slider deltas, keeping each picture's crop and rotation independent. */
  const copy = useCallback((): void => {
    const current = currentDraft();
    if (current) setClipboard({ ...visibleRetouch(current.image, current.retouch).adjustments });
  }, [currentDraft, visibleRetouch]);

  /** Paste a slider snapshot without reusing another picture's crop geometry. */
  const paste = useCallback((): void => {
    const current = currentDraft();
    if (current && clipboard) {
      setRetouch({ ...current.retouch, adjustments: { ...clipboard } });
      void flush().catch(console.error);
    }
  }, [clipboard, currentDraft, flush, setRetouch]);

  useEffect(() => {
    /** Preserve the original complete current-image beacon, including profile choices. */
    const unload = (): void => {
      const current = currentImage(getState());
      if (!current || !navigator.sendBeacon) return;
      const body = reviewRequestBody(current, readDraft(current));
      navigator.sendBeacon(reviewUrl("api/review"), new Blob([JSON.stringify(body)], { type: "application/json" }));
    };
    window.addEventListener("beforeunload", unload);
    return (): void => {
      window.removeEventListener("beforeunload", unload);
      window.clearTimeout(metadataTimer.current);
      window.clearTimeout(retouchTimer.current);
    };
  }, [getState, readDraft]);

  const visible = draft?.image.id === image?.id ? draft : null;
  return {
    tags: visible && (visible.tagsDirty || metadataFocus === "tags") ? visible.tags : (image?.tags.join(", ") ?? ""),
    notes: visible && (visible.notesDirty || metadataFocus === "notes") ? visible.notes : (image?.notes ?? ""),
    retouch:
      visible && (visible.retouchDirty || retouchFocus)
        ? visible.retouch
        : normalizedRetouch(image?.retouch ?? defaultRetouch()),
    clipboard,
    setTags: (value: string): void => metadata("tags", value),
    setNotes: (value: string): void => metadata("notes", value),
    focusMetadata,
    focusRetouch,
    setRetouch,
    flush,
    copy,
    paste,
  };
}
