/** Initialize shared review/viewer state; feature models initialize and own their own transient state. */
import type { ReviewState } from "./types";

/** A provider starts with no server snapshot or pending shared selection. */
export function createState(): ReviewState {
  return {
    data: null,
    currentId: null,
    labelFilters: new Set(),
    cropEditing: false,
    localRetouchDirty: false,
    mobileDrawer: null,
    pendingProfileSelections: new Map(),
    histogramOpen: false,
    informationOpen: false,
  };
}
