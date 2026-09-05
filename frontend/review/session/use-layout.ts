/**
 * Bridge responsive browser measurements into reactive layout state.
 * Preact owns layout classes/styles; observers only measure available image space.
 */
import type { RefObject } from "preact";
import { useEffect, useLayoutEffect, useState } from "preact/hooks";

/** Subscribe to a browser breakpoint for the mounted component's lifetime. */
export function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState<boolean>(() => window.matchMedia(query).matches);
  useEffect(() => {
    const media = window.matchMedia(query);
    /** Forward browser breakpoint changes into the next Preact render. */
    const change = (): void => setMatches(media.matches);
    change();
    media.addEventListener("change", change);
    return (): void => media.removeEventListener("change", change);
  }, [query]);
  return matches;
}

/** Reserve the panel's actual overlap rather than assuming fixed desktop/mobile heights. */
export function usePanelSafeArea(
  workspace: RefObject<HTMLElement>,
  panel: RefObject<HTMLElement>,
  layoutKey: string,
): number {
  const [safeArea, setSafeArea] = useState<number>(0);
  useLayoutEffect(() => {
    const workspaceElement = workspace.current,
      panelElement = panel.current;
    if (!workspaceElement || !panelElement) return;
    /** Read geometry only; the caller applies the resulting CSS variable through JSX. */
    const measure = (): void =>
      setSafeArea(
        Math.max(
          0,
          Math.ceil(workspaceElement.getBoundingClientRect().bottom - panelElement.getBoundingClientRect().top),
        ),
      );
    let frame = 0;
    /** Defer resize-driven rendering to the next frame to avoid observer delivery loops in WebKit. */
    const schedule = (): void => {
      window.cancelAnimationFrame(frame);
      frame = window.requestAnimationFrame(measure);
    };
    const observer = new ResizeObserver(schedule);
    observer.observe(workspaceElement);
    observer.observe(panelElement);
    measure();
    return (): void => {
      observer.disconnect();
      window.cancelAnimationFrame(frame);
    };
  }, [workspace, panel, layoutKey]);
  return safeArea;
}
