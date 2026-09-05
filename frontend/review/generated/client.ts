/** Generated from Rust wire contracts; regenerate with npm run contracts:generate. */
import { createOperation, type OperationOptions } from "../core/transport";
import { operations, type OperationContracts } from "./operations";
/** Named API methods prevent callers from inventing response types or forgetting route identities. */
export const reviewApi = {
  state: createOperation<
    OperationContracts["state"]["request"],
    OperationContracts["state"]["response"],
    Record<never, never>
  >(operations["state"]),
  review: createOperation<
    OperationContracts["review"]["request"],
    OperationContracts["review"]["response"],
    Record<never, never>
  >(operations["review"]),
  ui: createOperation<OperationContracts["ui"]["request"], OperationContracts["ui"]["response"], Record<never, never>>(
    operations["ui"],
  ),
  burst: createOperation<
    OperationContracts["burst"]["request"],
    OperationContracts["burst"]["response"],
    { burst_id: string | number }
  >(operations["burst"]),
  publish: createOperation<
    OperationContracts["publish"]["request"] | undefined,
    OperationContracts["publish"]["response"],
    Record<never, never>
  >(operations["publish"]),
  sampler_create: createOperation<
    OperationContracts["sampler_create"]["request"],
    OperationContracts["sampler_create"]["response"],
    Record<never, never>
  >(operations["sampler_create"]),
  sampler_get: createOperation<
    OperationContracts["sampler_get"]["request"],
    OperationContracts["sampler_get"]["response"],
    { job_id: string | number }
  >(operations["sampler_get"]),
  sampler_priority: createOperation<
    OperationContracts["sampler_priority"]["request"],
    OperationContracts["sampler_priority"]["response"],
    { job_id: string | number }
  >(operations["sampler_priority"]),
  sampler_select: createOperation<
    OperationContracts["sampler_select"]["request"],
    OperationContracts["sampler_select"]["response"],
    { job_id: string | number; entry_key: string | number }
  >(operations["sampler_select"]),
  diffusion_create: createOperation<
    OperationContracts["diffusion_create"]["request"],
    OperationContracts["diffusion_create"]["response"],
    Record<never, never>
  >(operations["diffusion_create"]),
  diffusion_get: createOperation<
    OperationContracts["diffusion_get"]["request"],
    OperationContracts["diffusion_get"]["response"],
    { job_id: string | number }
  >(operations["diffusion_get"]),
  diffusion_apply: createOperation<
    OperationContracts["diffusion_apply"]["request"],
    OperationContracts["diffusion_apply"]["response"],
    Record<never, never>
  >(operations["diffusion_apply"]),
  diffusion_reset: createOperation<
    OperationContracts["diffusion_reset"]["request"],
    OperationContracts["diffusion_reset"]["response"],
    Record<never, never>
  >(operations["diffusion_reset"]),
  panorama_create: createOperation<
    OperationContracts["panorama_create"]["request"],
    OperationContracts["panorama_create"]["response"],
    Record<never, never>
  >(operations["panorama_create"]),
  panorama_update: createOperation<
    OperationContracts["panorama_update"]["request"],
    OperationContracts["panorama_update"]["response"],
    { project_id: string | number }
  >(operations["panorama_update"]),
  panorama_previews: createOperation<
    OperationContracts["panorama_previews"]["request"] | undefined,
    OperationContracts["panorama_previews"]["response"],
    { project_id: string | number }
  >(operations["panorama_previews"]),
  panorama_render: createOperation<
    OperationContracts["panorama_render"]["request"] | undefined,
    OperationContracts["panorama_render"]["response"],
    { project_id: string | number }
  >(operations["panorama_render"]),
} as const;
/** Derive exact call options for helper functions composing an existing operation. */
export type ReviewOperationOptions<Request, Params> = OperationOptions<Request, Params>;
