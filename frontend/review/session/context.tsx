/** Durable session and per-image drafts live above recoverable view boundaries, never inside a workspace render. */
import { createContext, type ComponentChildren } from "preact";
import { useContext, useLayoutEffect } from "preact/hooks";
import { createModel, signal, useModel, type ReadonlySignal } from "@preact/signals";
import { useReviewModel } from "../core/context";
import type { ReviewModelValue } from "../core/model";
import type {
  BasicRetouchAdjustments,
  ReadonlyData,
  RetouchSettings,
  RetouchObservation,
  ReviewImageObservation as ReviewImage,
} from "../core/types";
import { selectedProfile } from "../core/selectors";
import { retouchFromVisibleControls } from "./retouch-controls";
import { ReviewSessionModel, type ReviewSession } from "./use-session";
import { ReviewDraftModel, type DraftModelValue } from "./draft-model";

/** Models expose stable commands and narrowly observed presentation instead of rerendered controller copies. */
interface RuntimeValue {
  session: ReviewSession;
  drafts: DraftModelValue;
  readonly clipboard: ReadonlySignal<ReadonlyData<BasicRetouchAdjustments> | null>;
  setClipboard: (value: BasicRetouchAdjustments | null) => void;
  suspend: () => void;
  resume: () => void;
  stop: () => void;
}

/** Compose independent models once; model ownership disposes their timers only when the application ends. */
const ReviewRuntime = createModel((catalog: ReviewModelValue): RuntimeValue => {
  const session = new ReviewSessionModel(catalog);
  const clipboard = signal<BasicRetouchAdjustments | null>(null);
  const drafts = new ReviewDraftModel({
    findImage: (id: number): ReviewImage | null => catalog.confirmedImage(id),
    save: session.saveImageReview,
    visibleRetouch: (image: ReviewImage, value: RetouchObservation): RetouchSettings => {
      const state = catalog.getState();
      const displayed = catalog.image(image.id).peek() || image;
      const selected = selectedProfile(displayed, state);
      const profile = state.data?.profiles.find((item) => item.index === selected?.profile_index) || null;
      return retouchFromVisibleControls(value, profile, displayed);
    },
    presentRetouch: catalog.setRetouchDraft,
    schedule: (callback: () => void, delay: number): (() => void) => {
      const timer = window.setTimeout(callback, delay);
      return (): void => window.clearTimeout(timer);
    },
  });
  session.setDraftReader(
    (image) => drafts.fields(image.id),
    (id) => drafts.flush(id, false, { automatic: true }),
  );
  session.attachDrafts(drafts);
  return {
    session,
    drafts,
    clipboard,
    setClipboard: (value: BasicRetouchAdjustments | null): void => {
      clipboard.value = value;
    },
    suspend: (): void => {
      void drafts.flushOwned(undefined, true).catch(() => undefined);
    },
    resume: (): void => {
      void session.refresh().catch(() => undefined);
    },
    stop: (): void => {
      drafts.stop();
      session.stop();
    },
  };
});

const RuntimeContext = createContext<RuntimeValue | null>(null);

/** Install lifecycle listeners synchronously; hidden/pagehide reuse the normal queue and per-revision deduplication. */
export function SessionProvider({ children }: { children: ComponentChildren }): ComponentChildren {
  const catalog = useReviewModel();
  const runtime = useModel(() => new ReviewRuntime(catalog));
  useLayoutEffect(() => {
    /** Flush owned drafts on hide and refresh server state when the page becomes visible. */
    const visibility = (): void => {
      if (document.visibilityState === "hidden") runtime.suspend();
      else runtime.resume();
    };
    /** Refresh a restored back-forward-cache page whose stream may have been suspended. */
    const restore = (event: PageTransitionEvent): void => {
      if (event.persisted) runtime.resume();
    };
    document.addEventListener("visibilitychange", visibility);
    window.addEventListener("pagehide", runtime.suspend);
    window.addEventListener("pageshow", restore);
    return (): void => {
      runtime.stop();
      document.removeEventListener("visibilitychange", visibility);
      window.removeEventListener("pagehide", runtime.suspend);
      window.removeEventListener("pageshow", restore);
    };
  }, [runtime]);
  return <RuntimeContext.Provider value={runtime}>{children}</RuntimeContext.Provider>;
}

/** Read model identity without subscribing to unrelated connection, image, or form changes. */
export function useReviewRuntime(): RuntimeValue {
  const runtime = useContext(RuntimeContext);
  if (!runtime) throw new Error("Review session requires SessionProvider");
  return runtime;
}

/** Existing callers receive stable session actions; reactive properties subscribe only where accessed. */
export function useReviewSession(): ReviewSession {
  return useReviewRuntime().session;
}
