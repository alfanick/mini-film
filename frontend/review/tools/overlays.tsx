/** Independent overlay leaves subscribe directly to feature models; closed forms do no catalog computation. */
import type { ComponentChildren } from "preact";
import { memo } from "preact/compat";
import { useToolModels } from "./context";
import { Dialog } from "../components/Dialog";
import { ErrorBoundary } from "../components/ErrorBoundary";
import { InformationViewContext, ProfileInfoOverlay, CommandInvocationOverlay } from "../features/information/view";
import { PublishViewContext, PublishOverlay } from "../features/publish/view";
import { PanoramaViewContext, PanoramaOverlay } from "../features/panorama/view";
import { SamplerViewContext, SamplerOverlay } from "../features/sampler/view";
import { DiffusionViewContext, DiffusionOverlay } from "../features/diffusion/view";

/** Information cards share only their own mutually exclusive dialog model. */
function InformationHost(): ComponentChildren {
  const model = useToolModels().information;
  const state = model.state.value;
  return (
    <InformationViewContext.Provider value={model}>
      <Dialog
        id="profile-info-overlay"
        className="profile-info-overlay"
        labelledBy="profile-info-title"
        label="Profile info"
        open={state.profileInfoProfileIndex !== null}
        onClose={model.closeProfileInfo}
      >
        {state.profileInfoProfileIndex !== null && (
          <ErrorBoundary>
            <ProfileInfoOverlay />
          </ErrorBoundary>
        )}
      </Dialog>
      <Dialog
        id="command-invocation-overlay"
        className="command-invocation-overlay"
        labelledBy="command-invocation-title"
        label="Command invocation"
        open={state.commandInvocationOpen}
        onClose={model.closeCommandInvocation}
      >
        {state.commandInvocationOpen && (
          <ErrorBoundary>
            <CommandInvocationOverlay />
          </ErrorBoundary>
        )}
      </Dialog>
    </InformationViewContext.Provider>
  );
}

/** Publish form visibility and jobs are independent of sampler/diffusion and keepalive pulses. */
function PublishHost(): ComponentChildren {
  const model = useToolModels().publish;
  return (
    <PublishViewContext.Provider value={model}>
      <ErrorBoundary>
        <PublishOverlay />
      </ErrorBoundary>
    </PublishViewContext.Provider>
  );
}

/** A panorama view can recover without restarting its provider-owned create/update operation. */
function PanoramaHost(): ComponentChildren {
  const model = useToolModels().panorama;
  const open = model.state.value.panoramaOpen;
  return (
    <PanoramaViewContext.Provider value={model}>
      <Dialog
        id="panorama-overlay"
        className="panorama-overlay"
        labelledBy="panorama-title"
        label="Panorama"
        open={open}
        onClose={model.closePanoramaWizard}
      >
        {open && (
          <ErrorBoundary>
            <PanoramaOverlay />
          </ErrorBoundary>
        )}
      </Dialog>
    </PanoramaViewContext.Provider>
  );
}

/** Only sampler dialog state controls its catalog leaf lifetime. */
function SamplerHost(): ComponentChildren {
  const model = useToolModels().sampler;
  const open = model.state.value.samplerOpen;
  return (
    <SamplerViewContext.Provider value={model}>
      <Dialog
        id="sampler-overlay"
        className="sampler-overlay"
        labelledBy="sampler-title"
        label="Sampler"
        open={open}
        onClose={model.closeSampler}
      >
        {open && (
          <ErrorBoundary>
            <SamplerOverlay />
          </ErrorBoundary>
        )}
      </Dialog>
    </SamplerViewContext.Provider>
  );
}

/** Diffusion media observes its target-bound model without subscribing the image workspace to preview polling. */
function DiffusionHost(): ComponentChildren {
  const model = useToolModels().diffusion;
  const open = model.state.value.diffusionOpen;
  return (
    <DiffusionViewContext.Provider value={model}>
      <Dialog
        id="diffusion-overlay"
        className="diffusion-overlay"
        labelledBy="diffusion-title"
        label="Diffusion"
        open={open}
        onClose={model.closeDiffusion}
      >
        {open && (
          <ErrorBoundary>
            <DiffusionOverlay />
          </ErrorBoundary>
        )}
      </Dialog>
    </DiffusionViewContext.Provider>
  );
}

/** Memoization keeps ordinary workspace renders from calling every closed tool leaf again. */
export const ToolOverlayHost = memo(function ToolOverlayHost({
  placement,
}: {
  placement: "inside" | "outside";
}): ComponentChildren {
  return placement === "inside" ? (
    <>
      <SamplerHost />
      <DiffusionHost />
    </>
  ) : (
    <>
      <InformationHost />
      <PublishHost />
      <PanoramaHost />
    </>
  );
});
