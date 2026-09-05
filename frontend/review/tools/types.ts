/** Tool-session boundaries keep dialogs independent of the main review implementation and its save queue. */
import type { ReviewStateMessage, ReviewUiState } from "../core/types";

/** The two shared-session capabilities needed by dialogs without exposing save-queue internals. */
export interface ToolSessionActions {
  applyMessage(this: void, message: ReviewStateMessage): void;
  updateSharedUi(this: void, patch: Partial<ReviewUiState>): Promise<void>;
}
