/** Generated from Rust wire contracts; regenerate with npm run contracts:generate. */
import { createOperation, type OperationOptions } from "../core/transport";
import { operations, type OperationContracts } from "./operations";
/** Named API methods prevent callers from inventing response types or forgetting route identities. */
export const reviewApi = {
  state: createOperation<
    OperationContracts["state"]["request"],
    OperationContracts["state"]["response"],
    OperationContracts["state"]["parameters"]
  >(operations["state"]),
  review: createOperation<
    OperationContracts["review"]["request"],
    OperationContracts["review"]["response"],
    OperationContracts["review"]["parameters"]
  >(operations["review"]),
  ui: createOperation<
    OperationContracts["ui"]["request"],
    OperationContracts["ui"]["response"],
    OperationContracts["ui"]["parameters"]
  >(operations["ui"]),
  burst: createOperation<
    OperationContracts["burst"]["request"],
    OperationContracts["burst"]["response"],
    OperationContracts["burst"]["parameters"]
  >(operations["burst"]),
  publish: createOperation<
    OperationContracts["publish"]["request"] | undefined,
    OperationContracts["publish"]["response"],
    OperationContracts["publish"]["parameters"]
  >(operations["publish"]),
  sampler_create: createOperation<
    OperationContracts["sampler_create"]["request"],
    OperationContracts["sampler_create"]["response"],
    OperationContracts["sampler_create"]["parameters"]
  >(operations["sampler_create"]),
  sampler_get: createOperation<
    OperationContracts["sampler_get"]["request"],
    OperationContracts["sampler_get"]["response"],
    OperationContracts["sampler_get"]["parameters"]
  >(operations["sampler_get"]),
  sampler_priority: createOperation<
    OperationContracts["sampler_priority"]["request"],
    OperationContracts["sampler_priority"]["response"],
    OperationContracts["sampler_priority"]["parameters"]
  >(operations["sampler_priority"]),
  sampler_select: createOperation<
    OperationContracts["sampler_select"]["request"],
    OperationContracts["sampler_select"]["response"],
    OperationContracts["sampler_select"]["parameters"]
  >(operations["sampler_select"]),
  diffusion_create: createOperation<
    OperationContracts["diffusion_create"]["request"],
    OperationContracts["diffusion_create"]["response"],
    OperationContracts["diffusion_create"]["parameters"]
  >(operations["diffusion_create"]),
  diffusion_get: createOperation<
    OperationContracts["diffusion_get"]["request"],
    OperationContracts["diffusion_get"]["response"],
    OperationContracts["diffusion_get"]["parameters"]
  >(operations["diffusion_get"]),
  diffusion_apply: createOperation<
    OperationContracts["diffusion_apply"]["request"],
    OperationContracts["diffusion_apply"]["response"],
    OperationContracts["diffusion_apply"]["parameters"]
  >(operations["diffusion_apply"]),
  diffusion_reset: createOperation<
    OperationContracts["diffusion_reset"]["request"],
    OperationContracts["diffusion_reset"]["response"],
    OperationContracts["diffusion_reset"]["parameters"]
  >(operations["diffusion_reset"]),
  panorama_create: createOperation<
    OperationContracts["panorama_create"]["request"],
    OperationContracts["panorama_create"]["response"],
    OperationContracts["panorama_create"]["parameters"]
  >(operations["panorama_create"]),
  panorama_update: createOperation<
    OperationContracts["panorama_update"]["request"],
    OperationContracts["panorama_update"]["response"],
    OperationContracts["panorama_update"]["parameters"]
  >(operations["panorama_update"]),
  panorama_previews: createOperation<
    OperationContracts["panorama_previews"]["request"] | undefined,
    OperationContracts["panorama_previews"]["response"],
    OperationContracts["panorama_previews"]["parameters"]
  >(operations["panorama_previews"]),
  panorama_render: createOperation<
    OperationContracts["panorama_render"]["request"] | undefined,
    OperationContracts["panorama_render"]["response"],
    OperationContracts["panorama_render"]["parameters"]
  >(operations["panorama_render"]),
} as const;
/** Derive exact call options for helper functions composing an existing operation. */
export type ReviewOperationOptions<Request, Params> = OperationOptions<Request, Params>;
