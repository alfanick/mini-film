/**
 * Connect per-image reactive draft revisions to controlled inputs and browser unload handling.
 * Request and timer ownership lives in the draft model, so shared navigation cannot retarget an edit.
 */
import { useCallback, useLayoutEffect } from "preact/hooks";
import { useReviewRuntime } from "./context";
import { useReviewModel } from "../core/context";
import { currentImage, defaultRetouch, normalizedRetouch, selectedProfile } from "../core/selectors";

import type {
  BasicRetouchAdjustments as RetouchAdjustments,
  ReadonlyData,
  RetouchObservation as RetouchSettings,
  ReviewImageObservation as ReviewImage,
} from "../core/types";

import { retouchFromVisibleControls } from "./retouch-controls";
import { fieldDirty } from "./draft-model";

export { parseTags } from "./draft-model";

/** Controlled values and callbacks keep existing focus, debounce, and explicit Enter behavior. */
export interface ReviewEdits {
  tags: string;
  notes: string;
  retouch: RetouchSettings;
  clipboard: ReadonlyData<RetouchAdjustments> | null;
  errors: readonly { imageId: number; message: string }[];
  retry: (imageId: number) => Promise<void>;
  setTags: (value: string) => void;
  setNotes: (value: string) => void;
  focusMetadata: (field: "tags" | "notes" | null) => void;
  focusRetouch: (active: boolean) => void;
  setRetouch: (value: RetouchSettings, save?: boolean) => void;
  flush: (forceRetouch?: boolean) => Promise<void>;
  copy: () => void;
  paste: () => void;
}

/** Retain drafts per picture while subscribing the controlled fields only to the displayed image. */
export function useReviewEdits(): ReviewEdits {
  const model = useReviewModel();
  const { drafts, session, clipboard: storedClipboard, setClipboard } = useReviewRuntime();
  const imageId = model.field("currentId").value;
  const image = imageId !== null ? model.image(imageId).value : null;
  const clipboard = storedClipboard.value;

  /** Resolve current profile baselines at the save boundary, preserving the original clamped input rules. */
  const visibleRetouch = useCallback(
    (current: ReviewImage, value: RetouchSettings): RetouchSettings => {
      const snapshot = model.getState();
      const displayed = model.image(current.id).peek() || current;
      const selected = selectedProfile(displayed, snapshot);
      const profile = snapshot.data?.profiles.find((item) => item.index === selected?.profile_index) || null;
      return retouchFromVisibleControls(value, profile, displayed);
    },
    [model],
  );
  const draft = imageId !== null ? drafts.image(imageId).value : null;
  const metadataFocus = drafts.metadataFocus.value;
  const retouchFocus = drafts.retouchFocus.value;
  const errors = drafts.errors.value;

  // Shared navigation reuses the focused input element; move keyboard ownership without copying the old value.
  useLayoutEffect((): void => {
    if (imageId === null) return;
    const focused = drafts.metadataFocus.peek();
    if (focused && focused.imageId !== imageId) drafts.focusMetadata(imageId, focused.field);
    if (drafts.retouchFocus.peek() !== null && drafts.retouchFocus.peek() !== imageId)
      drafts.focusRetouch(imageId, true);
  }, [drafts, imageId]);

  /** Capture the current image identity before handing a flush to the asynchronous save queue. */
  const flush = useCallback(
    (forceRetouch = false): Promise<void> => {
      const current = currentImage(model.getState());
      return current ? drafts.flush(current.id, forceRetouch) : Promise.resolve();
    },
    [drafts, model],
  );

  /** Copy slider deltas only; crop and rotation remain owned by their original picture. */
  const copy = useCallback((): void => {
    const current = currentImage(model.getState());
    if (!current) return;
    const pending = drafts.read(current.id);
    if (pending) setClipboard({ ...visibleRetouch(current, pending.retouch.value).adjustments });
  }, [drafts, model, visibleRetouch, setClipboard]);

  /** Paste into this picture's geometry and explicitly commit the newly created local revision. */
  const paste = useCallback((): void => {
    const current = currentImage(model.getState());
    if (!current || !clipboard) return;
    const pending = drafts.read(current.id);
    if (!pending) return;
    drafts.setRetouch(current.id, { ...pending.retouch.value, adjustments: { ...clipboard } });
    void drafts.flush(current.id).catch(() => undefined);
  }, [clipboard, drafts, model]);

  /** Dispatch an input event against the identity visible at that event boundary. */
  const withCurrent = (callback: (id: number) => void): void => {
    const id = model.getState().currentId;
    if (id !== null) callback(id);
  };
  return {
    tags:
      draft && (fieldDirty(draft.tags) || (metadataFocus?.imageId === imageId && metadataFocus.field === "tags"))
        ? draft.tags.value
        : image?.tags.join(", ") || "",
    notes:
      draft && (fieldDirty(draft.notes) || (metadataFocus?.imageId === imageId && metadataFocus.field === "notes"))
        ? draft.notes.value
        : image?.notes || "",
    retouch:
      draft && (fieldDirty(draft.retouch) || retouchFocus === imageId)
        ? draft.retouch.value
        : normalizedRetouch(image?.retouch || defaultRetouch()),
    clipboard,
    errors,
    retry: async (id: number): Promise<void> => {
      await session.refresh();
      await drafts.flush(id);
    },
    setTags: (value: string): void => withCurrent((id) => drafts.setMetadata(id, "tags", value)),
    setNotes: (value: string): void => withCurrent((id) => drafts.setMetadata(id, "notes", value)),
    focusMetadata: (field): void => withCurrent((id) => drafts.focusMetadata(id, field)),
    focusRetouch: (active): void => withCurrent((id) => drafts.focusRetouch(id, active)),
    setRetouch: (value, save = true): void => withCurrent((id) => drafts.setRetouch(id, value, save)),
    flush,
    copy,
    paste,
  };
}
