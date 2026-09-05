/** Compose independent tool hooks behind the small action interface consumed by the main review shell. */
import { useReviewContext } from "../core/context";
import type { ReviewImage } from "../core/types";
import { useSampler, type SamplerActions } from "./use-sampler";
import { useDiffusion, type DiffusionActions } from "./use-diffusion";
import { useInformation, type InformationActions } from "./use-information";
import { usePanorama, type PanoramaActions } from "./use-panorama";
import { usePublish, type PublishActions } from "./use-publish";
import type { ToolSessionActions } from "./types";

/** Combined tool commands plus the current-session selectors required by legacy-compatible view composition. */
export interface ToolsController
  extends SamplerActions, DiffusionActions, InformationActions, PanoramaActions, PublishActions {
  findImage(this: void, id: number | null): ReviewImage | null;
  minRating(this: void): number;
  updateSharedUi: ToolSessionActions["updateSharedUi"];
}

/** Mount each tool's lifecycle once while exposing fresh selectors and narrow session callbacks. */
export function useTools(session: ToolSessionActions): ToolsController {
  const { state } = useReviewContext();
  const sampler = useSampler();
  const diffusion = useDiffusion(session);
  const information = useInformation();
  const panorama = usePanorama(session);
  const publish = usePublish(session);
  return {
    ...sampler,
    ...diffusion,
    ...information,
    ...panorama,
    ...publish,
    findImage: (id) => state.data?.images.find((image) => image.id === id) || null,
    minRating: () => state.data?.ui.min_rating || 0,
    updateSharedUi: session.updateSharedUi,
  };
}
