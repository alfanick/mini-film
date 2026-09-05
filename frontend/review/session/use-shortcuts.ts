/**
 * Own global review keyboard and wheel shortcuts with lifecycle cleanup.
 * Form controls handle their own keys; dialog and crop state take precedence over photo navigation.
 */
import { useLayoutEffect, useRef } from "preact/hooks";
import type { JSX, RefObject } from "preact";
import { useReviewContext } from "../core/context";
import {
  currentImage,
  isDirectCompressedImage,
  isSoocProfile,
  profilesAreImplicitOnly,
  selectedProfile,
} from "../core/selectors";
import { WHEEL_NAV_COOLDOWN_MS, WHEEL_NAV_RESET_MS, WHEEL_NAV_THRESHOLD_PX } from "../core/constants";
import type { ReviewLabel } from "../core/types";
import type { ReviewActions } from "./use-session";
import type { ReviewEdits } from "./use-edits";
import type { ToolsController } from "../tools/use-tools";

interface ShortcutOptions {
  actions: ReviewActions;
  edits: ReviewEdits;
  tools: ToolsController;
  shortcutsOpen: boolean;
  setShortcutsOpen: (open: boolean) => void;
  tagsRef: RefObject<HTMLInputElement>;
  notesRef: RefObject<HTMLInputElement>;
  mobile: boolean;
  appRef: RefObject<HTMLDivElement>;
}
interface WheelState {
  axis: "x" | "y" | null;
  amount: number;
  lastAt: number;
  lockedUntil: number;
}

/** Match established fullscreen behavior without rendering outside the Preact application. */
async function toggleFullscreen(app: HTMLDivElement | null): Promise<void> {
  if (document.fullscreenElement) await document.exitFullscreen();
  else await app?.requestFullscreen();
}

/** Return a JSX wheel handler and attach one current keyboard listener. */
export function useReviewShortcuts(options: ShortcutOptions): (event: JSX.TargetedWheelEvent<HTMLElement>) => void {
  const { state, update } = useReviewContext();
  const wheel = useRef<WheelState>({ axis: null, amount: 0, lastAt: 0, lockedUntil: 0 });
  const { actions, edits, tools, shortcutsOpen, setShortcutsOpen, tagsRef, notesRef, mobile, appRef } = options;
  const modal = state.diffusionOpen || state.samplerOpen || state.panoramaOpen || tools.publishOpen || shortcutsOpen;
  useLayoutEffect(() => {
    /** Route keyboard actions in modal-first order so typing never rates a picture accidentally. */
    const keydown = (event: KeyboardEvent): void => {
      if (event.defaultPrevented) return;
      const key = event.key.toLowerCase();
      if (state.profileInfoProfileIndex !== null && event.key === "Escape") {
        event.preventDefault();
        tools.closeProfileInfo();
        return;
      }
      if (state.commandInvocationOpen && event.key === "Escape") {
        event.preventDefault();
        tools.closeCommandInvocation();
        return;
      }
      if (state.diffusionOpen || state.samplerOpen || state.panoramaOpen) {
        if (event.key === "Escape") {
          event.preventDefault();
          if (state.diffusionOpen) tools.closeDiffusion();
          else if (state.samplerOpen) tools.closeSampler();
          else tools.closePanoramaWizard();
        }
        return;
      }
      if (tools.publishOpen) {
        if (event.key === "Escape") {
          event.preventDefault();
          tools.togglePublishWizard(false);
        }
        if (event.target instanceof Element && event.target.closest(".publish-card")) return;
      }
      if (shortcutsOpen) {
        if (event.key === "Escape" || event.key === "?" || (event.key === "/" && event.shiftKey)) {
          event.preventDefault();
          setShortcutsOpen(false);
        }
        return;
      }
      if (event.target instanceof Element && event.target.matches("#tags, #notes, #min-rating")) return;
      const plain = !event.ctrlKey && !event.metaKey && !event.altKey;
      const image = currentImage(state);
      let task: Promise<void> | undefined;
      if (event.key === "Escape") {
        if (!state.histogramOpen && !state.informationOpen && !state.mobileDrawer) return;
        if (state.histogramOpen) update({ histogramOpen: false });
        else if (state.informationOpen) update({ informationOpen: false });
        else if (state.mobileDrawer) update({ mobileDrawer: null });
        event.preventDefault();
        return;
      }
      if (event.key === "?" || (event.key === "/" && event.shiftKey)) {
        event.preventDefault();
        setShortcutsOpen(true);
        return;
      }
      if (event.key === "," || event.key === "/") {
        event.preventDefault();
        if (mobile) update({ mobileDrawer: "metadata" });
        const ref = event.key === "," ? tagsRef : notesRef;
        /** Tags append at the end while notes select all, preserving both established entry shortcuts. */
        const focus = (): void => {
          ref.current?.focus();
          if (event.key === "/") ref.current?.select();
          else if (ref.current) ref.current.setSelectionRange(ref.current.value.length, ref.current.value.length);
        };
        if (mobile) window.requestAnimationFrame(focus);
        else focus();
        return;
      }
      if (state.cropEditing && plain && key === "r") return;
      if (plain && (key === "c" || key === "v")) {
        if (!image || isDirectCompressedImage(image) || isSoocProfile(selectedProfile(image, state))) return;
        event.preventDefault();
        if (key === "c") edits.copy();
        else edits.paste();
        return;
      }
      if (event.target instanceof Element && event.target.closest(".retouch, .crop-tools")) return;
      if (plain && key === "h") update({ histogramOpen: !state.histogramOpen });
      else if (plain && key === "i") update({ informationOpen: !state.informationOpen });
      else if (key === "f") task = toggleFullscreen(appRef.current);
      else if (event.key === "ArrowRight" || event.key === "Enter") task = actions.move(1);
      else if (event.key === "ArrowLeft") task = actions.move(-1);
      else if (event.key === "PageDown" || event.key === "PageUp")
        task = actions.stepProfile(event.key === "PageDown" ? 1 : -1);
      else if (event.key === " ") {
        const profile = selectedProfile(image, state);
        if (profile && !profilesAreImplicitOnly(state, image)) task = actions.toggleProfile(profile);
      } else if (event.key === "ArrowUp" || event.key === "ArrowDown")
        task = actions.rate((image?.rating || 0) + (event.key === "ArrowUp" ? 1 : -1));
      else if (["`", "§", "1", "2", "3", "4", "5"].includes(event.key))
        task = actions.rate(["`", "§"].includes(event.key) ? 0 : Number(event.key));
      else {
        const labels: Record<string, ReviewLabel> = {
          6: "red",
          7: "yellow",
          8: "green",
          9: "blue",
          0: "purple",
          r: "red",
          y: "yellow",
          g: "green",
          b: "blue",
          p: "purple",
          n: "none",
        };
        const label = labels[key];
        if (!label) return;
        task = actions.toggleLabel(label);
      }
      if (!["ArrowLeft", "ArrowRight", "Enter"].includes(event.key)) event.preventDefault();
      if (task) void task.catch(console.error);
    };
    window.addEventListener("keydown", keydown);
    return (): void => window.removeEventListener("keydown", keydown);
  }, [state, update, actions, edits, tools, shortcutsOpen, setShortcutsOpen, tagsRef, notesRef, mobile, modal, appRef]);

  /** Accumulate trackpad impulses and impose a cooldown so one gesture performs one navigation step. */
  return (event: JSX.TargetedWheelEvent<HTMLElement>): void => {
    if (modal || state.cropEditing || event.ctrlKey || event.metaKey || event.altKey) return;
    if (event.target instanceof Element && event.target.closest("input, textarea, select, .retouch, .crop-tools"))
      return;
    event.preventDefault();
    const factor =
      event.deltaMode === WheelEvent.DOM_DELTA_LINE
        ? 40
        : event.deltaMode === WheelEvent.DOM_DELTA_PAGE
          ? window.innerHeight
          : 1;
    const axis = Math.abs(event.deltaX) > Math.abs(event.deltaY) ? "x" : "y";
    const delta = (axis === "x" ? event.deltaX : event.deltaY) * factor;
    const now = performance.now();
    if (!Number.isFinite(delta) || Math.abs(delta) < 1 || now < wheel.current.lockedUntil) return;
    if (wheel.current.axis !== axis || now - wheel.current.lastAt > WHEEL_NAV_RESET_MS) wheel.current.amount = 0;
    wheel.current.axis = axis;
    wheel.current.amount += delta;
    wheel.current.lastAt = now;
    if (Math.abs(wheel.current.amount) < WHEEL_NAV_THRESHOLD_PX) return;
    const direction = Math.sign(wheel.current.amount);
    wheel.current.amount = 0;
    wheel.current.lockedUntil = now + WHEEL_NAV_COOLDOWN_MS;
    void (axis === "x" ? actions.move(direction) : actions.stepProfile(direction)).catch(console.error);
  };
}
