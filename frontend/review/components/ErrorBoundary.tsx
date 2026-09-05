/** Contain rendering failures without discarding provider-owned drafts or silently blanking the review UI. */
import type { ComponentChildren } from "preact";
import { useErrorBoundary } from "preact/hooks";
import { errorMessage } from "../core/api";

/** Let the user retry rendering while the parent provider retains their unsaved work. */
export function ErrorBoundary({ children }: { children: ComponentChildren }): ComponentChildren {
  const boundary: [unknown, () => void] = useErrorBoundary();
  const error: unknown = boundary[0];
  const reset: () => void = boundary[1];
  if (error) {
    return (
      <section role="alert" class="empty">
        <h2>Review could not display this view</h2>
        <p>{errorMessage(error)}</p>
        <p>Your pending edits remain in this session.</p>
        <button type="button" onClick={reset}>
          Try displaying the review again
        </button>
      </section>
    );
  }
  return children;
}
