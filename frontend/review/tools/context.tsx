/** Colocate tool lifecycle hooks and controlled forms away from the image workspace's render boundary. */
import { computed, createModel, signal, useModel, type ReadonlySignal } from "@preact/signals";
import { createContext, type ComponentChildren } from "preact";
import { useContext, useLayoutEffect } from "preact/hooks";
import { useTools, type ToolsController } from "./use-tools";
import { ToolOverlays } from "./overlays";
import type { ToolSessionActions } from "./types";

/** The workspace subscribes to visibility only; overlays subscribe to their complete current tool presentation. */
interface ToolModelValue {
  controller: ReadonlySignal<ToolsController>;
  publishOpen: ReadonlySignal<boolean>;
  replace: (controller: ToolsController) => void;
}

/** Keep a stable context identity even while sliders, polling or publish text update tool-local state. */
const ToolModel = createModel((initial: ToolsController): ToolModelValue => {
  const current = signal(initial);
  return {
    controller: current,
    publishOpen: computed(() => current.value.publishOpen),
    replace: (controller: ToolsController): void => {
      current.value = controller;
    },
  };
});
const ToolContext = createContext<ToolModelValue | null>(null);

/** Mount each tool's effects once without lifting its controlled inputs into the image workspace. */
export function ToolsProvider({
  session,
  children,
}: {
  session: ToolSessionActions;
  children: ComponentChildren;
}): ComponentChildren {
  const controller = useTools(session);
  const model = useModel(() => new ToolModel(controller));
  useLayoutEffect((): void => model.replace(controller), [controller, model]);
  return <ToolContext.Provider value={model}>{children}</ToolContext.Provider>;
}

/** Reject accidental use outside the owning provider instead of sharing singleton tool state. */
function useToolModel(): ToolModelValue {
  const model = useContext(ToolContext);
  if (!model) throw new Error("Review tools require ToolsProvider");
  return model;
}

/** Read stable commands without subscribing the workspace to form text, previews or polling results. */
export function useActiveTools(): ToolsController {
  const model = useToolModel();
  return { ...model.controller.peek(), publishOpen: model.publishOpen.value };
}

/** Keep the existing inside/outside layout while only open tool rendering reacts to tool-local changes. */
export function ToolOverlayHost({ placement }: { placement: "inside" | "outside" }): ComponentChildren {
  const model = useToolModel();
  return <ToolOverlays tools={model.controller.value} placement={placement} />;
}
