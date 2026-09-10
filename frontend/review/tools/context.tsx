/** Stable provider-owned feature models survive view recovery without controller copying or broad subscriptions. */
import { computed, createModel, useModel, type ReadonlySignal } from "@preact/signals";
import { createContext, type ComponentChildren } from "preact";
import { useContext } from "preact/hooks";
import { useReviewSession } from "../session/context";
import { useReviewModel } from "../core/context";
import type { ReviewModelValue } from "../core/model";
import type { ToolSessionActions } from "./types";
import { InformationModel, type InformationModelValue } from "../features/information/model";
import { PanoramaModel, type PanoramaModelValue } from "../features/panorama/model";
import { PublishModel, type PublishActions } from "../features/publish/model";
import { SamplerModel, type SamplerModelValue } from "../features/sampler/model";
import { DiffusionModel, type DiffusionModelValue } from "../features/diffusion/model";

/** Context transports identities only; each feature view observes its own model signals directly. */
export interface ToolModels {
  information: InformationModelValue;
  panorama: PanoramaModelValue;
  publish: PublishActions;
  sampler: SamplerModelValue;
  diffusion: DiffusionModelValue;
  modalOpen: ReadonlySignal<boolean>;
}

/** Compose models in the provider's lifetime so closing or crashing a dialog never creates a second operation owner. */
const ReviewTools = createModel((catalog: ReviewModelValue, session: ToolSessionActions): ToolModels => {
  const information = new InformationModel(catalog);
  const panorama = new PanoramaModel(catalog, session);
  const publish = new PublishModel(catalog, session);
  const sampler = new SamplerModel(catalog, session);
  const diffusion = new DiffusionModel(catalog, session);
  return {
    information,
    panorama,
    publish,
    sampler,
    diffusion,
    modalOpen: computed(
      () =>
        information.state.value.profileInfoProfileIndex !== null ||
        information.state.value.commandInvocationOpen ||
        panorama.state.value.panoramaOpen ||
        publish.publishOpen.value ||
        sampler.state.value.samplerOpen ||
        diffusion.state.value.diffusionOpen,
    ),
  };
});
const ToolContext = createContext<ToolModels | null>(null);

/** Mount model ownership above all recoverable workspace and dialog view boundaries. */
export function ToolsProvider({ children }: { children: ComponentChildren }): ComponentChildren {
  const catalog = useReviewModel();
  const session = useReviewSession();
  const models = useModel(() => new ReviewTools(catalog, session));
  return <ToolContext.Provider value={models}>{children}</ToolContext.Provider>;
}

/** Reject accidental use outside the owning provider instead of sharing singleton tool state. */
export function useToolModels(): ToolModels {
  const model = useContext(ToolContext);
  if (!model) throw new Error("Review tools require ToolsProvider");
  return model;
}

/** The shell needs launch commands and one modal flag, not complete form/controller presentations. */
export interface ActiveTools {
  modalOpen: boolean;
  openProfileInfo: InformationModelValue["openProfileInfo"];
  openCommandInvocation: InformationModelValue["openCommandInvocation"];
  openPanoramaWizard: PanoramaModelValue["openPanoramaWizard"];
  openSampler: SamplerModelValue["openSampler"];
  openDiffusion: DiffusionModelValue["openDiffusion"];
  togglePublishWizard: PublishActions["togglePublishWizard"];
}

/** Observe only publish visibility; all exported launch functions retain their model-bound identity. */
export function useActiveTools(): ActiveTools {
  const models = useToolModels();
  return {
    modalOpen: models.modalOpen.value,
    openProfileInfo: models.information.openProfileInfo,
    openCommandInvocation: models.information.openCommandInvocation,
    openPanoramaWizard: models.panorama.openPanoramaWizard,
    openSampler: models.sampler.openSampler,
    openDiffusion: models.diffusion.openDiffusion,
    togglePublishWizard: models.publish.togglePublishWizard,
  };
}
