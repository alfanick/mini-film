/**
 * Keep modal state in Preact while the native dialog owns the top layer, focus containment, and background inertness.
 * Explicit focus restoration also covers a dialog that disappears with its parent feature.
 */
import type { ComponentChildren, JSX } from "preact";
import { useLayoutEffect, useRef } from "preact/hooks";
import { ErrorBoundary } from "./ErrorBoundary";

/** Shared modal presentation keeps existing overlay classes and stable accessible heading IDs. */
interface DialogProps {
  id: string;
  className: string;
  labelledBy: string;
  label: string;
  open: boolean;
  onClose: () => void;
  children: ComponentChildren;
}

/** Find visible native controls, excluding disabled fields and content hidden by closed details or inert parents. */
function dialogControls(dialog: HTMLDialogElement): HTMLElement[] {
  return Array.from(
    dialog.querySelectorAll<HTMLElement>(
      "a[href], button, input, select, textarea, summary, [tabindex], [contenteditable=true]",
    ),
  ).filter(
    (element): boolean =>
      element.tabIndex >= 0 &&
      !element.matches(":disabled") &&
      !element.closest("[inert]") &&
      element.getClientRects().length > 0 &&
      getComputedStyle(element).visibility !== "hidden",
  );
}

/** Synchronize native modal capabilities without letting the browser become a second source of application state. */
export function Dialog({ id, className, labelledBy, label, open, onClose, children }: DialogProps): JSX.Element {
  const ref = useRef<HTMLDialogElement>(null);
  const returnFocus = useRef<HTMLElement | null>(null);

  /** Restore the invoker only if it still belongs to the current page. */
  function restoreFocus(): void {
    const invoker = returnFocus.current;
    returnFocus.current = null;
    if (invoker?.isConnected) invoker.focus({ preventScroll: true });
  }

  /** Keep Tab at the modal edges instead of transferring focus into the hosting browser's chrome. */
  function containTab(event: JSX.TargetedKeyboardEvent<HTMLDialogElement>): void {
    if (event.key !== "Tab" || event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) return;
    const controls = dialogControls(event.currentTarget);
    const first = controls[0];
    const last = controls[controls.length - 1];
    if (!first || !last) {
      event.preventDefault();
      event.currentTarget.focus();
    } else if (
      event.shiftKey &&
      (document.activeElement === first || !controls.some((control) => control === document.activeElement))
    ) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  useLayoutEffect((): void => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) {
      returnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      dialog.showModal();
      // Chromium can initially focus an implicit scroll container, whose reverse Tab otherwise leaves the page.
      dialogControls(dialog)[0]?.focus({ preventScroll: true });
    } else if (!open && dialog.open) {
      dialog.close();
      restoreFocus();
    }
  }, [open]);

  useLayoutEffect((): (() => void) => {
    const dialog = ref.current;
    return (): void => {
      if (dialog?.open) dialog.close();
      restoreFocus();
    };
  }, []);

  return (
    <dialog
      ref={ref}
      id={id}
      class={`review-dialog ${className}`}
      aria-labelledby={labelledBy}
      aria-label={label}
      hidden={!open}
      onKeyDown={containTab}
      onCancel={(event): void => {
        event.preventDefault();
        onClose();
      }}
      onClose={(event): void => {
        if (open && !event.currentTarget.open) onClose();
      }}
      onClick={(event): void => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <ErrorBoundary>{children}</ErrorBoundary>
    </dialog>
  );
}
