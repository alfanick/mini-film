/** Mount stable reactive tool views with narrowly scoped providers so every dialog follows current review state. */
import type { ComponentChildren } from "preact";
import { useReviewContext } from "../core/context";
import * as constants from "../core/constants";
import * as selectors from "../core/selectors";
import { reviewUrl } from "../core/api";
import type { ToolsController } from "./use-tools";
import {
  InformationViewContext,
  type InformationViewDependencies,
  ProfileInfoOverlay,
  CommandInvocationOverlay,
} from "../views/information";
import * as informationHelpers from "./information-helpers";
import { PublishViewContext, type PublishViewDependencies, PublishOverlay } from "../views/publish";
import * as publishHelpers from "./publish-helpers";
import { PanoramaViewContext, type PanoramaViewDependencies, PanoramaOverlay } from "../views/panorama";
import * as panoramaHelpers from "./panorama-helpers";
import { SamplerViewContext, type SamplerViewDependencies, SamplerOverlay } from "../views/sampler";
import * as samplerHelpers from "./sampler-helpers";
import { DiffusionViewContext, type DiffusionViewDependencies, DiffusionOverlay } from "../views/diffusion";
import * as diffusionHelpers from "./diffusion-helpers";

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
    commandInvocationCopyText: informationHelpers.commandInvocationCopyText,
    commandInvocationDisplayValue: informationHelpers.commandInvocationDisplayValue,
    commandInvocationLines: informationHelpers.commandInvocationLines,
    findImage: tools.findImage,
    loadProfilePp3: tools.loadProfilePp3,
    profileByIndex: tools.profileByIndex,
    profileDisplayName: selectors.profileDisplayName,
    profilePp3DownloadName: informationHelpers.profilePp3DownloadName,
    profilePp3Key: informationHelpers.profilePp3Key,
    profilePp3Url: informationHelpers.profilePp3Url,
    renderAdjustments: informationHelpers.renderAdjustments,
    renderGrain: informationHelpers.renderGrain,
    renderPp3Adjustments: informationHelpers.renderPp3Adjustments,
    renderSharpening: informationHelpers.renderSharpening,
    reviewUrl: reviewUrl,
    state: state,
    versionedUrl: selectors.versionedUrl,
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
    COLOR_LABELS: constants.COLOR_LABELS,
    RATING_VALUES: constants.RATING_VALUES,
    capitalize: selectors.capitalize,
    publishProgressPercent: publishHelpers.publishProgressPercent,
    reviewUrl: reviewUrl,
  };
  const panorama: PanoramaViewDependencies = {
    PANORAMA_MATCHING_MODES: constants.PANORAMA_MATCHING_MODES,
    PANORAMA_PROJECTIONS: constants.PANORAMA_PROJECTIONS,
    capitalize: selectors.capitalize,
    closePanoramaWizard: tools.closePanoramaWizard,
    currentPanoramaProject: tools.currentPanoramaProject,
    generatePanoramaPreviews: tools.generatePanoramaPreviews,
    minRating: tools.minRating,
    movePanoramaSource: tools.movePanoramaSource,
    panoramaStatusText: panoramaHelpers.panoramaStatusText,
    renderPanoramaFinal: tools.renderPanoramaFinal,
    updatePanorama: tools.updatePanorama,
    selectPanoramaProject: tools.selectPanoramaProject,
    state: state,
    togglePanoramaSource: tools.togglePanoramaSource,
    updateSharedUi: tools.updateSharedUi,
    versionedUrl: selectors.versionedUrl,
  };
  const sampler: SamplerViewDependencies = {
    samplerRootRef: tools.samplerRootRef,
    buildSamplerHierarchy: samplerHelpers.buildSamplerHierarchy,
    capitalize: selectors.capitalize,
    closeSampler: tools.closeSampler,
    reviewUrl: reviewUrl,
    samplerMediaStyle: samplerHelpers.samplerMediaStyle,
    samplerSelectedEntry: tools.samplerSelectedEntry,
    samplerStatusText: samplerHelpers.samplerStatusText,
    selectSamplerEntry: tools.selectSamplerEntry,
    state: state,
    toggleSamplerSection: tools.toggleSamplerSection,
    updateSamplerSelection: tools.updateSamplerSelection,
  };
  const diffusion: DiffusionViewDependencies = {
    DIFFUSION_DETAIL_AREAS: constants.DIFFUSION_DETAIL_AREAS,
    DIFFUSION_METHODS: constants.DIFFUSION_METHODS,
    DIFFUSION_PRESETS: constants.DIFFUSION_PRESETS,
    applyDiffusion: tools.applyDiffusion,
    closeDiffusion: tools.closeDiffusion,
    diffusionAfterSource: diffusionHelpers.diffusionAfterSource,
    diffusionBeforeSource: tools.diffusionBeforeSource,
    diffusionDetailFrameStyle: diffusionHelpers.diffusionDetailFrameStyle,
    diffusionDetailMediaStyle: diffusionHelpers.diffusionDetailMediaStyle,
    diffusionMediaStyle: tools.diffusionMediaStyle,
    diffusionPresetIsActive: diffusionHelpers.diffusionPresetIsActive,
    diffusionPresetSettings: diffusionHelpers.diffusionPresetSettings,
    diffusionPreviewContext: tools.diffusionPreviewContext,
    diffusionProfile: diffusionHelpers.diffusionProfile,
    diffusionSourceLabel: diffusionHelpers.diffusionSourceLabel,
    diffusionStatusText: tools.diffusionStatusText,
    findImage: tools.findImage,
    formatPercent: diffusionHelpers.formatPercent,
    normalizeDiffusionSettings: diffusionHelpers.normalizeDiffusionSettings,
    profileDisplayName: selectors.profileDisplayName,
    requestDiffusionPreview: tools.requestDiffusionPreview,
    resetDiffusion: tools.resetDiffusion,
    reviewUrl: reviewUrl,
    setDiffusionSettings: tools.setDiffusionSettings,
    state: state,
    versionedUrl: selectors.versionedUrl,
  };
  return (
    <>
      {placement !== "inside" && (
        <InformationViewContext.Provider value={information}>
          <div
            id="profile-info-overlay"
            class="profile-info-overlay"
            hidden={state.profileInfoProfileIndex === null}
            onClick={(event) => {
              if (event.target === event.currentTarget) tools.closeProfileInfo();
            }}
          >
            {state.profileInfoProfileIndex !== null && <ProfileInfoOverlay />}
          </div>
          <div
            id="command-invocation-overlay"
            class="command-invocation-overlay"
            hidden={!state.commandInvocationOpen}
            onClick={(event) => {
              if (event.target === event.currentTarget) tools.closeCommandInvocation();
            }}
          >
            {state.commandInvocationOpen && <CommandInvocationOverlay />}
          </div>
        </InformationViewContext.Provider>
      )}
      {placement !== "inside" && (
        <PublishViewContext.Provider value={publish}>
          <PublishOverlay />
        </PublishViewContext.Provider>
      )}
      {placement !== "inside" && (
        <PanoramaViewContext.Provider value={panorama}>
          <div
            id="panorama-overlay"
            class="panorama-overlay"
            hidden={!state.panoramaOpen}
            onClick={(event) => {
              if (event.target === event.currentTarget) tools.closePanoramaWizard();
            }}
          >
            {state.panoramaOpen && <PanoramaOverlay />}
          </div>
        </PanoramaViewContext.Provider>
      )}
      {placement !== "outside" && (
        <SamplerViewContext.Provider value={sampler}>
          <div
            id="sampler-overlay"
            class="sampler-overlay"
            hidden={!state.samplerOpen}
            onClick={(event) => {
              if (event.target === event.currentTarget) tools.closeSampler();
            }}
          >
            {state.samplerOpen && <SamplerOverlay />}
          </div>
        </SamplerViewContext.Provider>
      )}
      {placement !== "outside" && (
        <DiffusionViewContext.Provider value={diffusion}>
          <div
            id="diffusion-overlay"
            class="diffusion-overlay"
            hidden={!state.diffusionOpen}
            onClick={(event) => {
              if (event.target === event.currentTarget) tools.closeDiffusion();
            }}
          >
            {state.diffusionOpen && <DiffusionOverlay />}
          </div>
        </DiffusionViewContext.Provider>
      )}
    </>
  );
}
