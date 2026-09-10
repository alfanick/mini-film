/** Tool-session boundaries keep dialogs independent of the main review implementation and its save queue. */
import type { ReviewStateMessage, ReviewUiState } from "../core/types";
import type { CommitContext, CommitScope } from "../session/barriers";

/** The two shared-session capabilities needed by dialogs without exposing save-queue internals. */
export interface ToolSessionActions {
  applyMessage(this: void, message: ReviewStateMessage): void;
  updateSharedUi(this: void, patch: Partial<ReviewUiState>): Promise<void>;
  commit<T>(this: void, scope: CommitScope, operation: (context: CommitContext) => Promise<T>): Promise<T>;
}
