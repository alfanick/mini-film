/** Mount stable reactive tool views with narrowly scoped providers so every dialog follows current review state. */
import type { ComponentChildren } from "preact";
import { useReviewContext } from "../core/context";
import type { ToolsController } from "./use-tools";
import { Dialog } from "../components/Dialog";
import {
  InformationViewContext,
  type InformationViewDependencies,
  ProfileInfoOverlay,
  CommandInvocationOverlay,
} from "../features/information/view";
import { PublishViewContext, type PublishViewDependencies, PublishOverlay } from "../features/publish/view";
import { PanoramaViewContext, type PanoramaViewDependencies, PanoramaOverlay } from "../features/panorama/view";
import { SamplerViewContext, type SamplerViewDependencies, SamplerOverlay } from "../features/sampler/view";
import { DiffusionViewContext, type DiffusionViewDependencies, DiffusionOverlay } from "../features/diffusion/view";

/** Render dialogs declaratively; backdrop clicks use the same actions as buttons and keyboard shortcuts. */
export function ToolOverlays({
  tools,
  placement = "all",
}: {
  tools: ToolsController;
  placement?: "inside" | "outside" | "all";
}): ComponentChildren {
  const { state } = useReviewContext();
  const information: InformationViewDependencies = {
    closeCommandInvocation: tools.closeCommandInvocation,
    closeProfileInfo: tools.closeProfileInfo,
    findImage: tools.findImage,
    loadProfilePp3: tools.loadProfilePp3,
    profileByIndex: tools.profileByIndex,
    state: state,
  };
  const publish: PublishViewDependencies = {
    publishOpen: tools.publishOpen,
    publishForm: tools.publishForm,
    publishSubmitting: tools.publishSubmitting,
    publishError: tools.publishError,
    publishJob: tools.publishJob,
    publishRerender: tools.publishRerender,
    publishStats: tools.publishStats,
    togglePublishWizard: tools.togglePublishWizard,
    setPublishField: tools.setPublishField,
    togglePublishLabel: tools.togglePublishLabel,
    submitPublish: tools.submitPublish,
  };
  const panorama: PanoramaViewDependencies = {
    closePanoramaWizard: tools.closePanoramaWizard,
    currentPanoramaProject: tools.currentPanoramaProject,
    generatePanoramaPreviews: tools.generatePanoramaPreviews,
    minRating: tools.minRating,
    movePanoramaSource: tools.movePanoramaSource,
    renderPanoramaFinal: tools.renderPanoramaFinal,
    updatePanorama: tools.updatePanorama,
    selectPanoramaProject: tools.selectPanoramaProject,
    state: state,
    togglePanoramaSource: tools.togglePanoramaSource,
    updateSharedUi: tools.updateSharedUi,
  };
  const sampler: SamplerViewDependencies = {
    samplerRootRef: tools.samplerRootRef,
    closeSampler: tools.closeSampler,
    samplerSelectedEntry: tools.samplerSelectedEntry,
    selectSamplerEntry: tools.selectSamplerEntry,
    state: state,
    toggleSamplerSection: tools.toggleSamplerSection,
    updateSamplerSelection: tools.updateSamplerSelection,
  };
  const diffusion: DiffusionViewDependencies = {
    applyDiffusion: tools.applyDiffusion,
    closeDiffusion: tools.closeDiffusion,
    diffusionBeforeSource: tools.diffusionBeforeSource,
    diffusionMediaStyle: tools.diffusionMediaStyle,
    diffusionPreviewContext: tools.diffusionPreviewContext,
    diffusionStatusText: tools.diffusionStatusText,
    findImage: tools.findImage,
    requestDiffusionPreview: tools.requestDiffusionPreview,
    resetDiffusion: tools.resetDiffusion,
    setDiffusionSettings: tools.setDiffusionSettings,
    state: state,
  };
  return (
    <>
      {placement !== "inside" && (
        <InformationViewContext.Provider value={information}>
          <Dialog
            id="profile-info-overlay"
            className="profile-info-overlay"
            labelledBy="profile-info-title"
            label="Profile info"
            open={state.profileInfoProfileIndex !== null}
            onClose={tools.closeProfileInfo}
          >
            {state.profileInfoProfileIndex !== null && <ProfileInfoOverlay />}
          </Dialog>
          <Dialog
            id="command-invocation-overlay"
            className="command-invocation-overlay"
            labelledBy="command-invocation-title"
            label="Command invocation"
            open={state.commandInvocationOpen}
            onClose={tools.closeCommandInvocation}
          >
            {state.commandInvocationOpen && <CommandInvocationOverlay />}
          </Dialog>
        </InformationViewContext.Provider>
      )}
      {placement !== "inside" && (
        <PublishViewContext.Provider value={publish}>
          <PublishOverlay />
        </PublishViewContext.Provider>
      )}
      {placement !== "inside" && (
        <PanoramaViewContext.Provider value={panorama}>
          <Dialog
            id="panorama-overlay"
            className="panorama-overlay"
            labelledBy="panorama-title"
            label="Panorama"
            open={state.panoramaOpen}
            onClose={tools.closePanoramaWizard}
          >
            {state.panoramaOpen && <PanoramaOverlay />}
          </Dialog>
        </PanoramaViewContext.Provider>
      )}
      {placement !== "outside" && (
        <SamplerViewContext.Provider value={sampler}>
          <Dialog
            id="sampler-overlay"
            className="sampler-overlay"
            labelledBy="sampler-title"
            label="Sampler"
            open={state.samplerOpen}
            onClose={tools.closeSampler}
          >
            {state.samplerOpen && <SamplerOverlay />}
          </Dialog>
        </SamplerViewContext.Provider>
      )}
      {placement !== "outside" && (
        <DiffusionViewContext.Provider value={diffusion}>
          <Dialog
            id="diffusion-overlay"
            className="diffusion-overlay"
            labelledBy="diffusion-title"
            label="Diffusion"
            open={state.diffusionOpen}
            onClose={tools.closeDiffusion}
          >
            {state.diffusionOpen && <DiffusionOverlay />}
          </Dialog>
        </DiffusionViewContext.Provider>
      )}
    </>
  );
}
